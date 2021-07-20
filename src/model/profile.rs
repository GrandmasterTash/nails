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

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountProfile {
    pub profile_id: Option<String>
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceProfile {
    profile_id: Option<String>
}