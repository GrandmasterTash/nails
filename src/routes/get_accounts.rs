use mongodb::bson::doc;
use futures::TryStreamExt;
use actix_web::{HttpResponse, dev::HttpResponseBuilder, http::StatusCode};
use crate::{model::account::{prelude::*, Account}, utils::{context::RequestContext, errors::InternalError}};

///
/// Http handler for getting multiple accounts.
///
#[tracing::instrument(name="get_accounts", skip(ctx), level="info")]
pub async fn handle(ctx: RequestContext) -> Result<HttpResponse, InternalError> {

    Ok(HttpResponseBuilder::new(StatusCode::OK)
        .json(get_accounts(&ctx).await?))
}

pub async fn get_accounts(ctx: &RequestContext) -> Result<Vec<Account>, InternalError> {

    let collection = ctx.db().collection_with_type::<Account>(ACCOUNTS);
    let cursor = collection.find(doc!{}, None).await?; // Yes this would return ALL accounts.
    Ok(cursor.try_collect().await?)                    // In a real system we'd paginate and limit.
}