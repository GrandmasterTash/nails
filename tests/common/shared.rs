use tracing::debug;
use std::net::UdpSocket;
use std::sync::{Arc, Mutex};
use lazy_static::lazy_static;
use testcontainers::{Docker, core::Port};
use testcontainers::{Container, RunArgs, clients::Cli};
use crate::common::docker::{mongo::INTERNAL_PORT as MONGO_PORT, mongo::Mongo, rabbit::INTERNAL_PORT as RABBIT_PORT, rabbit::Rabbit};

lazy_static! {
    // Docker containers.
    pub static ref CONTAINERS: Arc<Mutex<DockerContainers>> = Arc::new(Mutex::new(DockerContainers::default()));

    // Used to start/stop docker containers.
    static ref DOCKER: Cli = Cli::default();
}

pub struct DockerContainers {
    ref_count: usize,
    mongo_container: Option<Container<'static, Cli, Mongo>>,
    rabbit_container: Option<Container<'static, Cli, Rabbit>>,
}

impl DockerContainers {
    pub fn start(&mut self) {
        self.ref_count += 1;

        if self.ref_count == 1 {
            println!("Starting docker containers, use TESTS_USE_EXISTING_CONTAINERS=true to avoid this step");
            self.start_mongodb();
            self.start_rabbitmq();

        } else {
            debug!("Skipping containers start - already started ({} tests now running)", self.ref_count);
        }
    }

    pub fn stop(&mut self) {
        self.ref_count -= 1;

        if self.ref_count == 0 {
            self.stop_rabbitmq();
            self.stop_mongodb();
        } else {
            debug!("Skipping containers stop, {} tests still running", self.ref_count);
        }
    }

    fn start_mongodb(&mut self) {
        println!("Starting MongoDB docker container...");
        let mongo_port = get_port();
        self.mongo_container = Some(DOCKER.run_with_args(
            Mongo::default(),
            RunArgs::default()
                .with_mapped_port(Port { local: mongo_port, internal: MONGO_PORT })));
        std::env::set_var("MONGO_URI", format!("mongodb://admin:changeme@localhost:{}/", mongo_port));
        println!("Started MongoDB on port {}", mongo_port);
    }

    fn stop_mongodb(&mut self) {
        println!("Stopping MongoDB docker container...");
        self.mongo_container.as_ref().unwrap().stop();
        self.mongo_container = None;
        println!("Stopped MongoDB");
    }

    fn start_rabbitmq(&mut self) {
        println!("Starting RabbitMQ docker container...");
        let rabbit_port = get_port();
        std::env::set_var("TEST_RABBIT_PORT", format!("{}", rabbit_port));
        self.rabbit_container = Some(DOCKER.run_with_args(Rabbit::default(),
            RunArgs::default().with_mapped_port(Port { local: rabbit_port, internal: RABBIT_PORT })));
        std::env::set_var("RABBIT_URI", format!("amqp://admin:changeme@localhost:{}/%2f", rabbit_port)); // Encoded vhost slash at end
        println!("Started RabbitMQ on port {}", rabbit_port);
    }

    fn stop_rabbitmq(&mut self) {
        println!("Stopping RabbitMQ docker container...");
        self.rabbit_container.as_ref().unwrap().stop();
        self.rabbit_container = None;
        println!("Stopped RabbitMQ");
    }
}

impl Default for DockerContainers {
    fn default() -> Self {
        DockerContainers {
            ref_count: 0,
            mongo_container: None,
            rabbit_container: None,
        }
    }
}

///
/// Get a random port available port from the OS.
///
fn get_port() -> u16 {
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    match socket.local_addr() {
        Ok(address) => address.port(),
        Err(err) => {
            eprintln!("Unable to obtain a port from the OS for a test container: {}", err.to_string());
            0
        }
    }
}

pub fn get_rabbitmq_port() -> u16 {
    match std::env::var("TEST_RABBIT_PORT") {
        Ok(port) =>  port.parse::<u16>().expect("Couldn't parse TEST_RABBIT_PORT"),
        Err(_) => RABBIT_PORT,
    }
}