use tracing::info;
use ansi_term::Colour;
use serde::Deserialize;
use parking_lot::RwLock;
use lazy_static::lazy_static;
use actix_web::{Responder, web::Query};
use actix_http::http::{HeaderMap, StatusCode};

pub mod prelude {
    use ansi_term::Colour;
    use super::USE_COLOUR;
    use lazy_static::lazy_static;

    const GREY: Colour = Colour::RGB(110, 110, 110);

    // Some logging format symbols, potentially with colour.
    lazy_static! {
        pub static ref COLON: String = match *USE_COLOUR {
            true  => Colour::Yellow.paint(":").to_string(),
            false => String::from(":"),
        };

        pub static ref IN: String = match *USE_COLOUR {
            true  => GREY.paint("> ").to_string(),
            false => String::from("> "),
        };

        pub static ref OUT: String = match *USE_COLOUR {
            true  => GREY.paint("< ").to_string(),
            false => String::from("< "),
        };

        pub static ref IN_2: String = match *USE_COLOUR {
            true  => GREY.paint(">> ").to_string(),
            false => String::from(">> "),
        };

        pub static ref OUT_2: String = match *USE_COLOUR {
            true  => GREY.paint("<< ").to_string(),
            false => String::from("<< "),
        };
    }
}

///
/// Return the status with ansi-colouring.
///
pub fn colour_status(status: u16) -> String {
    if *USE_COLOUR {
        return match status {
            status @ 200..=299 => format!("{}", Colour::RGB(107, 142, 35).paint(format!("{}", status))),
            status @ 300..=399 => format!("{}", Colour::RGB(255, 140,  0).paint(format!("{}", status))),
            status @         _ => format!("{}", Colour::RGB(205,  92, 92).paint(format!("{}", status))),
        }
    }

    format!("{}", status)
}

enum Level {
    On,
    Off,
    Bullet { matcher: Option<(String, String)> },
}

lazy_static! {
    // In general configuration should be passed in a context struct via Actix .data extractors.
    // Any configuration in a lazy static block exists because there are sections are code where
    // The contexts cannot be accessed or where it becomes increasingly complex to do so.
    // For example, actix middleware, extractors, error responders, etc.

    /// The tracer is used aid us in times of crisis to write request and response bodies to the
    /// console for all incoming and outgoing request/responses as well as async notifications.
    static ref TRACER: RwLock<Level> = RwLock::new(Level::Off);

    /// For those terms that don't support ansi colour set USE_COLOUR to false.
    pub static ref USE_COLOUR: bool = std::env::var("USE_COLOUR")
        .unwrap_or_default()
        .to_lowercase() == "true";
}

///
/// Uses a RwLock to ascsertain if tracer is on or off.
///
pub fn tracer_on(headers: &HeaderMap) -> bool {
    let lock = TRACER.read();
    match &*lock {
        Level::On => true,
        Level::Off => false,
        Level::Bullet { matcher } => {
            // If the tracer has a key/value which match one of the headers specified, then tracer is
            // on (for this request).
            if let Some((match_key, match_value)) = matcher {
                let match_value = match_value.to_lowercase();

                if let Some(header_value) = headers.get(match_key) {
                    if let Ok(header_value) = header_value.to_str() {
                        return header_value.to_lowercase() == match_value
                    }
                }
            }

            return false
        }
    }
}

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
    "off".with_status(StatusCode::OK)
}

#[derive(Deserialize)]
pub struct Params {
    header: String,
    value: String
}

///
/// HTTP Handler to turn tracer on.
///
pub async fn handle_bullet(params: Query<Params>) -> impl Responder {
    {
        let mut lock = TRACER.write();
        *lock = Level::Bullet { matcher: Some((params.header.clone(), params.value.clone())) };
    }
    info!("Tracer buller is on where {}={}", params.header, params.value);
    "bullet".with_status(StatusCode::OK)
}