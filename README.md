
<img align="left" src="nails.png">

# Nails
Nails is a sample Rust HTTP microservice which demonstrates how to integrate Actix_Web with MongoDB and RabbitMQ. It adds some resilience and observability features to provide a more production-ready stack to work in. The clients and routes modules contain an example small vertical slice of a full production stack.

## Features

- HTTP request which interact with **MongoDB** and can produce **RabbitMQ** notifications.
- **HTTP request retries** - HTTP statuses of 50x and IO errors will, by default, be re-attempted.
- **Resilient RabbitMQ connection** - if the connection to RabbitMQ goes down, Nails will attempt reconnections.
- **Integration tests** using docker containers - dependencies are ready to go in docker.
- **Request ID propagation** - the X-Correlation-ID header is propagated to downstream requests and async notifications all share originating id.
- **Distributed tracing** - Jaeger integration for monitoring of requests and instrumented methods.
- Request/Response payload logging with a **tracer bullet** - can be dynamically enabled/disabled without downtime to look at HTTP traffic payloads in detail.
- **Secret credentials** - MongoDB and RabbitMQ credentials can passed via env vars or via credential files.
- **Health-check** to monitor MongoDB, RabbitMQ and other downstream services.
- **Time travel** - tests can set a fixed time in the service to assert exact time values with predicable results.

## Running Nails

To run Nails, first start the docker containers for RabbitMQ, MongoDB, Wiremock and Jaeger.

`docker-compose up`

Next compile and run the Rust app with cargo.

`cargo run`

You can use the RESTClient (a VSCode extension) to open the *utils/test.http* file and execute requests against the service.

## Testing Nails

`cargo test`

or (if you want a summary rather than to see every test name)

`cargo test -q`

or (if you want to run a subset or specific test)

`cargo test <test_name_prefix>`

or (if you want console output)

`cargo test -- --nocapture`

Or you can use the rust-analyzer VSCode extension to click on the test to run.

## Running Code Coverage

Install tarpaulin into cargo. Then run

`cargo tarpaulin --out Lcov`

Use the VSCode extension 'Coverage Gutters: Watch' to scan the Lcov file and display line coverage.

## Configuring Nails

Nail is configured via environment variables. Credentials can be provided via a secrets file - see *config.rs* for details of all configuration options and some extra details in the *.env* file itself.

Developers can use the .env file to override default configuration.

## Building Docker Image

To compile and build entirely inside a docker container do this: -

`docker build -t nails .`

## Adding a new Endpoint

Create an actix HTTP handler in the routes package. Look at the sample endpoints and ensure you use the
RequestContext extractor - it gives access to RabbitMQ, MongoDB and other features.

Include the route at the top of lib.rs in configure_routes().

Add any new errors that can be raised in to *utils/errors.rs*.

Profit.

## Known Issues
- Currently there's no CatchUnwind support in run_test - so if a test fails/panics the containers are left running. For this reason recommend you run tests with TESTS_USE_EXISTING_CONTAINERS=true.
- Error responses are not constructed inside a tracing span so logged errors lose the span's context. Future work in the actix-otel project looks like a feature flag sync-middleware may make this feasible.