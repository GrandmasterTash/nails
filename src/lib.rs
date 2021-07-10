mod utils;
mod model;
mod routes;
mod clients;
mod middleware;

use tracing::info;
use dotenv::dotenv;
use std::sync::Arc;
use crossbeam_channel::bounded;
use actix_service::ServiceFactory;
use opentelemetry_jaeger::Uninstall;
use middleware::{request_body_log, request_id, response_body_log};
use actix_web_opentelemetry::RequestTracing as OpenTelemetryMiddleware;
use opentelemetry::{global, sdk::{propagation::TraceContextPropagator,trace,trace::Sampler}};
use tracing_subscriber::{prelude::__tracing_subscriber_SubscriberExt, Registry, util::SubscriberInitExt};
use actix_web::{App, HttpServer, body::Body, dev::{ServiceRequest, ServiceResponse}, middleware::Condition, web, web::Data};
use utils::{config::{Configuration, default_env}, context::{InitialisationContext, PartialRequestContext}, errors::{configure_json_extractor, InternalError}, mongo::{get_mongo_db, update_mongo}, rabbit::rabbit_publisher};
use routes::{admin::{health, ping, set_time, settings, tracer}, create_account, get_account, get_account_profile, get_accounts, get_device_profile, update_account};

// TODO: Propagate span context into middleware so logged errors are within a span.
//    This will require a newer actix_otel see https://github.com/OutThereLabs/actix-web-opentelemetry/pull/60/commits/66ce5b5b16b32004f1374263b60adf0f3141fe71
//    If we can't do this, create an ExternalError that builds from an InternalError + RequestContext and have request id logged.

pub const APP_NAME: &'static str = "Nails"; // Keep in sync with cargo.toml

///
/// The HTTP endpoints are wired-in here.
///
fn configure_routes(cfg: &mut web::ServiceConfig) {
    cfg
        // Admin/internal
        .route("/ping", web::get().to(ping::handle))
        .route("/health", web::get().to(health::handle))
        .route("/settings", web::get().to(settings::handle))
        .route("/tracer/on", web::post().to(tracer::handle_on))
        .route("/tracer/off", web::post().to(tracer::handle_off))
        .route("/set_time/{fixed_time}", web::post().to(set_time::handle_set))
        .route("/reset_time", web::post().to(set_time::handle_reset))

        // Account
        .route("/account/{account_id}", web::get().to(get_account::handle))
        .route("/accounts", web::get().to(get_accounts::handle))
        .route("/create-account", web::post().to(create_account::handle))
        .route("/update-account-status", web::put().to(update_account::handle_status))

        // Profiles
        .route("/account-profile/{profile_id}", web::get().to(get_account_profile::handle))
        .route("/device-profile/{profile_id}", web::get().to(get_device_profile::handle));
}

///
/// Initialise MongoDB, RabbitMQ, etc, and start the HTTP server.
///
/// Called from main.rs. The split from binary to library means we can write integration tests
/// in the /tests folder which can call various public methods in this file to create the service
/// with near-identical set-up as the runtime instance.
///
pub async fn lib_main() -> Result<(), std::io::Error> {
    let (ctx, _uninstall) = init_everything().await?;
    let init_ctx = Arc::new(ctx);
    let server_cfg = init_ctx.config().clone();

    // Use this to expose metrics for prometheus.
    // let exporter = opentelemetry_prometheus::exporter().init();
    // let request_metrics = actix_web_opentelemetry::RequestMetrics::new(
    //     opentelemetry::global::meter("actix_web"),
    //     Some(|req: &actix_web::dev::ServiceRequest| {
    //         req.path() == "/metrics" && req.method() == actix_web::http::Method::GET
    //     }),
    //     Some(exporter),
    // );

    // Start the HTTP server now, spawning an App for each worker thread.
    HttpServer::new(move || app(init_ctx.clone())
        // .wrap(request_metrics.clone()) // Prometheus metrics for each endpoint.

        // Add here not in app due to change in ServiceFactory signature.
        .wrap(response_body_log::Logging))
        .bind(format!("0.0.0.0:{}", server_cfg.port))?
        .keep_alive(server_cfg.keep_alive)
        .client_timeout(server_cfg.client_timeout)
        .run()
        .await
}

///
/// Initialise configuration, tracing. Connect to MongoDB and connect to RabbitMQ.
///
/// Return a context object which can be passed into HTTP request handlers to access config,
/// MongoDB, RabbitMQ (via publisher), a HTTP client, etc. Also return the Jaeger guard which,
/// when dropped, will terminate the Jaeger tracing pipeline.
///
pub async fn init_everything() -> Result<(InitialisationContext, Option<Uninstall>), InternalError> {
    // Load any local dev settings as environment variables from a .env file.
    dotenv().ok();

    // Default log level to INFO if it's not specified.
    default_env("RUST_LOG", "INFO");

    // Load the service configuration into struct and initialise any lazy statics.
    let config = Configuration::from_env().expect("The service configuration is not correct");

    // Initialise open-telemetry distributed tracing.
    let uninstall = init_tracing(&config);

    info!("{}\n{}", BANNER, config.fmt_console()?);

    // Create a MongoDB client and connect to it before proceeding.
    let db = get_mongo_db(APP_NAME, &config).await?;

    // Ensure the schema is in sync with the code.
    update_mongo(&db).await?;

    // Notifications are done with RabbitMQ. The publisher of rabbit messages runs in it's own thread and we
    // use an internal channel (crossbeam) to send notifications from HTTP request handler threads to this
    // RabbitMQ thread - which in-turn, transmits the message over the wire. This means the handlers are not blocked
    // and can use a fire-and-forget approach to notifications.
    let rabbit_config = config.clone();
    let (tx, rx) = bounded(config.notification_queue_size);
    // TODO: Investigate tokio blocking.... couldn't get it to work originally. This could allow us
    // to handle send failures in the HTTP handlers themselves if required. The current, external thread
    // implementation is a barrier to that.
    std::thread::spawn(move || rabbit_publisher(rx, APP_NAME, rabbit_config));

    // Create a context object that can be used as a parameter in any HTTP request handler.
    // Actix_web will wrap in a Data wrapper (essentially an Arc) and share it amongst each
    // worker thread.
    Ok((InitialisationContext::new(db, config.clone(), tx.clone()), uninstall))
}

///
/// Initialise tracing and plug-in the Jaeger feature if enabled.
///
fn init_tracing(config: &Configuration) -> Option<Uninstall> {
    global::set_text_map_propagator(TraceContextPropagator::new());

    let jaeger = match config.distributed_tracing {
        true => { // Install the Jaeger pipeline.
            let (tracer, uninstall) = opentelemetry_jaeger::new_pipeline()
                .with_service_name(APP_NAME)
                .with_trace_config(trace::config().with_default_sampler(Sampler::AlwaysOn))
                .with_agent_endpoint(config.jaeger_endpoint.clone().unwrap_or_default())
                .install()
                .expect("Unable to build Jaeger pipeline");
            Some((tracer, uninstall))
        },
        false => None
    };

    match jaeger {
        Some((tracer, uninstall)) => {
            if let Err(err) = Registry::default()
                .with(tracing_subscriber::EnvFilter::from_default_env()) // Set the tracing level to match RUST_LOG env variable.
                .with(tracing_subscriber::fmt::layer().with_test_writer())
                .with(tracing_opentelemetry::layer().with_tracer(tracer))
                .try_init() {
                    info!("Tracing already initialised: {}", err.to_string()); // Allowed error here - tests call this fn repeatedly.
            }
            return Some(uninstall)
        },
        None => {
            if let Err(err) = Registry::default()
                .with(tracing_subscriber::EnvFilter::from_default_env()) // Set the tracing level to match RUST_LOG env variable.
                .with(tracing_subscriber::fmt::layer().with_test_writer())
                .try_init() {
                    info!("Tracing already initialised: {}", err.to_string()); // Allowed error here - tests call this fn repeatedly.
            }
            return None
        },
    };
}

// Want to test capturing tracing span in middleware.
// fn render_error<B>(mut res: actix_web::dev::ServiceResponse<B>) -> actix_web::Result<actix_web::middleware::errhandlers::ErrorHandlerResponse<B>> {
//     error!("TEST");
//     res.response_mut()
//        .headers_mut()
//        .insert(actix_http::http::header::CONTENT_TYPE, actix_http::http::HeaderValue::from_static("Error"));
//     Ok(actix_web::middleware::errhandlers::ErrorHandlerResponse::Response(res))
// }

///
/// Create a configured actix_web HttpServer App with configured middleware, data and routes.
///
pub fn app(ctx: Arc<InitialisationContext>) -> App<
    impl ServiceFactory<
        Request = ServiceRequest,
        Config = (),
        Response = ServiceResponse,
        Error = actix_web::Error,
        InitError = ()>,
    Body> {

    App::new()
        .wrap(request_body_log::Logging)
        .wrap(request_id::Middleware::new(Data::new(PartialRequestContext::from(ctx.clone()))))

        // .wrap(ErrorHandlers::new().handler(StatusCode::BAD_REQUEST, render_error))

        // Enable open-telemetry tracing on incoming requests.
        .wrap(Condition::new(ctx.config().distributed_tracing, OpenTelemetryMiddleware::new()))

        // Ensure all endpoints return detailed Json request parse errors.
        .app_data(configure_json_extractor())

        // Add the routes to this root url path.
        .service(web::scope(&ctx.config().base_url).configure(configure_routes))
            // .wrap(actix_web_opentelemetry::RequestTracing::new())
}

const BANNER: &str = r#"
     __      _ _                    ,
  /\ \ \__ _(_) |___   Rust        /(  ___________
 /  \/ / _` | | / __|  MongoDB    |  >:===========`
/ /\  / (_| | | \__ \  RabbitMQ    )(
\_\ \/ \__,_|_|_|___/  Actix Web   ""
"#;