
use std::collections::HashMap;
use testcontainers::{Container, Docker, Image, WaitForMessage};

///
/// A docker container for spinning up MongoDB.
///

pub const INTERNAL_PORT: u16 = 27017;
const CONTAINER_IDENTIFIER: &str = "mongo";
const DEFAULT_TAG: &str = "4.0.17";

#[derive(Debug, Default, Clone)]
pub struct MongoArgs;

impl IntoIterator for MongoArgs {
    type Item = String;
    type IntoIter = ::std::vec::IntoIter<String>;

    fn into_iter(self) -> <Self as IntoIterator>::IntoIter {
        vec![].into_iter()
    }
}

#[derive(Debug)]
pub struct Mongo {
    tag: String,
    arguments: MongoArgs,
}

impl Default for Mongo {
    fn default() -> Self {
        Mongo {
            tag: DEFAULT_TAG.to_string(),
            arguments: MongoArgs {},
        }
    }
}

impl Image for Mongo {
    type Args = MongoArgs;
    type EnvVars = HashMap<String, String>;
    type Volumes = HashMap<String, String>;
    type EntryPoint = std::convert::Infallible;

    fn descriptor(&self) -> String {
        format!("{}:{}", CONTAINER_IDENTIFIER, &self.tag)
    }

    fn wait_until_ready<D: Docker>(&self, container: &Container<'_, D, Self>) {
        container
            .logs()
            .stdout
            .wait_for_message("waiting for connections on port")
            .unwrap();
    }

    fn args(&self) -> <Self as Image>::Args {
        self.arguments.clone()
    }

    fn env_vars(&self) -> Self::EnvVars {
        let mut vars = HashMap::new();
        vars.insert("MONGO_INITDB_ROOT_USERNAME".to_string(), "admin".to_string());
        vars.insert("MONGO_INITDB_ROOT_PASSWORD".to_string(), "changeme".to_string());
        vars
    }

    fn volumes(&self) -> Self::Volumes {
        HashMap::new()
    }

    fn with_args(self, arguments: <Self as Image>::Args) -> Self {
        Mongo { arguments, ..self }
    }
}