use uuid::Uuid;
use std::pin::Pin;
use tracing::trace;
use futures::Future;
use actix_web::Result;
use actix_web::web::Data;
use std::task::{Context, Poll};
use futures::future::{ok, Ready};
use actix_web::{Error, HttpMessage};
use actix_service::{Service, Transform};
use actix_web::http::header::{HeaderName, HeaderValue};
use actix_web::{dev::ServiceRequest, dev::ServiceResponse};
use crate::utils::context::{PartialRequestContext, RequestContext};


/// The header set by the middleware
pub const REQUEST_ID_HEADER: &str = "x-correlation-id";

/// Request ID wrapper.
pub struct Middleware {
    ctx: Data<PartialRequestContext>
}

impl Middleware {
    pub fn new(ctx: Data<PartialRequestContext>) -> Self {
        Middleware { ctx }
    }
}

impl<S, B> Transform<S> for Middleware
where
    S: Service<Request = ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Request = ServiceRequest;
    type Response = ServiceResponse<B>;
    type Error = Error;
    type InitError = ();
    type Transform = RequestIDMiddleware<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ok(RequestIDMiddleware { service, ctx: self.ctx.clone() })
    }
}

/// Actual actix-web middleware
pub struct RequestIDMiddleware<S> {
    service: S,
    ctx: Data<PartialRequestContext>
}

impl<S, B> Service for RequestIDMiddleware<S>
where
    S: Service<Request = ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Request = ServiceRequest;
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(cx)
    }

    fn call(&mut self, req: ServiceRequest) -> Self::Future {
        // make object mutable (required as the header must be used inside `.call`)
        let mut req = req;
        let request_id = ensure_request_has_id(&mut req);

        // Create a RequestContext extractor for the request.
        req.extensions_mut().insert(RequestContext::from(self.ctx.clone(), request_id.clone()));

        // propagate the call
        let fut = self.service.call(req);

        // Ensure the response has the request-id in it.
        Box::pin(async move {
            let mut res = fut.await?;

            let value = match HeaderValue::from_str(request_id.as_str()) {
                Ok(value) => Some(value),
                Err(err) => {
                    trace!("Unable to set header value for request_id {} : {}", request_id, err.to_string());
                    None
                },
            };

            if let Some(value) = value {
                res.headers_mut().append(
                    HeaderName::from_static(REQUEST_ID_HEADER),
                    value);
            }

            Ok(res)
        })
    }
}

fn ensure_request_has_id(req: &mut ServiceRequest) -> String {
    // Get any existing request id from the caller. If it's not a valid header value (unicode rubbish)
    // the we'll discard it.
    let request_id = match req.headers().get(REQUEST_ID_HEADER) {
        Some(header_value) => {
            match header_value.to_str() {
                Ok(value) => Some(value.to_string()),
                Err(err) => {
                    trace!("Request X-Correlation-ID {:?} was not a valid value it will be replaced: {}", header_value, err);
                    None
                }
            }
        },
        None => None,
    };

    // If there's a valid, existing request_id that's what we'll use, otherwise we'll generate one
    // And use that.
    match request_id {
        Some(request_id) => request_id,
        None => {
            // Generate and set the header - replace any existing.
            let request_id = Uuid::new_v4().to_hyphenated().to_string();

            // Unlikely to go wrong, but the following ensure we don't put rubbish in a header value.
            let header_value = match HeaderValue::from_str(&request_id.clone()) {
                Ok(header_value) => Some(header_value),
                Err(err) => {
                    trace!("Unable to put generated request_id {} into a HeaderValue: {}", request_id, err.to_string());
                    None
                },
            };

            if let Some(header_value) = header_value {
                req.headers_mut().insert(HeaderName::from_static(REQUEST_ID_HEADER), header_value);
            }

            request_id
        }
    }
}