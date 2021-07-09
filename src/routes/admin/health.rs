use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;
use actix_web::{HttpResponse, dev::HttpResponseBuilder, http::StatusCode};
use crate::utils::{context::{RequestContext}, errors::InternalError, http::get, mongo, rabbit};

#[derive(Serialize)]
struct Health {
    healthy: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>
}

pub async fn handle(ctx: RequestContext) -> Result<HttpResponse, InternalError> {
    let mut health = HashMap::<&str, Health>::new();
    health.insert("mongodb", mongo_health(&ctx).await);
    health.insert("rabbitmq", rabbit_health());
    health.insert("auth", ping_remote(format!("{}/auth/ping", ctx.config().auth_address), &ctx).await);

    let status = match health.values().any(|health| !health.healthy) {
        true  => StatusCode::SERVICE_UNAVAILABLE,
        false => StatusCode::OK,
    };

    Ok(HttpResponseBuilder::new(status).json(json!(
        {
            "MongoDB": health["mongodb"],
            "RabbitMQ": health["rabbitmq"],
            "Auth": health["auth"]
        }
    )))
}

async fn ping_remote(url: String, ctx: &RequestContext) -> Health {
    match get(url).dont_retry().send(ctx).await {
        Ok(response) => {
            match response.status() {
                   200 => Health { healthy: true, message: None },
                status => Health { healthy: false, message: Some(format!("Bad response status {}", status)) }
            }
        },
        Err(err) => Health { healthy: false, message: Some(err.to_string()) },
    }
}

async fn mongo_health(ctx: &RequestContext) -> Health {
    match mongo::ping(&ctx.db()).await {
        Err(err) => Health { healthy: false, message: Some(err.to_string()) },
        Ok(_) => Health { healthy: true, message: None }
    }
}

fn rabbit_health() -> Health {
    match *rabbit::RABBIT_CONNECTED.read() {
        true  => Health { healthy: true, message: None },
        false => Health { healthy: false, message: Some("Not connected".to_string()) }
    }
}