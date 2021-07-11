use uuid::Uuid;
use std::rc::Rc;
use std::pin::Pin;
use std::cell::RefCell;
use itertools::Itertools;
use tracing::{info, trace};
use std::task::{Context, Poll};
use futures::stream::StreamExt;
use actix_service::{Service, Transform};
use futures::future::{ok, Future, Ready};
use actix_web::web::{Bytes, BytesMut, Data};
use actix_http::http::{HeaderName, HeaderValue};
use actix_web::{dev::ServiceRequest, dev::ServiceResponse, Error, HttpMessage};
use crate::{routes::admin::tracer::{prelude::*, tracer_on}, utils::context::{PartialRequestContext, RequestContext}};

/// The header set by the middleware
pub const REQUEST_ID_HEADER: &str = "x-correlation-id";

///
/// This middleware servers a number of purposes.
/// - It ensures a request has a unique request id.
/// - It ensures the response contains the same request id.
/// - It constructs a RequestContext used by HTTP handlers.
/// - It traces the request with tracer if approriate.
///
pub struct Middleware {
    ctx: Data<PartialRequestContext>
}

impl Middleware {
    pub fn new(ctx: Data<PartialRequestContext>) -> Self {
        Middleware { ctx }
    }
}

impl<S: 'static, B> Transform<S> for Middleware
where
    S: Service<Request = ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Request = ServiceRequest;
    type Response = ServiceResponse<B>;
    type Error = Error;
    type InitError = ();
    type Transform = RequestMiddleware<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ok(RequestMiddleware {
            service: Rc::new(RefCell::new(service)),
            ctx: Rc::new(RefCell::new(self.ctx.clone()))
        })
    }
}

pub struct RequestMiddleware<S> {
    // This is special: We need this to avoid lifetime issues.
    service: Rc<RefCell<S>>,
    ctx: Rc<RefCell<Data<PartialRequestContext>>>,
}

impl<S, B> Service for RequestMiddleware<S>
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
        let ctx = self.ctx.clone();

        Box::pin(async move {
            // Ensure the request has a request id - generate or use provided.
            let request_id = ensure_request_has_id(&mut req);

            // Trace the request if appropriate
            let tracer = trace(&mut req).await;

            // Create a RequestContext extractor for the request.
            req.extensions_mut().insert(RequestContext::from(
                ctx.borrow_mut().clone(),
                request_id.clone(),
                tracer));

            // Forward the call now.
            let mut res = svc.call(req).await?;

            // Mirror the request id onto the response.
            ensure_response_has_id(&mut res, &request_id);

            Ok(res)
        })
    }
}


///
/// If the request doesn't have an X-Correlation-Id header, then generate a random ID value
/// and add the header to the request.
///
/// The value found or generated is returned.
///
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
            let header_value = match HeaderValue::from_str(&request_id) {
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

///
/// Create a header for the request_id and put it on the response.
///
fn ensure_response_has_id<B>(res: &mut ServiceResponse<B>, request_id: &str) {
    // Ensure the response has the request id on it.
    let value = match HeaderValue::from_str(request_id) {
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
}

///
/// To trace a payload we must read it from the stream then reconstruct it and set it back.
///
/// If the the request qualifies for tracing, returns true.
///
async fn trace(req: &mut ServiceRequest) -> bool {
    if tracer_on(req.headers()) {
        let mut body = BytesMut::new();
        let mut stream = req.take_payload();
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(chunk) => body.extend_from_slice(&chunk),
                Err(err) => trace!("Unable to read payload: {}", err.to_string()),
            };
        }

        let body = body.freeze();

        info!("Request received from {addr}\n{in}{url}\n{headers}{body}\n",
            addr = req.connection_info().realip_remote_addr().unwrap_or("unknown"),
            in   = *IN,
            url  = format_path(&req),
            headers = format_headers(&req),
            body = format_body(&body));

        // Rebuild the request as we've just consumed the stream.
        let mut payload = actix_http::h1::Payload::empty();
        payload.unread_data(body);
        req.set_payload(payload.into());
        return true
    }
    false
}

fn format_path(req: &ServiceRequest) -> String {
    format!("{} {}", req.method(), req.uri())
}

fn format_headers(req: &ServiceRequest) -> String {
    req.headers().iter()
        .map(|(key, value)| format!("{in}{key}{colon} {value}",
            in    = *IN,
            key   = key,
            colon = *COLON,
            value = value.to_str().unwrap_or("cant read value")))
        .join("\n")
}

fn format_body(body: &Bytes) -> String {
    if body.is_empty() {
        return String::new();
    }

    format!("\n{}", String::from_utf8(body.to_vec()).unwrap_or(String::from("cant read body")))
}