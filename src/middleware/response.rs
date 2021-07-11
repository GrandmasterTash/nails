use tracing::info;
use std::pin::Pin;
use std::future::Future;
use itertools::Itertools;
use std::marker::PhantomData;
use actix_http::ResponseHead;
use std::task::{Context, Poll};
use futures::future::{ok, Ready};
use actix_web::web::{Bytes, BytesMut};
use actix_service::{Service, Transform};
use crate::routes::admin::tracer::{colour_status, prelude::*, tracer_on};
use actix_web::body::{BodySize, MessageBody, ResponseBody};
use actix_web::{dev::ServiceRequest, dev::ServiceResponse, Error};

pub struct Middleware;

impl<S: 'static, B> Transform<S> for Middleware
where
    S: Service<Request = ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    B: MessageBody + 'static,
{
    type Request = ServiceRequest;
    type Response = ServiceResponse<BodyLogger<B>>;
    type Error = Error;
    type InitError = ();
    type Transform = LoggingMiddleware<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ok(LoggingMiddleware { service })
    }
}

pub struct LoggingMiddleware<S> {
    service: S,
}

impl<S, B> Service for LoggingMiddleware<S>
where
    S: Service<Request = ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    B: MessageBody,
{
    type Request = ServiceRequest;
    type Response = ServiceResponse<BodyLogger<B>>;
    type Error = Error;
    type Future = WrapperStream<S, B>;

    fn poll_ready(&mut self, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(cx)
    }

    fn call(&mut self, req: ServiceRequest) -> Self::Future {
        let partial_log = match tracer_on(req.headers()) {
            false => None,
            true => {
                let remote_addr = req.connection_info().realip_remote_addr().unwrap_or("unknown").to_string();
                Some(format!("Response sent to {addr}\n{out}{method}{uri}",
                    addr   = remote_addr,
                    out    = *OUT,
                    method = req.method(),
                    uri    = req.uri()))
            }
        };

        WrapperStream {
            partial_log,
            fut: self.service.call(req),
            _t: PhantomData,
        }
    }
}

#[pin_project::pin_project]
pub struct WrapperStream<S, B>
where
    B: MessageBody,
    S: Service,
{
    #[pin]
    partial_log: Option<String>,
    #[pin]
    fut: S::Future,
    _t: PhantomData<(B,)>,
}

impl<S, B> Future for WrapperStream<S, B>
where
    B: MessageBody,
    S: Service<Request = ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
{
    type Output = Result<ServiceResponse<BodyLogger<B>>, Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let projected = self.project();
        let partial_log = projected.partial_log.clone();
        let res = futures::ready!(projected.fut.poll(cx));

        Poll::Ready(res.map(|res| {
            res.map_body(move |resp_head, body| {
                let more_log = match partial_log {
                    None => None,
                    Some(partial_log) => Some(format!("{} {}\n{}",
                        partial_log,
                        colour_status(resp_head.status.as_u16()),
                        format_headers(resp_head))),
                };

                ResponseBody::Body(BodyLogger {
                    more_log,
                    body,
                    body_accum: BytesMut::new(),
                })
            })
        }))
    }
}

#[pin_project::pin_project(PinnedDrop)]
pub struct BodyLogger<B> {
    more_log: Option<String>,
    #[pin]
    body: ResponseBody<B>,
    body_accum: BytesMut,
}

#[pin_project::pinned_drop]
impl<B> PinnedDrop for BodyLogger<B> {
    fn drop(self: Pin<&mut Self>) {
        if let Some(more_log) = &self.more_log {
            let body = match self.body_accum.len() {
                0 => String::default(),
                _ => format!("\n{}", String::from_utf8(self.body_accum.to_vec()).unwrap_or(String::from("cant read body")))
            };
            info!("{}{}\n", more_log, body);
        }
    }
}

impl<B: MessageBody> MessageBody for BodyLogger<B> {
    fn size(&self) -> BodySize {
        self.body.size()
    }

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<Bytes, Error>>> {
        let this = self.project();

        match this.body.poll_next(cx) {
            Poll::Ready(Some(Ok(chunk))) => {
                this.body_accum.extend_from_slice(&chunk);
                Poll::Ready(Some(Ok(chunk)))
            }
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

fn format_headers(rsp: &ResponseHead) -> String {
    rsp.headers()
        .iter()
        .map(|(key, value)| format!("{out}{key}{colon} {value}",
            out   = *OUT,
            key   = key,
            colon = *COLON,
            value = value.to_str().unwrap_or("cant read value")) )
        .join("\n")
}