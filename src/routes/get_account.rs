use mongodb::bson::doc;
use actix_web::{HttpResponse, dev::HttpResponseBuilder, http::StatusCode, web::Path};
use crate::{model::account::{prelude::*, Account}, utils::{context::RequestContext, errors::InternalError}};

///
/// Http handler for getting an account.
///
#[tracing::instrument(name="get_account", level="info")]
pub async fn handle(Path(account_id): Path<String>, ctx: RequestContext)
    -> Result<HttpResponse, InternalError> {

    let account = get_account(&account_id, &ctx).await?;

    match account {
        Some(account) => Ok(HttpResponseBuilder::new(StatusCode::OK).json(account)),

        // Note: 204 rather than 404 (the latter indicates the uri isn'y present not the content itself)
        None => Ok(HttpResponseBuilder::new(StatusCode::NO_CONTENT).finish())
    }
}

///
/// Return the specified account.
///
pub async fn get_account(account_id: &str, ctx: &RequestContext)
    -> Result<Option<Account>, InternalError> {

    let collection = ctx.db().collection_with_type(ACCOUNTS);

    Ok(collection.find_one(doc! { "accountId": account_id }, None).await?)
}