use uuid::Uuid;
use serde_json::Value;
use parking_lot::RwLock;
use lazy_static::lazy_static;
use std::{fs, time::Duration};
use tracing::{debug, error, info, warn};
use backoff::{ExponentialBackoff, retry_notify};
use super::{context::RequestContext, errors::InternalError};
use crate::{routes::admin::tracer, utils::config::Configuration};
use crossbeam_channel::{Receiver, RecvTimeoutError::Timeout, Sender};
use lapin::{BasicProperties, Channel, Connection, ConnectionProperties, ExchangeKind, options::{BasicPublishOptions, ExchangeDeclareOptions}, types::{AMQPValue, FieldTable, ShortString}};

//
// This file contains all the rabbit publishing code. Each HTTP handler is given a crossbeam
// channel (in the RequestContext) to send Notifications to, eg: -
//
//    notify(<topic>).body(<json>).send(&ctx);
//
// The RabbitMQ publisher runs in it's own thread and will pick these notifications up and
// send to the RabbitMQ exchange. At present, if the transmission fails to reach RabbitMQ, the
// original handler cannot respond to the error.
//

pub mod prelude {
    pub const TOPIC_ACCOUNT_CREATED: &str = "account.created";
    pub const TOPIC_ACCOUNT_STATUS_UPDATED: &str = "account.status.updated";
}

lazy_static! {
    ///
    /// The RabbitMQ publisher runs in a single thread and part of it's event loop is to check the
    /// connection status (and re-connect if not open). This flag tracks the known state of the connection
    /// and can be used by the health check to indicate if the RabbitMQ connection is healthy or not.
    ///
    pub static ref RABBIT_CONNECTED: RwLock<bool> = RwLock::new(false);
}

pub struct NotificationRequest {
    topic: &'static str,
    body: Option<Value>
}

impl NotificationRequest {
    pub fn body(&mut self, body: Value) -> &mut Self {
        self.body = Some(body);
        self
    }

    ///
    /// Asynchronously send the message to RabbitMQ. The caller cannot action any failure (currently).
    ///
    pub fn send(&mut self, ctx: &RequestContext) {
        ctx.publisher()
            .fire_and_forget(Notification::new(
                self.topic,
                self.body.clone().unwrap_or_default(),
                ctx.request_id()));
    }
}

pub fn notify(topic: &'static str) -> NotificationRequest {
    NotificationRequest { topic, body: None }
}

///
/// This is a communication channel to send notifications to another thread who is responsible for
/// external messages being sent.
///
pub type Publisher = Sender<Notification>;

///
/// An internal notifcation to a publisher thread which will send an external async RabbitMQ message.
///
#[derive(Debug)]
pub struct Notification {
    topic: &'static str, // The routing key to send the message via.
    version: u16,        // The body schema version - allows for breaking mutation of message structure.
    request_id: String,  // The correlation-id of the initiating request.
    body: Value          // The JSON representation of the message body.
}

impl Notification {
    pub fn new(topic: &'static str, body: Value, request_id: &str) -> Self {
        Notification { topic, body, request_id: request_id.to_string(), version: 1 }
    }
}

pub trait FireAndForget {
    ///
    /// Send the notification via a local channel to another thread. This allows the call to
    /// return asap whilst the notification is sent to a specific publisher thread
    /// which will send the notification to the external system.
    ///
    /// The call is blocked if the local crossbeam channel buffer is full. Errors in the recipient
    /// thread (the sender) are logged and are non-fatal (i.e. they do not effect the client caller).
    ///
    fn fire_and_forget(&self, notification: Notification);
}

impl FireAndForget for Publisher {
    #[tracing::instrument(name="fire_and_forget", level="info")]
    fn fire_and_forget(&self, notification: Notification) {
        if let Err(err) = self.send(notification) {
            error!("Failed to send notification {}", err);
        }
    }
}

///
/// Pass both the channel and connection around together. This allows us to switch them
/// out if there's a connection error.
///
struct RabbitConnection {
    connection: Connection,
    channel: Channel,
}

///
/// Create a back-off policy for retrying operations.
///
/// If the max elapsed time is None, the operation will retry forever.
///
fn backoff(timeout: Option<Duration>) -> ExponentialBackoff {
    ExponentialBackoff {
        multiplier: 3.0,
        max_elapsed_time: timeout,
        ..Default::default()
    }
}

///
/// Attempt to connect to RabbitMQ, retrying on any failure.
///
fn connect(config: &Configuration, timeout: Option<Duration>) -> Result<(Connection, Channel), InternalError> {
    info!("Connecting to RabbitMQ...");

    let uri = match &config.rabbit_credentials {
        Some(filename) => {
            debug!("Loading RabbitMQ credentials from secrets file {}", filename);

            // Read username and password from a secrets file.
            let credentials = fs::read_to_string(filename).map_err(|err| InternalError::UnableToReadCredentials{ cause: err.to_string() })?;
            let mut credentials = credentials.lines();
            let uri = config.rabbit_uri.replace("$USERNAME", credentials.next().unwrap_or_default());
            uri.replace("$PASSWORD", credentials.next().unwrap_or_default())
        },
        None => config.rabbit_uri.clone(),
    };

    let log_warn = |err, _dur| warn!("Failed to re-connect to RabbitMQ {}", err);
    let op = || {
        let conn = Connection::connect(&uri, ConnectionProperties::default()).wait()?;
        let channel = conn.create_channel().wait()?;

        info!("Connected to RabbitMQ");
        *RABBIT_CONNECTED.write() = true;

        // Create the exchange if it doesn't already exist.
        channel.exchange_declare(
            &config.rabbit_exchange,
            ExchangeKind::Topic,
            ExchangeDeclareOptions { durable: true, ..ExchangeDeclareOptions::default() },
            FieldTable::default()).wait()?;

        Ok((conn, channel))
    };

    retry_notify(backoff(timeout), op, log_warn).map_err(|err|err.into())
}

///
/// Check the connection. If it's not open - re-connect.
///
fn check_connection(rabbit_connection: &mut RabbitConnection, config: &Configuration) {
    if !rabbit_connection.channel.status().connected() {
        *RABBIT_CONNECTED.write() = false;

        match connect(&config, None) {
            Ok((connection, channel)) => {
                rabbit_connection.connection = connection;
                rabbit_connection.channel = channel;
            },
            Err(err) => error!("Failed to re-connect to RabbitMQ: {}", err),
        };
    }
}

///
/// Dedicated rabbit publishing thread.
///
pub fn rabbit_publisher(rx: Receiver::<Notification>, app_name: &str, config: Configuration) {
    let mut connection = match connect(&config, Some(Duration::from_secs(30))) {
        Ok((connection, channel)) => RabbitConnection { connection, channel },
        Err(err) => {
            error!("Stopping process. Unable to connect to RabbitMQ at start-up: {}", err);
            std::process::exit(1)
        }
    };

    let mut running = true;

    // Main thread loop - publish to the RabbitMQ exchange anything send to this thread.
    while running {
        // Wait for another thread to ask us to send a RabbitMQ message. Every second we'll check the state
        // of the RabbitMQ connection and repair it if it's closed.
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(notification) => {
                if let Some((bytes, props)) = to_rabbit_message(&notification, app_name) {
                    send(props, bytes, notification, &connection, &config);
                }
            },
            Err(Timeout) => check_connection(&mut connection, &config),
            Err(err) => {
                running = false;
                debug!("Expected error in RabbitMQ thread: {}", err);
                info!("Terminating RabbitMQ thread");
            }
        }
    }
}

///
/// Convert the Notification into the headers and payload for sending to RabbitMQ.
///
fn to_rabbit_message(notification: &Notification, app_name: &str) -> Option<(Vec<u8>, BasicProperties)> {
    match serde_json::to_vec(&notification.body) {
        Ok(bytes) => {
            let mut headers = FieldTable::default();
            headers.insert("version".to_string().into(), AMQPValue::ShortInt(notification.version as i16));
            headers.insert("messageType".to_string().into(), AMQPValue::LongString(notification.topic.to_string().into()));

            let props = BasicProperties::default()
                .with_app_id(app_name.to_string().into())
                .with_content_type("application/json".to_string().into())
                .with_correlation_id(notification.request_id.clone().into())
                .with_message_id(Uuid::new_v4().to_hyphenated().to_string().into())
                .with_headers(headers);

            Some((bytes, props))
        },
        Err(err) => {
            error!("Failed to serialise notification {:?} : {}", notification, err.to_string());
            None
        }
    }
}

///
/// Send the RabbitMQ message - any errors are logged but ignored.
///
#[tracing::instrument(name="send_rabbitmq", skip(props, bytes, notification, cc, config), level="info")]
fn send(props: BasicProperties, bytes: Vec<u8>, notification: Notification, cc: &RabbitConnection, config: &Configuration) {
    match cc.channel.basic_publish(
        &config.rabbit_exchange,
        notification.topic,
        BasicPublishOptions::default(),
        bytes,
        props.clone()).wait() {
            Ok(mut confirm) => {
                // Ensure the exchange confirms the send.
                match confirm.wait() {
                    Err(err) => error!("Failed to ack send for notification {:?}: {}", notification, err.to_string()),
                    _ => {
                        if tracer::tracer_on() {
                            let headers = format!("version: {}\nmessageType: {}",
                                notification.version,
                                notification.topic);

                            info!("Emitting message to {}\nApp-Id: {}\nContent-Type: {:?}\nCorrelation-Id: {:?}\nMessage-Id: {:?}\n{}\n{}",
                                notification.topic,
                                props.app_id().format(),
                                props.content_type().format(),
                                props.correlation_id().format(),
                                props.message_id().format(),
                                headers,
                                notification.body);
                        }
                    }
                }
            },
            Err(err) => error!("Failed to send notification {:?} : {}", notification, err.to_string())
    };
}


trait FormattableShortString {
    fn format(&self) -> &str;
}

impl <'a> FormattableShortString for &'a Option<ShortString> {
    fn format(&self) -> &'a str {
        match self {
            Some(short) => short.as_str(),
            None => "",
        }
    }
}
