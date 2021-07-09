use mongodb::bson::doc;
use actix_web::{HttpResponse, dev::HttpResponseBuilder, http::StatusCode, web::Path};
use crate::{model::profile::{prelude::*, AccountProfile}, utils::{context::RequestContext, errors::InternalError}};

///
/// Http handler for getting an account profile.
///
#[tracing::instrument(name="get_account_profile", level="info")]
pub async fn handle(Path(profile_id): Path<String>, ctx: RequestContext)
    -> Result<HttpResponse, InternalError> {

    let profile = get_account_profile(&profile_id, &ctx).await?;

    match profile {
        Some(profile) => Ok(HttpResponseBuilder::new(StatusCode::OK).json(profile)),
        None => Ok(HttpResponseBuilder::new(StatusCode::NO_CONTENT).finish())
    }
}

///
/// Return the specified account profile.
///
pub async fn get_account_profile(profile_id: &str, ctx: &RequestContext) -> Result<Option<AccountProfile>, InternalError> {
    let collection = ctx.db().collection_with_type(ACCOUNT_PROFILES);
    Ok(collection.find_one(doc! { "profileId": profile_id }, None).await?)
}