use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use super::external_id::ExternalId;
use prelude::*;

pub mod prelude {
    use serde::{Deserialize, Serialize};

    // Device fields.
    pub const DEVICE_ID: &str = "deviceId";
    pub const ENABLED: &str   = "enabled";

    #[derive(Debug, Deserialize, Serialize)]
    pub enum DeviceType {
        SMARTPHONE,
        PC,
        STB
    }
}

#[skip_serializing_none]
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewDevice {
    pub device_id: Option<String>,
    pub profile_id: Option<String>,
    pub device_type: DeviceType,
    pub enabled: Option<bool>,
    pub external_ids: Option<Vec<ExternalId>>,
}

#[skip_serializing_none]
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub device_id: String,
    pub profile_id: String,
    pub device_type: DeviceType,
    pub enabled: bool,
    pub external_ids: Option<Vec<ExternalId>>,
}