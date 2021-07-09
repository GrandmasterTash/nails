pub mod docker;
pub mod shared;

use uuid::Uuid;
use futures::Future;
use self::shared::CONTAINERS;
use std::{sync::Arc, time::Duration};
use actix_http::{Request, http::Method};
use actix_service::{Service, ServiceFactory};
use actix_web::{App, dev::{Body, ServiceRequest, ServiceResponse}, test::{TestRequest, call_service}};

// Interesting - test runner: https://dev.to/tjtelan/how-to-build-a-custom-integration-test-harness-in-rust-7n7

///
/// Get an instance of an App with routes and connections to mongo and rabbit setup.
///
pub async fn start_app() -> App<
    impl ServiceFactory<
        Request = ServiceRequest,
        Config = (),
        Response = ServiceResponse,
        Error = actix_web::Error,
        InitError = ()>,
    Body> {

    let ctx = match nails::init_everything().await {
        Ok(ctx) => ctx.0,
        Err(err) => panic!(format!("init_everthing failed: {}", err.to_string()))
    };
    nails::app(Arc::new(ctx))
}

///
/// Run set-up and teardown before the actual test logic.
///
/// This allows us to ref-count the tests using any launched docker containers and we
/// can tear-down those containers when the last test calls teardown.
///
pub async fn run_test<T: 'static>(test: T) -> ()
    where
        T: Future,
{
    setup();

    test.await;
    // BUG: If a test panics, teardown isn't called.

    // Small non-blocking delay needed to avoid fast tests starting and stopping mongo
    // before other tests get a chance to start. This delay needs to be just enough to allow
    // the next test to acquire the mutex guard around the shared resources.
    tokio::time::delay_for(Duration::from_millis(10)).await; // Tokio 0.2.x
    // tokio::time::sleep(Duration::from_millis(10)).await; // Tokio 1.x

    teardown();
}

///
/// Start docker containers if they are not already.
///
fn setup() {
    // Point all downstreams to the mock server.
    std::env::set_var("AUTH_ADDRESS", mockito::server_url());

    // Repeatedly initialising the Jaeger tracing causes panics in the logs during tests.
    std::env::set_var("DISTRIBUTED_TRACING", "false");

    if !use_existing_containers() {
        let containers = CONTAINERS.clone();
        let mut containers = containers.lock().unwrap();
        containers.start();
    }
}

///
/// Stop docker containers if we're the last test.
///
fn teardown() {
    if !use_existing_containers() {
        let containers = CONTAINERS.clone();
        let mut containers = containers.lock().unwrap();
        containers.stop()
    }
}

fn use_existing_containers() -> bool {
    match std::env::var("TESTS_USE_EXISTING_CONTAINERS") {
        Ok(value) => value.to_lowercase().eq("true"),
        Err(_) => false,
    }
}

pub fn new_uuid() -> String {
    Uuid::new_v4().to_hyphenated().to_string()
}

///
/// Set the time inside the running service to be a fixed value. Must be an ISO8601
/// format, eg. "2020-02-01T12:30:00.123Z"
///
pub async fn freeze_time<S, B, E>(service: &mut S, fixed_time: &str)
where
    S: Service<Request = Request, Response = ServiceResponse<B>, Error = E>,
    E: std::fmt::Debug,
{
    let req = TestRequest::with_uri(&format!("/set_time/{}", fixed_time)).method(Method::POST).to_request();
    let resp = call_service(service, req).await;
    assert_eq!(200, resp.status());
}



// _    _ _______ _______ _____
// | |  | |__   __|__   __|  __ \
// | |__| |  | |     | |  | |__) |
// |  __  |  | |     | |  |  ___/
// | |  | |  | |     | |  | |
// |_|  |_|  |_|     |_|  |_|
//
// A wrapper around the actix test wrapper around the actix web client.
//
// Why? Just makes your tests a little less verbose. And we love writing lots of tests don't we!
//
// It's similar to the one in the main utils::http module but subtly different. It panics and doesn't
// have a retry implementation.
//
pub mod http {
    use futures::StreamExt;
    use actix_service::Service;
    use std::collections::HashMap;
    use serde::{Serialize, de::DeserializeOwned};
    use actix_web::{dev::ServiceResponse, test, web::BytesMut};
    use actix_http::{body::MessageBody, http::{HeaderValue, Method}};

    pub struct HttpRequest {
        url: String,
        method: Method,
        body: Option<Vec<u8>>, // If we ever need more than JSON, change this to a new enum None, Json(T), Form(T), Text(T), etc.
        headers: HashMap<String, String>,
        query_params: HashMap<&'static str, &'static str>,
    }

    impl HttpRequest {
        fn new(method: Method, url: String) -> Self {
            HttpRequest {
                url,
                body: None,
                method,
                headers: HashMap::new(),
                query_params: HashMap::new(),
            }
        }

        pub fn header(&mut self, name: &str, value: &str) -> &mut Self {
            self.headers.insert(name.to_string(), value.to_string());
            self
        }

        pub fn query_param(&mut self, name: &'static str, value: &'static str) -> &mut Self {
            self.query_params.insert(name, value);
            self
        }

        pub fn body<T: Serialize>(&mut self, body: T) -> &mut Self {
            self.body = Some(serde_json::to_vec(&body).expect(&format!("Cant set test body for {}", self.url)));
            self
        }

        pub async fn send<S, B, E>(&mut self, app: &mut S) -> HttpResponse<B>
        where
            S: Service<Request = actix_http::Request, Response = ServiceResponse<B>, Error = E>,
            E: std::fmt::Debug,
        {
            // Build an actix web client request.
            let mut req = test::TestRequest::with_uri(&self.url).method(self.method.clone());

            for query_param in &self.query_params {
                req = req.param(&query_param.0, &query_param.1);
            }

            // Append all the specified header.
            for header in &self.headers {
                req = req.header(header.0, HeaderValue::from_str(header.1.as_str()).expect(&format!("Failed to set header value {}", header.1)));
            }

            // Make the request now with the appropriate body type.
            if let Some(bytes) = &self.body {
                req = req.set_payload(bytes.clone());
            }

            let req = req.to_request();
            let resp = app.call(req).await.unwrap();

            HttpResponse {
                url: self.url.clone(),
                method: self.method.clone(),
                inner: resp
            }
        }
    }
    pub struct HttpResponse<B> {
        url: String,     // The original request URL.
        method: Method,  // The original request HTTP method.
        inner: ServiceResponse<B>
    }

    impl <B> HttpResponse<B>
    where B: MessageBody + Unpin
    {
        pub fn status(&self) -> u16 {
            self.inner.status().as_u16()
        }

        pub fn _url(&self) -> &str {
            &self.url
        }

        pub fn _method(&self) -> Method {
            self.method.clone()
        }

        pub async fn read_body<T: DeserializeOwned>(&mut self) -> T {
            // Lifted from actix_web::test::read_body_json
            let mut body = self.inner.take_body();
            let mut bytes = BytesMut::new();
            while let Some(item) = body.next().await {
                bytes.extend_from_slice(&item.unwrap());
            }
            let bytes = bytes.freeze();

            serde_json::from_slice(&bytes).expect(&format!("Failed to read json response for {} {}", self.method, self.url))
        }
    }

    #[allow(dead_code)]
    pub fn post(url: &str) -> HttpRequest {
        HttpRequest::new(Method::POST, url.to_string())
    }

    #[allow(dead_code)]
    pub fn put(url: &str) -> HttpRequest {
        HttpRequest::new(Method::PUT, url.to_string())
    }

    #[allow(dead_code)]
    pub fn get(url: &str) -> HttpRequest {
        HttpRequest::new(Method::GET, url.to_string())
    }

    #[allow(dead_code)]
    pub fn delete(url: &str) -> HttpRequest {
        HttpRequest::new(Method::DELETE, url.to_string())
    }
}


// _____       _     _     _ _   __  __  ____
// |  __ \     | |   | |   (_) | |  \/  |/ __ \
// | |__) |__ _| |__ | |__  _| |_| \  / | |  | |
// |  _  // _` | '_ \| '_ \| | __| |\/| | |  | |
// | | \ \ (_| | |_) | |_) | | |_| |  | | |__| |
// |_|  \_\__,_|_.__/|_.__/|_|\__|_|  |_|\___\_\
//
pub mod rabbit {
    use uuid::Uuid;
    use serde_json::Value;
    use futures::StreamExt;
    use tokio::task::{self, JoinHandle};
    use std::{sync::{Arc, Mutex}, time::{Duration, Instant}};
    use assert_json_diff::{CompareMode, Config, assert_json_matches_no_panic};
    use lapin::{Connection, ConnectionProperties, ExchangeKind, options::{BasicAckOptions, BasicConsumeOptions, ExchangeDeclareOptions, QueueBindOptions, QueueDeclareOptions}, types::FieldTable};

    #[derive(Debug)]
    pub struct RabbitMessage {
        payload: String
    }

    pub struct TestRabbitListener {
        messages: Arc<Mutex<Vec<RabbitMessage>>>,
        _join_handle: JoinHandle<()>,
    }

    impl TestRabbitListener {
        pub async fn assert_payload_received(&self, expected: Value) -> RabbitMessage {
            let started = Instant::now();
            loop {
                {
                    let lock = self.messages.lock().expect("unable to lock rabbit messsage");

                    // Check each capture message (so far) to see if the expected payload
                    // has been recieved.
                    for message in &*lock {
                        let actual: Value = serde_json::from_str(&message.payload).expect("Rabbit payload wasn't JSON");
                        match assert_json_matches_no_panic(&actual, &expected, Config::new(CompareMode::Strict)) {
                            Ok(_)  => {
                                return RabbitMessage{ payload: message.payload.clone() }
                            },
                            Err(_err) => {
                                // These messages aren't the same, maybe the next one is?
                                // Uncomment this next line if your test isn't finding the message you're expecting.
                                // println!("{}", _err);
                                ()
                            },
                        };
                    }
                }

                if (Instant::now() - started) > Duration::from_secs(10) {
                    panic!("Failed to get expected RabbitMQ message");
                }

                actix_rt::time::delay_for(Duration::from_millis(200)).await;
            }
        }
    }

    pub async fn listen_to_topic(topic: &'static str) -> TestRabbitListener {
        let messages = Arc::new(Mutex::new(Vec::<RabbitMessage>::new()));
        let inner_messages = messages.clone();

        let join_handle = task::spawn_blocking(move || {
            tokio::spawn(async move {
                // Connect to rabbit.
                let connection = Connection::connect("amqp://admin:changeme@localhost:5672", ConnectionProperties::default()).wait().expect("No test rabbit connection");
                let channel = connection.create_channel().wait().expect("No test channel");
                let queue_name = format!("test-{}", Uuid::new_v4().to_hyphenated().to_string());

                // Bind our test queue to the exchange.
                channel.exchange_declare(
                    "platform.events",
                    ExchangeKind::Topic,
                    ExchangeDeclareOptions { durable: true, ..ExchangeDeclareOptions::default() },
                    FieldTable::default()).wait().expect("cant declare test exchange");

                let _queue = channel
                    .queue_declare(
                        &queue_name,
                        QueueDeclareOptions { auto_delete: true, ..QueueDeclareOptions::default() },
                        FieldTable::default(),
                    )
                    .wait().expect("Cant create test queue");

                channel.queue_bind(
                    &queue_name,
                    "platform.events",
                    topic,
                    QueueBindOptions::default(),
                    FieldTable::default())
                    .wait().expect("cant bind");

                // Listen for messages.
                let mut consumer = channel
                    .basic_consume(
                        &queue_name,
                        "test-consumer",
                        BasicConsumeOptions::default(),
                        FieldTable::default(),
                    )
                    .wait().expect("cant consume");

                while let Some(msg) = consumer.next().await {
                    let (_channel, delivery) = msg.expect("error in consumer");
                    delivery
                        .ack(BasicAckOptions::default())
                        .await
                        .expect("ack");

                    // Pop any received messages in a list to check later.
                    let message = RabbitMessage { payload: String::from_utf8_lossy(&delivery.data).to_string() };
                    inner_messages.lock().expect("unable to lock rabbit messages").push(message);
                }
            });

            () // JoinHandle needs a type.
        });

        TestRabbitListener { _join_handle: join_handle, messages: messages.clone() }
    }
}