use crate::utils::mongo::{bson_date, optional_bson_date};
use chrono::{DateTime, Utc};
use mongodb::bson::Bson;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use super::{device::{Device, NewDevice}, external_id::ExternalId};
use prelude::*;

pub mod prelude {
    use serde::{Deserialize, Serialize};

    // Collection name
    pub const ACCOUNTS: &str = "Accounts";

    // Account fields.
    pub const ACCOUNT_ID: &str      = "accountId";
    pub const STATUS: &str          = "status";
    pub const CREATED: &str         = "created";
    pub const MODIFIED: &str        = "modified";
    pub const CREDENTIALS: &str     = "credentials";
    pub const DEVICES: &str         = "devices";

    // Account statuses.
    pub const STATUS_ACTIVE: &str = "ACTIVE";

    #[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
    pub enum AccountStatus {
        ACTIVE,
        RESTRICTED,
        SUSPENDED,
        CANCELLED // Terminal - status can't change from this value.
    }
}

///
/// The API schema for POSTing a new Account.
///
#[skip_serializing_none] // Use this to stop writing null fields to MongoDB.
#[serde(rename_all = "camelCase")]
#[derive(Debug, Deserialize, Serialize)]
pub struct NewAccount {
    pub account_id: Option<String>,
    pub status: Option<AccountStatus>,
    pub profile_id: Option<String>,
    pub salutation: Option<String>,
    pub billing_address: Option<Vec<AddressLine>>,
    pub external_ids: Option<Vec<ExternalId>>,
    pub devices: Option<Vec<NewDevice>>,
}

///
/// The API schema for updating the account status.
///
#[serde(rename_all = "camelCase")]
#[derive(Debug, Deserialize, Serialize)]
pub struct StatusModification {
    pub account_id: String,
    pub status: AccountStatus
}

///
/// This is the public schema for retrieving an Account.
///
/// This struct is intended to be de-serialised from MongoDB only - due to the interop
/// between chrono dates and bson we have to wire-in a custom date deserialiser.
///
#[skip_serializing_none]
#[serde(rename_all = "camelCase")]
#[derive(Debug, Deserialize, Serialize)]
pub struct Account {
    pub account_id: String,
    pub profile_id: String,
    pub status: AccountStatus,
    pub salutation: Option<String>,
    pub devices: Option<Vec<Device>>,
    pub external_ids: Option<Vec<ExternalId>>,
    pub billing_address: Option<Vec<AddressLine>>,

    #[serde(deserialize_with = "bson_date")]
    pub created: DateTime<Utc>,

    #[serde(default, deserialize_with = "optional_bson_date")]
    pub modified: Option<DateTime<Utc>>,
}

impl From<AccountStatus> for Bson {
    fn from(status: AccountStatus) -> Self {
        match status {
            AccountStatus::ACTIVE     => Bson::String("ACTIVE".to_string()),
            AccountStatus::RESTRICTED => Bson::String("RESTRICTED".to_string()),
            AccountStatus::SUSPENDED  => Bson::String("SUSPENDED".to_string()),
            AccountStatus::CANCELLED  => Bson::String("CANCELLED".to_string()),
        }
    }
}

#[serde(rename_all = "camelCase")]
#[derive(Debug, Deserialize, Serialize)]
pub struct AddressLine {
    pub key: String,
    pub value: String
}
