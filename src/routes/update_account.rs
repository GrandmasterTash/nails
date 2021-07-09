use serde_json::json;
use mongodb::bson::{Document, doc};
use actix_web::{HttpResponse, dev::HttpResponseBuilder, http::StatusCode, web::Json};
use crate::{model::account::{prelude::*, Account, StatusModification}, routes::get_account::get_account, utils::{context::RequestContext, errors::InternalError, rabbit::{notify, prelude::*}}};

///
/// Http handler for updating an account's status.
///
#[tracing::instrument(name="update_account_status", level="info")]
pub async fn handle_status(update: Json<StatusModification>, ctx: RequestContext)
    -> Result<HttpResponse, InternalError> {

    update_account_status(update.into_inner(), &ctx).await?;

    Ok(HttpResponseBuilder::new(StatusCode::OK).finish())
}

///
/// Update the account's status. An error is returned if the update cannot proceed.
///
pub async fn update_account_status(update: StatusModification, ctx: &RequestContext)
    -> Result<(), InternalError> {

    // Find the account.
    let account = match get_account(&update.account_id, ctx).await? {
        Some(account) => account,
        None => return Err(InternalError::AccountNotFound{ account_id: update.account_id })
    };

    // Validate and populate defaults.
    let doc = validate_status_update(&update, &account, ctx).await?;

    // Update the account in MongoDB now.
    let result = ctx.db().collection(ACCOUNTS).update_one(
        /* Filter  */ doc!{ ACCOUNT_ID: &account.account_id },
        /* Update  */ doc,
        /* Options */ None)
        .await?;

    // Emit a notification to RabbitMQ (or whatever event system is configured).
    if result.modified_count > 0 {
        notify(TOPIC_ACCOUNT_STATUS_UPDATED)
            .body(json!({
                "accountId": &account.account_id,
                "oldStatus": account.status,
                "newStatus": update.status
            }))
            .send(&ctx);
    }

    Ok(())
}

///
/// Validate the request and populate additional details - returning a MongoDB Document to insert if all is good.
///
async fn validate_status_update(update: &StatusModification, account: &Account, ctx: &RequestContext)
    -> Result<Document, InternalError> {

    if account.status == AccountStatus::CANCELLED {
        return Err(InternalError::AccountCancelled {account_id: account.account_id.clone() })
    }

    Ok(doc! { "$set": { STATUS: update.status, MODIFIED: ctx.now() } })
}
