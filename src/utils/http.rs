use url::Url;
use futures::Stream;
use serde_json::Value;
use itertools::Itertools;
use tracing::{info, warn};
use std::collections::HashMap;
use serde::de::DeserializeOwned;
use actix_web_opentelemetry::ClientExt;
use std::{pin::Pin, str::FromStr, time::Duration};
use crate::{APP_NAME, middleware::request_id::REQUEST_ID_HEADER, routes::admin::tracer};
use super::{config::Configuration, context::RequestContext, errors::InternalError};
use actix_web::{client::{Client, ClientRequest, ClientResponse}, dev::Decompress, web::Bytes};
use actix_http::{Payload, client::Connector, error::PayloadError, http::{Method, HeaderName, HeaderValue, header}};

///
/// Construct a configured HTTP client.
///
pub fn http_client(config:&Configuration) -> Client {
    Client::builder()
        .header(header::USER_AGENT, APP_NAME)
        .timeout(Duration::from_secs(config.server_timeout))
        .connector(Connector::new()
            .timeout(Duration::from_secs(config.server_timeout))
            .finish())
        .finish()
}

///
/// Alias onto the Actix Futures response.
///
type ActixHttpResponse = ClientResponse<Decompress<Payload<Pin<Box<dyn Stream<Item = Result<Bytes, PayloadError>>>>>>>;

///
/// Our own library-agnostic wrapper around a HTTP request.
///
/// We can tweak it's api to suit our usage and make our code more readable than if we relied
/// on other libraries directly.
///
/// Example: -
///    let response = post(format!("{}/remote/endpoint/{}", address, id)
///        .query_param("name", "value")
///        .header("name1", "value1")
///        .header("name2", "value2")
///        .json_body(&<json>)
///        .send(ctx)
///        .await?;
///
///    let thing: Thing = match response.status() {
///        200 => response.json()?
///        400 => return Err(yyyy)
///          _ => return Err(xxxx)
///    }
///
pub struct HttpRequest {
    url: String,
    method: Method,
    body: Option<Vec<u8>>,
    headers: HashMap<String, String>,
    query_params: HashMap<String, String>,
    dont_retry: bool,
    body_error: Option<InternalError> // Send when the body is set externally but fails to serialise. This means we can handle errors on send() not body().
}

impl HttpRequest {
    fn new(method: Method, url: String) -> Self {
        HttpRequest {
            url,
            body: None,
            method,
            headers: HashMap::new(),
            query_params: HashMap::new(),
            dont_retry: false,
            body_error: None
        }
    }

    pub fn header(&mut self, name: &str, value: &str) -> &mut Self {
        self.headers.insert(name.to_string(), value.to_string());
        self
    }

    pub fn query_param(&mut self, name: &str, value: &str) -> &mut Self {
        self.query_params.insert(name.to_string(), value.to_string());
        self
    }

    ///
    /// Set a JSON body to be sent.
    ///
    pub fn json(&mut self, body: &Value) -> &mut Self {
        match serde_json::to_vec(&body) {
            Ok(bytes) => self.body = Some(bytes),
            Err(err) => self.body_error = Some(InternalError::InvalidJsonError { cause: err.to_string() })
        };
        self
    }

    ///
    /// By default, errors or 500 status responses will be retried. Use this to supress that behaviour.
    ///
    pub fn dont_retry(&mut self) -> &mut Self {
        self.dont_retry = true;
        self
    }

    ///
    /// Send the HTTP request - and return a response.
    ///
    /// Some home-grown retry logic is used if error's or 500 HTTP status are returned.
    /// Home-grown because all the published crates rely on Tokio 1+ so we're limited.
    /// The entire method (nearly) is in the retry loop because the AWC request is consumed by
    /// send - so it's reconstructed on each re-attempt.
    ///
    pub async fn send(&mut self, ctx: &RequestContext) -> Result<HttpResponse, InternalError> {
        // If we failed to serailise the body, fail at this point.
        if let Some(body_error) = &self.body_error {
            return Err(body_error.to_owned())
        }

        // Parse the url and query params and urlencode.
        let mut url = Url::parse(&self.url)?;

        for query_param in &self.query_params {
            url.query_pairs_mut().append_pair(&query_param.0, &query_param.1);
        }

        let mut attempts: u8 = 1;
        let mut resp = loop {
            // Build an actix web client request.
            let mut req = ctx.client().request(self.method.clone(), url.as_str());

            // Append all the specified header.
            for header in &self.headers {
                append_header(header.0, header.1, &mut req)?;
            }

            // Add the request_id header.
            append_header(REQUEST_ID_HEADER, ctx.request_id(), &mut req)?;

            self.trace_request();

            // Make the request now with the appropriate body type.
            let resp = match &self.body {
                None => req.trace_request().send().await,
                Some(body) => req.trace_request().send_body(serde_json::to_string(body)?).await
            };

            // Handle the response - re-trying if an error occurs.
            match resp {
                Ok(resp) if resp.status().as_u16() < 500 => {
                    break Ok(resp);
                },
                Ok(resp) => {
                    // If we have a response but it's a 50x.
                    actix_rt::time::delay_for(Duration::from_secs(ctx.config().client_retry_delay)).await;
                    attempts += 1;

                    // If retries exceeded fail.
                    if self.dont_retry || (attempts > ctx.config().client_retry_limit) {
                        break Err(InternalError::RemoteRequestError { cause: format!("Remote request returned {}", resp.status()), url: url.to_string() });
                    }

                    // Only warn once.
                    if attempts == 2 {
                        warn!("Request to {} failed with status {}, retrying...", url.to_string(), resp.status());
                    }
                },
                Err(err) => {
                    actix_rt::time::delay_for(Duration::from_secs(ctx.config().client_retry_delay)).await;
                    attempts += 1;

                    // If retries exceeded fail.
                    if self.dont_retry || (attempts > ctx.config().client_retry_limit) {
                        break Err(err.into());
                    }

                    // Only warn once.
                    if attempts == 2 {
                        warn!("Request to {} failed with {}, retrying...", url.to_string(), err.to_string());
                    }
                },
            };
        }?;

        let resp = HttpResponse {
            url: url.to_string(),
            method: self.method.clone(),
            body: resp.body().await?,
            inner: resp
        };

        resp.trace();
        Ok(resp)
    }

    fn trace_request(&self) {
        if tracer::tracer_on() {
            let body = match &self.body {
                None => String::default(),
                Some(body) => format!("\n{}", String::from_utf8(body.clone()).unwrap_or("cant read body".to_string())),
            };

            let headers = match self.headers.is_empty() {
                true => String::default(),
                false => format!("\n{}", self.headers.iter().map(|(key, value)| format!("{}: {}", key, value)).join("\n"))
            };

            info!("Sending downstream request\n{} {}{}{}",
                self.method,
                self.url,
                headers,
                body);
        }
    }
}

fn append_header(name: &str, value: &str, req: &mut ClientRequest) -> Result<(), InternalError> {
    req.headers_mut().append(
        HeaderName::from_str(name)?,
        HeaderValue::from_str(value)?);
    Ok(())
}

pub struct HttpResponse {
    url: String,    // The original request URL.
    method: Method, // The original request HTTP method.
    body: Bytes,    // Any received payload.
    inner: ActixHttpResponse
}

impl HttpResponse {
    pub fn status(&self) -> u16 {
        self.inner.status().as_u16()
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn method(&self) -> Method {
        self.method.clone()
    }

    fn trace(&self) {
        if tracer::tracer_on() {
            let body = match self.body.len() {
                0 => String::default(),
                _ => format!("\n{}", String::from_utf8_lossy(&self.body)),
            };

            let headers = match self.inner.headers().is_empty() {
                true => String::default(),
                false => format!("\n{}", self.inner.headers().iter().map(|(key, value)| format!("{}: {}", key, value.to_str().unwrap_or("cant read header"))).join("\n"))
            };

            info!("Received response from downstream request\n{} {} {}{}{}",
                self.method,
                self.url,
                self.inner.status(),
                headers,
                body);
        }
    }

    pub fn json<T: DeserializeOwned>(&self) -> Result<T, InternalError> {
        Ok(serde_json::from_slice(&self.body)?)
    }
}

pub fn post(url: String) -> HttpRequest {
    HttpRequest::new(Method::POST, url)
}

pub fn _put(url: String) -> HttpRequest {
    HttpRequest::new(Method::PUT, url)
}

pub fn get(url: String) -> HttpRequest {
    HttpRequest::new(Method::GET, url)
}

pub fn _delete(url: String) -> HttpRequest {
    HttpRequest::new(Method::DELETE, url)
}