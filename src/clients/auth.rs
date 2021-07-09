use serde_json::json;
use serde::Deserialize;
use crate::{utils::http::post, utils::{context::RequestContext, errors::InternalError}};

///
/// This is an example of how to make a downstream HTTP request via another service.
///

// Dummy example response body from an example request.
#[derive(Debug, Deserialize)]
pub struct ClaimResponse {
    claims: Vec<String>
}

///
/// Pass the session token to the remote auth service to check if the claim is assigned.
///
/// This is just an example downstream HTTP request.
///
pub async fn check_claim(claim: &str, ctx: &RequestContext) -> Result<ClaimResponse, InternalError> {

    let response = post(format!("{}/auth/get-claims", ctx.config().auth_address))
        .header("content-type", "application/json")
        .query_param("param1", "value1")
        .json(&json!({ "token": "eg session token from source request here" }))
        .send(ctx)
        .await?;

    match response.status() {
        200 => Ok(response.json()?),
        403 => Err(InternalError::InvalidClaim { claim: claim.to_string() }),
        any_other_status => Err(InternalError::RemoteRequestError { cause: format!("Bad response status {}", any_other_status), url: format!("{} {}", response.method(), response.url()) })
    }
}