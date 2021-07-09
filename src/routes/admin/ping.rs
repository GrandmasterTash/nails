use actix_web::Responder;

pub async fn handle() -> impl Responder {
    "pong"
}