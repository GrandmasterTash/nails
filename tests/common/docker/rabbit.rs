
use std::collections::HashMap;
use testcontainers::{Container, Docker, Image, WaitForMessage};

///
/// A docker container for spinning up RabbitMQ.
///

pub const INTERNAL_PORT: u16 = 5672;
const CONTAINER_IDENTIFIER: &str = "bitnami/rabbitmq";
const DEFAULT_TAG: &str = "3.8.18";

#[derive(Debug, Default, Clone)]
pub struct RabbitArgs;

impl IntoIterator for RabbitArgs {
    type Item = String;
    type IntoIter = ::std::vec::IntoIter<String>;

    fn into_iter(self) -> <Self as IntoIterator>::IntoIter {
        vec![].into_iter()
    }
}

#[derive(Debug)]
pub struct Rabbit {
    tag: String,
    arguments: RabbitArgs,
}

impl Default for Rabbit {
    fn default() -> Self {
        Rabbit {
            tag: DEFAULT_TAG.to_string(),
            arguments: RabbitArgs {},
        }
    }
}

impl Image for Rabbit {
    type Args = RabbitArgs;
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
            .wait_for_message("Server startup complete")
            .unwrap();
    }

    fn args(&self) -> <Self as Image>::Args {
        self.arguments.clone()
    }

    fn env_vars(&self) -> Self::EnvVars {
        let mut vars = HashMap::new();
        vars.insert("RABBITMQ_USERNAME".to_string(), "admin".to_string());
        vars.insert("RABBITMQ_PASSWORD".to_string(), "changeme".to_string());
        vars
    }

    fn volumes(&self) -> Self::Volumes {
        HashMap::new()
    }

    fn with_args(self, arguments: <Self as Image>::Args) -> Self {
        Rabbit { arguments, ..self }
    }
}