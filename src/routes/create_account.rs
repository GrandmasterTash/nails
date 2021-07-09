use serde_json::json;
use mongodb::bson::{self, Document};
use actix_web::{HttpResponse, dev::HttpResponseBuilder, http::StatusCode, web::Json};
use super::{get_account_profile::get_account_profile, get_device_profile::get_device_profile};
use crate::{clients::auth, model::{account::{prelude::*, Account, NewAccount}, device::{prelude::*, NewDevice}, profile::prelude::*}, utils::{context::RequestContext, errors::InternalError, mongo::{Persistable, generate_id}, rabbit::{notify, prelude::*}}};

///
/// Http handler for creating an account.
///
#[tracing::instrument(name="create_account", skip(account), level="info")]
pub async fn handle(account: Json<NewAccount>, ctx: RequestContext) -> Result<HttpResponse, InternalError> {

    // Do not allow unless the caller has the create-account permission.
    let _response = auth::check_claim("create-account", &ctx).await?;

    // Call the 'business' tier method to do the work.
    let account = create_account(account.into_inner(), &ctx).await?;

    // Create HTTP response for the call.
    Ok(HttpResponseBuilder::new(StatusCode::CREATED).json(account))
}

///
/// Validate and create the account specified.
///
pub async fn create_account(new_account: NewAccount, ctx: &RequestContext) -> Result<Account, InternalError> {

    // Validate and populate defaults.
    let mut doc = validate_account(&new_account, ctx).await?;

    // Insert into MongoDB.
    ctx.db().collection(ACCOUNTS).insert_one(doc.clone(), None).await?;

    // Strip any credentials from the account before we return or notify the account details.
    // (I never actually got as far as adding any in the first place!).
    doc.remove(CREDENTIALS);

    // Convert the doc into an Account struct and return it to the caller. This avoids a round trip for the
    // caller to get the full account details with all generated values, AND avoids a write-read on the
    // database. So, assuming profiles are cached, a create account (and devices) results in a single write.
    // let account
    let account = bson::from_bson(doc.into())?;

    // Emit a notification to RabbitMQ (or whatever event system is configured).
    notify(TOPIC_ACCOUNT_CREATED).body(json!(account)).send(&ctx);

    Ok(account)
}

///
/// Validate the request and populate additional details - returning a MongoDB Document to insert if all is good.
///
async fn validate_account(account: &NewAccount, ctx: &RequestContext) -> Result<Document, InternalError> {

    // If specified, validate that the account profile exists.
    if let Some(profile_id) = &account.profile_id {
        if let None = get_account_profile(&profile_id, ctx).await? {
            return Err(InternalError::AccountProfileNotFound { profile_id: profile_id.clone() })
        }
    }

    // Turn our NewAccount structure into a Bson document. We're going to add defaults which
    // may not have been specified.
    let mut doc = account.to_doc()?;

    // Use a default profile if one isn't specified.
    if let None = account.profile_id {
        doc.insert(PROFILE_ID, DEFAULT);
    }

    // Set the CREATED field.
    doc.insert(CREATED, ctx.now());

    // Generate an accountId if one isn't specified.
    generate_id(ACCOUNT_ID, &mut doc, &account.account_id);

    // Default the account to active if no status was specified.
    if let None = account.status {
        doc.insert(STATUS, STATUS_ACTIVE);
    }

    // Validate any devices specified in the request.
    if let Some(devices) = &account.devices {
        for (idx, device) in devices.iter().enumerate() {
            let device_doc = get_sub_doc(DEVICES, idx, &mut doc)?;
            validate_device(device, device_doc, &ctx).await?;
        }
    }

    Ok(doc)
}

///
/// Validate the specified device and populate additional details.
///
async fn validate_device(device: &NewDevice, doc: &mut Document, ctx: &RequestContext) -> Result<(), InternalError> {

    // If specified, validate that the device profile exists.
    if let Some(profile_id) = &device.profile_id {
        if let None = get_device_profile(&profile_id, ctx).await? {
            return Err(InternalError::DeviceProfileNotFound { profile_id: profile_id.clone() })
        }
    }

    // Set the CREATED field.
    doc.insert(CREATED, ctx.now());

    // Use a default profile if one isn't specified.
    if let None = device.profile_id {
        doc.insert(PROFILE_ID, DEFAULT);
    }

    // Generate an internal deviceId if required.
    generate_id(DEVICE_ID, doc, &device.device_id);

    // Enable the device if not specified.
    if let None = device.enabled {
        doc.insert(ENABLED, true);
    }

    Ok(())
}

///
/// Return the Bson array element specified from the parent Document as a mutable child Document.
///
/// For example, given the following structure 'parent': -
///
/// {
///     "child": [
///         { "field": "value 1" },
///         { "field": "value 2" },
///     ]
/// }
///
/// Then get_sub_doc("child", 1, parent) will return the child with 'value 2' as it's own, 
/// mutable document.
///
fn get_sub_doc<'a>(key: &str, index: usize, parent: &'a mut Document) -> Result<&'a mut Document, InternalError> {
    let dev_doc = parent.get_array_mut(key)?;
    Ok(dev_doc[index].as_document_mut().ok_or(InternalError::BsonAccessError{cause: format!("{} not found in bson at {}", key, index)})?)
}

