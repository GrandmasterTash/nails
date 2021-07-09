use serde::{Deserialize, Serialize};

///
/// Rather than add external system identifiers as ad-hoc fields (each with their own index), we can
/// instead store all/any external identifiers in an indexed map in MongoDB.
///
#[serde(rename_all = "camelCase")]
#[derive(Debug, Deserialize, Serialize)]
pub struct ExternalId {
    pub key: String,
    pub value: String
}