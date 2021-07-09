use tracing::error;
use url::ParseError;
use serde_json::json;
use parking_lot::RwLock;
use lazy_static::lazy_static;
use crossbeam_channel::SendError;
use derive_more::{Display, Error};
use actix_http::{client::SendRequestError, error::PayloadError, http::header::{InvalidHeaderName, InvalidHeaderValue}};
use actix_web::{HttpResponse, ResponseError, client::JsonPayloadError, dev::HttpResponseBuilder, http::StatusCode, web::JsonConfig};
use mongodb::{bson::{self, document::ValueAccessError}, error::{ErrorKind, WriteFailure}};

lazy_static! {
    // In general configuration should be passed in a context struct via Actix .data extractors.
    // Any configuration in a lazy static block exists because there are sections are code where
    // The contexts cannot be accessed or where it becomes increasingly complex to do so.
    // For example, actix middleware, extractors, error responders, etc.

    /// When a 400 (bad request) is returned to the client, a useful error message will be sent.
    /// In production this exposes security concerns, so this setting is used to redact these messages.
    /// This value is a global as there are areas of the code which can't access the normal config struct.
    /// These tend to be extensions to the actix_web framework.
    pub static ref REDACT_ERROR_MESSAGES: RwLock<bool> = RwLock::new(false);
}

///
/// An error type used throughout the services code which can be converted into a HTTP error response.
///
/// All possible library or system errors are converted into one of these InternalErrors so our code
/// can have a clean Result<blah, InternalError> signature declaration and avoids excessive use or
/// operation.await.map_err(|err| blah) type call.
///
/// Conversion from a source error to an InternalError is done futher below with a series of From<T>
/// trait implementations.
///
#[derive(Clone, Debug, Display, Error)]
pub enum InternalError {
    #[display(fmt = "Unable to read credentials: {}", cause)]
    UnableToReadCredentials{ cause: String },

    #[display(fmt = "MongoDB error: {}", cause)]
    MongoDBError{ cause: String },

    #[display(fmt = "MongoDB schema needs to be v{} but it is v{} - try running with UPDATE_SCHEMA_ENABLED set", code_version, db_version)]
    MongoSchemaError{ code_version: i32, db_version: i32 },

    #[display(fmt = "MongoDB is locked for schema updating. Either another instance has locked it and is taking a long time or a previous update crashed and left the lock in place. {}", cause)]
    MongoLockedForUpdate{ cause: String },

    #[display(fmt = "The request had no fields to update")]
    MongoDBUpdateEmpty,

    #[display(fmt = "The request is not allowed and would cause a duplicate value: {}", cause)]
    MongoDuplicateError{ cause: String },

    #[display(fmt = "RabbitMQ error: {}", cause)]
    RabbitMQError{ cause: String },

    #[display(fmt = "Request format invalid: {}", reason)]
    RequestFormatError{ reason: String },

    #[display(fmt = "Failed to make downstream request: {}", cause)]
    SendRequestError{ cause: String },

    #[display(fmt = "{} claim invalid", claim)]
    InvalidClaim{ claim: String},

    #[display(fmt = "Url could not be parsed: {}", cause)]
    InvalidUrl{ cause: String },

    #[display(fmt = "Unable to convert to bson: {}", cause)]
    InvalidBsonError{ cause: String },

    #[display(fmt = "Unable to convert to json: {}", cause)]
    InvalidJsonError{ cause: String },

    #[display(fmt = "Unable to read bson: {}", cause)]
    BsonAccessError{ cause: String },

    #[display(fmt = "Request to {} failed with {}", url, cause)]
    RemoteRequestError{ cause: String, url: String },

    #[display(fmt = "Account {} not found", account_id)]
    AccountNotFound{ account_id: String },

    #[display(fmt = "Account profile {} not found", profile_id)]
    AccountProfileNotFound{ profile_id: String },

    #[display(fmt = "Device profile {} not found", profile_id)]
    DeviceProfileNotFound{ profile_id: String },

    #[display(fmt = "Account {} cannot be updated: it is cancelled", account_id)]
    AccountCancelled{ account_id: String },

    #[display(fmt = "Failed to internally notify: {}", cause)]
    SendNotificationError{ cause: String },

    #[display(fmt = "InvalidFormatError: {}", cause)]
    InvalidFormatError{ cause: String },
}

impl InternalError {
    fn error_code(&self) -> u16 {
        match *self {
            InternalError::InvalidFormatError{ cause: _ }                      => 0400,
            InternalError::UnableToReadCredentials{ cause: _ }                 => 0500,
            InternalError::InvalidClaim { claim: _ }                           => 1000,
            InternalError::RemoteRequestError { cause: _, url: _ }             => 1005,
            InternalError::RequestFormatError { reason: _ }                    => 1010,
            InternalError::RabbitMQError { cause: _ }                          => 1990,
            InternalError::MongoDBError { cause: _ }                           => 2001,
            InternalError::MongoSchemaError { code_version: _, db_version: _ } => 2002,
            InternalError::MongoLockedForUpdate { cause: _ }                   => 2003,
            InternalError::MongoDBUpdateEmpty                                  => 2004,
            InternalError::MongoDuplicateError { cause: _ }                    => 2005,
            InternalError::InvalidBsonError { cause: _ }                       => 2006,
            InternalError::InvalidJsonError { cause: _ }                       => 2105,
            InternalError::InvalidUrl { cause: _ }                             => 2150,
            InternalError::BsonAccessError { cause: _ }                        => 2207,
            InternalError::AccountNotFound { account_id: _ }                   => 2509,
            InternalError::AccountProfileNotFound { profile_id: _ }            => 2510,
            InternalError::DeviceProfileNotFound { profile_id: _ }             => 2511,
            InternalError::AccountCancelled { account_id: _ }                  => 2512,
            InternalError::SendNotificationError { cause: _ }                  => 2920,
            InternalError::SendRequestError { cause: _ }                       => 3000,
        }
    }

    ///
    /// Only 400 (bad request) responses can return an error message field.
    /// It is then controlled via the global redaction flag.
    ///
    fn redact_message(&self) -> bool {
        if self.status_code() != StatusCode::BAD_REQUEST {
            return true
        }
        *REDACT_ERROR_MESSAGES.read()
    }
}

impl ResponseError for InternalError {
    fn status_code(&self) -> StatusCode {
        match *self {
            InternalError::InvalidFormatError{ cause: _ }           => StatusCode::INTERNAL_SERVER_ERROR,
            InternalError::UnableToReadCredentials{ cause: _ }      => StatusCode::INTERNAL_SERVER_ERROR,
            InternalError::InvalidClaim { claim: _ }                => StatusCode::FORBIDDEN,
            InternalError::RemoteRequestError { cause: _, url: _ }  => StatusCode::INTERNAL_SERVER_ERROR,
            InternalError::RabbitMQError { cause: _ }               => StatusCode::INTERNAL_SERVER_ERROR,
            InternalError::MongoSchemaError { code_version: _, db_version: _ } => StatusCode::INTERNAL_SERVER_ERROR,
            InternalError::MongoLockedForUpdate { cause: _ }        => StatusCode::INTERNAL_SERVER_ERROR,
            InternalError::MongoDBError { cause: _ }                => StatusCode::INTERNAL_SERVER_ERROR,
            InternalError::MongoDBUpdateEmpty                       => StatusCode::BAD_REQUEST,
            InternalError::MongoDuplicateError { cause: _ }         => StatusCode::BAD_REQUEST,
            InternalError::RequestFormatError { reason: _ }         => StatusCode::BAD_REQUEST,
            InternalError::InvalidUrl { cause: _ }                  => StatusCode::INTERNAL_SERVER_ERROR,
            InternalError::InvalidJsonError { cause: _ }            => StatusCode::INTERNAL_SERVER_ERROR,
            InternalError::InvalidBsonError { cause: _ }            => StatusCode::INTERNAL_SERVER_ERROR,
            InternalError::BsonAccessError { cause: _ }             => StatusCode::INTERNAL_SERVER_ERROR,
            InternalError::AccountNotFound { account_id: _ }        => StatusCode::BAD_REQUEST,
            InternalError::AccountProfileNotFound { profile_id: _ } => StatusCode::BAD_REQUEST,
            InternalError::DeviceProfileNotFound { profile_id: _ }  => StatusCode::BAD_REQUEST,
            InternalError::AccountCancelled { account_id: _ }       => StatusCode::BAD_REQUEST,
            InternalError::SendNotificationError { cause: _ }       => StatusCode::INTERNAL_SERVER_ERROR,
            InternalError::SendRequestError { cause: _ }            => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse {
        error!("{}", self);// TODO: Can we capture the request_id?

        let body = match self.redact_message() {
            true => json!(
                {
                    "errorCode": self.error_code()
                }),
            false => json!(
                {
                    "errorCode": self.error_code(),
                    "message": self.to_string()
                }),
        };

        HttpResponseBuilder::new(self.status_code()).json(body)
    }
}

impl <T> From<SendError<T>> for InternalError {
    fn from(err: SendError<T>) -> Self {
        InternalError::SendNotificationError { cause: err.to_string() }
    }
}

impl From<mongodb::error::Error> for InternalError {
    fn from(error: mongodb::error::Error) -> Self {
        if let ErrorKind::WriteError(write_failure) = &*error.kind {
            if let WriteFailure::WriteError(write_error) = write_failure {
                if write_error.code == 11000 /* Duplicate key violation */ {
                    return InternalError::MongoDuplicateError { cause: error.to_string() }
                }
            }
        }

        InternalError::MongoDBError { cause: error.to_string() }
    }
}

impl From<bson::ser::Error> for InternalError {
    fn from(error: bson::ser::Error) -> Self {
        InternalError::InvalidBsonError { cause: error.to_string() }
    }
}

impl From<bson::de::Error> for InternalError {
    fn from(error: bson::de::Error) -> Self {
        InternalError::InvalidBsonError { cause: error.to_string() }
    }
}

impl From<serde_json::Error> for InternalError {
    fn from(error: serde_json::Error) -> Self {
        InternalError::InvalidJsonError { cause: error.to_string() }
    }
}

impl From<InternalError> for std::io::Error {
    fn from(error: InternalError) -> std::io::Error {
        std::io::Error::new(std::io::ErrorKind::Other, error.to_string() )
    }
}

impl From<ValueAccessError> for InternalError {
    fn from(error: ValueAccessError) -> Self {
        InternalError::BsonAccessError { cause: error.to_string() }
    }
}

impl From<lapin::Error> for InternalError {
    fn from(error: lapin::Error) -> Self {
        InternalError::RabbitMQError { cause: error.to_string() }
    }
}

impl <T: Into<InternalError>> From<backoff::Error<T>> for InternalError {
    fn from(error: backoff::Error<T>) -> Self {
        match error {
            backoff::Error::Permanent(err) => err.into(),
            backoff::Error::Transient(err) => err.into(),
        }
    }
}

impl From<ParseError> for InternalError {
    fn from(error: ParseError) -> Self {
        InternalError::InvalidUrl { cause: error.to_string() }
    }
}

impl From<PayloadError> for InternalError {
    fn from(error: PayloadError) -> Self {
        InternalError::InvalidJsonError { cause: error.to_string() }
    }
}

impl From<SendRequestError> for InternalError {
    fn from(error: SendRequestError) -> Self {
        InternalError::SendRequestError { cause: error.to_string() }
    }
}

impl From<JsonPayloadError> for InternalError {
    fn from(error: JsonPayloadError) -> Self {
        InternalError::InvalidJsonError { cause: error.to_string() }
    }
}

impl From<InvalidHeaderName> for InternalError {
    fn from(error: InvalidHeaderName) -> Self {
        InternalError::SendRequestError { cause: error.to_string() }
    }
}

impl From<InvalidHeaderValue> for InternalError {
    fn from(error: InvalidHeaderValue) -> Self {
        InternalError::SendRequestError { cause: error.to_string() }
    }
}

impl From<std::fmt::Error> for InternalError {
    fn from(_: std::fmt::Error) -> Self {
        todo!()
    }
}

///
/// Return JSON parse details as an error to the client.
///
pub fn configure_json_extractor() -> JsonConfig {
    JsonConfig::default()
        .error_handler(|err, _req| {
            InternalError::RequestFormatError { reason: err.to_string() }.into()
        })
}