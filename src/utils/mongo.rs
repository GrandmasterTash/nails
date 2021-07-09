use uuid::Uuid;
use tracing::{debug, info};
use chrono::{DateTime, Utc};
use std::{collections::HashMap, fs};
use serde::{Deserialize, Deserializer, Serialize};
use crate::utils::{config::Configuration, errors::InternalError};
use mongodb::{Client, Collection, Database, bson::{self, Document, doc}, options::ClientOptions};

///
/// Run any schema-like updates against MongoDB that haven't been run yet.
///
pub async fn update_mongo(db: &Database) -> Result<(), InternalError> {
    create_init_indexes(db).await?;
    create_default_profiles(db).await?;
    Ok(())
}

async fn create_init_indexes(db: &Database) -> Result<(), InternalError> {
    // Note: the current driver doesn't yet support creating indexes on collections, so the dbcommand
    // must be used instead.
    // https://docs.mongodb.com/manual/reference/command/createIndexes/#createindexes

    // Note: I've split multiple calls to the same collection to ease readability.
    db.run_command(doc! { "createIndexes": "Accounts", "indexes": [{ "key": { "accountId": 1 }, "name": "idx_accountId", "unique": true }] }, None).await?;
    db.run_command(doc! { "createIndexes": "Accounts", "indexes": [{ "key": { "devices.deviceId": 1 }, "name": "idx_deviceId", "unique": true, "sparse": true } ] }, None).await?;
    db.run_command(doc! { "createIndexes": "Accounts", "indexes": [{ "key": { "externalIds.key": 1, "externalIds.value": 1 }, "name": "idx_accountExternalId", "unique": true, "sparse": true }] }, None).await?;
    db.run_command(doc! { "createIndexes": "Accounts", "indexes": [{ "key": { "devices.externalIds.key": 1, "devices.externalIds.value": 1 }, "name": "idx_deviceExternalId", "unique": true, "sparse": true }] }, None).await?;
    db.run_command(doc! { "createIndexes": "AccountProfiles", "indexes": [{ "key": { "profileId": 1 }, "name": "idx_profileId", "unique": true }] }, None).await?;
    db.run_command(doc! { "createIndexes": "DeviceProfiles", "indexes": [{ "key": { "profileId": 1 }, "name": "idx_profileId", "unique": true }] }, None).await?;

    Ok(())
}

async fn create_default_profiles(db: &Database) -> Result<(), InternalError> {
    let col: Collection = db.collection("AccountProfiles");
    match col.insert_one(doc!{ "profileId": "DEFAULT" }, None).await {
        _ => () // Insert failures are fine if the profile already exists.
    };

    let col: Collection = db.collection("DeviceProfiles");
    match col.insert_one(doc!{ "profileId": "DEFAULT" }, None).await {
        _ => () // Insert failures are fine if the profile already exists.
    };
    Ok(())
}

pub async fn get_mongo_db(app_name: &str, config: &Configuration) -> Result<Database, InternalError> {

    let uri = match &config.mongo_credentials {
        Some(filename) => {
            debug!("Loading MongoDB credentials from secrets file {}", filename);

            // Read username and password from a secrets file.
            let credentials = fs::read_to_string(filename).map_err(|err| InternalError::UnableToReadCredentials{ cause: err.to_string() })?;
            let mut credentials = credentials.lines();
            let uri = config.mongo_uri.replace("$USERNAME", credentials.next().unwrap_or_default());
            uri.replace("$PASSWORD", credentials.next().unwrap_or_default())
        },
        None => config.mongo_uri.clone(),
    };

    // Parse the uri now.
    let mut client_options = ClientOptions::parse(&uri).await?;

    // Manually set an option.
    client_options.app_name = Some(app_name.to_string());

    // Get a handle to the deployment.
    let client = Client::with_options(client_options)?;

    info!("Connecting to MongoDB...");

    let db = client.database(&config.db_name);
    ping(&db).await?;

    info!("Connected to MongoDB");
    Ok(db)
}

pub async fn ping(db: &Database) -> Result<Document, InternalError> {
    Ok(db.run_command(doc! { "ping": 1 }, None).await?)
}

///
/// Insert the ID field into the MongoDB document with the value specified, or generate a new id if needed.
///
pub fn generate_id(field: &str, doc: &mut Document, existing_id: &Option<String>) -> String {
    match existing_id {
        None => {
            let new_id = Uuid::new_v4().to_hyphenated().to_string();
            doc.insert(field, new_id.clone());
            new_id
        },
        Some (id) => id.clone()
    }
}

pub trait Persistable<T: Serialize> {
    ///
    /// Convert into a MongoDB BSON document.
    ///
    fn to_doc(&self) -> Result<Document, InternalError>;
}

impl<T: Serialize> Persistable<T> for T {
    fn to_doc(&self) -> Result<Document, InternalError> {
        let bson = bson::to_bson(self)
            .map_err(|err| InternalError::InvalidBsonError{ cause: err.to_string() })?;

        match bson.as_document() {
            Some(doc) => Ok(doc.to_owned()),
            None => Err(InternalError::InvalidBsonError{ cause: "Result is empty Document".to_string() })
        }
    }
}

///
/// The current MongoDB driver doesn't (nicely) support Chrono::DateTimes so we must use
/// this fn to deserialize a BSON format Date into a DataTime<Utc> struct value.
///
/// Note - do not use this with structs which deserialise from JSON requests!
///
pub fn bson_date<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
    where D: Deserializer<'de>
{
    match mongodb::bson::Bson::deserialize(deserializer) {
        Ok(mongodb::bson::Bson::DateTime(datetime)) => Ok(datetime),
        _ => Err(serde::de::Error::custom("Failed to deserialise an datetime from bson"))
    }
}

pub fn optional_bson_date<'de, D>(deserializer: D) -> Result<Option<DateTime<Utc>>, D::Error>
    where D: Deserializer<'de>
{
    // Example of source - {"$date": "2021-06-24T05:00:20.024Z"}
    let map: Option<HashMap<String, String>> = Option::deserialize(deserializer)?;
    if let Some(map) = map {
        match map.get("$date") {
            None => return Err(serde::de::Error::custom(format!("Bson date was not formatted as expected {:?}", map))),
            Some(datetime) => return Ok(Some(datetime.parse::<DateTime<Utc>>().map_err(serde::de::Error::custom)?))
        }
    }

    Ok(None)
}
