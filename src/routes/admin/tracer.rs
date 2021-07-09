use tracing::info;
use parking_lot::RwLock;
use actix_web::Responder;
use lazy_static::lazy_static;
use actix_http::http::StatusCode;

pub enum Level {
    On,
    Off
}

lazy_static! {
    // In general configuration should be passed in a context struct via Actix .data extractors.
    // Any configuration in a lazy static block exists because there are sections are code where
    // The contexts cannot be accessed or where it becomes increasingly complex to do so.
    // For example, actix middleware, extractors, error responders, etc.

    /// The tracer is used aid us in times of crisis to write request and response bodies to the
    /// console for all incoming and outgoing request/responses as well as async notifications.
    pub static ref TRACER: RwLock<Level> = RwLock::new(Level::Off);
}

///
/// Uses a RwLock to ascsertain if tracer is on or off.
///
pub fn tracer_on() -> bool {
    match *TRACER.read() {
        Level::On => true,
        Level::Off => false,
    }
}

///
/// HTTP Handler to turn tracer on.
///
pub async fn handle_on() -> impl Responder {
    {
        let mut lock = TRACER.write();
        *lock = Level::On;
    }
    info!("Tracer is on");
    "on".with_status(StatusCode::OK)
}

///
/// HTTP Handler to turn tracer .... off.
///
pub async fn handle_off() -> impl Responder {
    {
        let mut lock = TRACER.write();
        *lock = Level::Off;
    }
    info!("Tracer is off");
    "on".with_status(StatusCode::OK)
}