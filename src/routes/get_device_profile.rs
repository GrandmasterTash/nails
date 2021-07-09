use mongodb::bson::doc;
use actix_web::{HttpResponse, dev::HttpResponseBuilder, http::StatusCode, web::Path};
use crate::{model::profile::{prelude::*, DeviceProfile}, utils::{context::RequestContext, errors::InternalError}};

///
/// Http handler for getting a device profile.
///
#[tracing::instrument(name="get_device_profile", level="info")]
pub async fn handle(Path(profile_id): Path<String>, ctx: RequestContext)
    -> Result<HttpResponse, InternalError> {

    let profile = get_device_profile(&profile_id, &ctx).await?;

    match profile {
        Some(profile) => Ok(HttpResponseBuilder::new(StatusCode::OK).json(profile)),

        // Note: 204 rather than 404 (the latter indicates the uri isn't present not the content itself)
        None => Ok(HttpResponseBuilder::new(StatusCode::NO_CONTENT).finish())
    }
}

///
/// Return the specified device profile.
///
pub async fn get_device_profile(profile_id: &str, ctx: &RequestContext) -> Result<Option<DeviceProfile>, InternalError> {
    let collection = ctx.db().collection_with_type(DEVICE_PROFILES);
    Ok(collection.find_one(doc! { "profileId": profile_id }, None).await?)
}