use serde::{Deserialize, Serialize};

pub mod prelude {
    // Collection names
    pub const DEVICE_PROFILES: &str = "DeviceProfiles";
    pub const ACCOUNT_PROFILES: &str = "AccountProfiles";

    // Field names.
    pub const PROFILE_ID: &str = "profileId";

    // Default profile ids.
    pub const DEFAULT: &str = "DEFAULT";
}

#[serde(rename_all = "camelCase")]
#[derive(Debug, Deserialize, Serialize)]
pub struct AccountProfile {
    profile_id: Option<String>
}

#[serde(rename_all = "camelCase")]
#[derive(Debug, Deserialize, Serialize)]
pub struct DeviceProfile {
    profile_id: Option<String>
}