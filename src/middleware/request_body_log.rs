use std::rc::Rc;
use std::pin::Pin;
use tracing::info;
use std::cell::RefCell;
use itertools::Itertools;
use std::task::{Context, Poll};
use futures::stream::StreamExt;
use actix_web::web::{Bytes, BytesMut};
use actix_service::{Service, Transform};
use futures::future::{ok, Future, Ready};
use actix_web::{dev::ServiceRequest, dev::ServiceResponse, Error, HttpMessage};

use crate::routes::admin::tracer;

pub struct Logging;

impl<S: 'static, B> Transform<S> for Logging
where
    S: Service<Request = ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Request = ServiceRequest;
    type Response = ServiceResponse<B>;
    type Error = Error;
    type InitError = ();
    type Transform = LoggingMiddleware<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ok(LoggingMiddleware {
            service: Rc::new(RefCell::new(service)),
        })
    }
}

pub struct LoggingMiddleware<S> {
    // This is special: We need this to avoid lifetime issues.
    service: Rc<RefCell<S>>,
}

impl<S, B> Service for LoggingMiddleware<S>
where
    S: Service<Request = ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Request = ServiceRequest;
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    fn poll_ready(&mut self, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(cx)
    }

    fn call(&mut self, mut req: ServiceRequest) -> Self::Future {
        let mut svc = self.service.clone();

        Box::pin(async move {
            if tracer::tracer_on() {
                let mut body = BytesMut::new();
                let mut stream = req.take_payload();
                while let Some(chunk) = stream.next().await {
                    body.extend_from_slice(&chunk?);
                }

                let body = body.freeze();

                info!("Request received from {}\n{}\n{}{}",
                    req.connection_info().realip_remote_addr().unwrap_or("unknown"),
                    format_path(&req),
                    format_headers(&req),
                    format_body(&body));

                // Rebuild the request as we've just consumed the stream.
                let mut payload = actix_http::h1::Payload::empty();
                payload.unread_data(body);
                req.set_payload(payload.into());
            }

            // Proceed.
            let res = svc.call(req).await?;
            Ok(res)
        })
    }
}

fn format_path(req: &ServiceRequest) -> String {
    format!("{} {}", req.method(), req.uri())
}

fn format_headers(req: &ServiceRequest) -> String {
    req.headers().iter()
        .map(|(key, value)| format!("{}: {}", key, value.to_str().unwrap_or("cant read value")))
        .join("\n")
}

fn format_body(body: &Bytes) -> String {
    if body.is_empty() {
        return String::new();
    }

    format!("\n{}", String::from_utf8(body.to_vec()).unwrap_or(String::from("cant read body")))
}