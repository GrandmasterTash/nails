use std::fmt::Write;
use std::env::VarError;
use config::ConfigError;
use serde::{Deserialize, Serialize};
use super::errors::{self, InternalError};
use crate::routes::admin::tracer::prelude::*;

///
/// The service configuration - initialised at start-up.
///
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Configuration {
    pub port: i32,                       // The port to run this service on.
    pub base_url: String,                // The root url to host endpoints on.
    pub db_name: String,                 // The MongoDB name to use.
    pub mongo_uri: String,               // The MongoDB connection URI. If a credentials file is used, $USERNAME, $PASSWORD should be used in the uri as placeholders.
    pub rabbit_uri: String,              // The RabbitMQ connection URI. If a credentials file is used, $USERNAME, $PASSWORD should be used in the uri as placeholders.
    pub auth_address: String,            // A (fake) remote service address - it's a wiremock example.
    pub keep_alive: Option<usize>,       // Allow client connections to be re-used. None disables.
    pub client_retry_delay: u64,         // Retry a failed HTTP request every n seconds.
    pub client_retry_limit: u8,          // How many times to retry a failed HTTP request.
    pub client_timeout: u64,             // Timeout (seconds) client http connections.
    pub server_timeout: u64,             // Timeout (seconds) downstream http connections to other services.
    pub jaeger_endpoint: Option<String>, // If jaeger tracing is enabled, this is the endpoint to send traces to.
    pub rabbit_exchange: String,         // The name of a RabbitMQ topic exchange to publish notications to.
    pub distributed_tracing: bool,       // Send traces to Jaeger.
    pub notification_queue_size: usize,  // An internal buffer size for messages being sent to RabbitMQ.
    pub redact_error_messages: bool,     // If true, any 400 responses to clients will only have a code and no descriptive message.
    pub mongo_credentials: Option<String>, // The path to the credentials file for MongoDB - None means use URI as-is.
    pub rabbit_credentials: Option<String>,// The path to the credentials file for RabbitMQ - None means use URI as-is.
}

impl Configuration {
    ///
    /// Load the service's configuration.
    ///
    pub fn from_env() -> Result<Configuration, ConfigError> {
        let mut cfg = config::Config::default();

        // Merge any environment variables with the same name as the struct fields.
        cfg.merge(config::Environment::new())?;

        // Set defaults for settings that were not specified.
        cfg.set_default("auth_address", "http://localhost:8111")?; // Wiremock in this example.
        cfg.set_default("base_url", "/")?;
        cfg.set_default("client_retry_delay", 5)?;
        cfg.set_default("client_retry_limit", 10)?;
        cfg.set_default("client_timeout", 30)?;
        cfg.set_default("db_name", "Accounts")?;
        cfg.set_default("distributed_tracing", false)?;
        cfg.set_default("jaeger_endpoint", None::<String>)?;
        cfg.set_default("keep_alive", Some(15))?;
        cfg.set_default("mongo_credentials", None::<String>)?;
        cfg.set_default("mongo_uri", "mongodb://admin:changeme@localhost:27017")?;
        cfg.set_default("notification_queue_size", 1000)?;
        cfg.set_default("port", 8989)?;
        cfg.set_default("rabbit_credentials", None::<String>)?;
        cfg.set_default("rabbit_exchange", "platform.events")?;
        cfg.set_default("rabbit_uri", "amqp://admin:changeme@localhost:5672")?;
        cfg.set_default("redact_error_messages", false)?;
        cfg.set_default("server_timeout", 20)?;

        let config: Configuration = cfg.try_into()?;
        *errors::REDACT_ERROR_MESSAGES.write() = config.redact_error_messages;

        if config.distributed_tracing && config.jaeger_endpoint.is_none() {
            panic!("Distributed tracing is enabled but no Jaeger endpoint is configured.");
        }

        Ok(config)
    }

    ///
    /// Pretty-print the config with ansi colours.
    ///
    pub fn fmt_console(&self) -> Result<String, InternalError> {
        // Serialise to JSON so we have fields to iterate.
        let values = serde_json::to_value(&self)?;

        // Turn into a hashmap.
        let values = values.as_object().expect("No config props");

        // Sort by keys.
        let mut sorted: Vec<_> = values.iter().collect();
        sorted.sort_by_key(|a| a.0);

        let mut output = String::new();
        for (k, v) in sorted {
            write!(&mut output, "{:>23}{} {}\n",
                k,
                *COLON,
                v)?;
        }

        Ok(output)
    }
}

///
/// If the specified environment variable is set for this process, set it to the default value specified.
///
pub fn default_env(key: &str, value: &str) {
    if let Err(VarError::NotPresent) = std::env::var(key) {
        std::env::set_var(key, value);
    }
}