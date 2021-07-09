use actix_http::http::StatusCode;
use actix_web::{HttpResponse, dev::HttpResponseBuilder};
use crate::utils::{context::RequestContext, errors::InternalError};

///
/// Allow support staff to view the current configuration of the system.
///
pub async fn handle(ctx: RequestContext) -> Result<HttpResponse, InternalError> {
    Ok(HttpResponseBuilder::new(StatusCode::OK).json(ctx.config()))
}