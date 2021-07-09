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
use actix_web::body::{BodySize, MessageBody, ResponseBody};
use actix_web::{dev::ServiceRequest, dev::ServiceResponse, Error};

use crate::routes::admin::tracer;

pub struct Logging;

impl<S: 'static, B> Transform<S> for Logging
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
        let remote_addr = req.connection_info().realip_remote_addr().unwrap_or("unknown").to_string();
        WrapperStream {
            remote_addr,
            uri: format!("{} {}", req.method(), req.uri()),
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
    remote_addr: String,
    #[pin]
    uri: String,
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
        let uri = projected.uri.clone();
        let remote_addr = projected.remote_addr.clone();
        let res = futures::ready!(projected.fut.poll(cx));

        Poll::Ready(res.map(|res| {
            res.map_body(move |resp_head, body| {
                let partial_log = match tracer::tracer_on() {
                    true => format!("{} {}\n{}", uri, resp_head.status.as_u16(), format_headers(resp_head)),
                    false => String::default(),
                };

                ResponseBody::Body(BodyLogger {
                    remote_addr,
                    partial_log,
                    body,
                    body_accum: BytesMut::new(),
                })
            })
        }))
    }
}

#[pin_project::pin_project(PinnedDrop)]
pub struct BodyLogger<B> {
    remote_addr: String,
    partial_log: String,
    #[pin]
    body: ResponseBody<B>,
    body_accum: BytesMut,
}

#[pin_project::pinned_drop]
impl<B> PinnedDrop for BodyLogger<B> {
    fn drop(self: Pin<&mut Self>) {
        if tracer::tracer_on() {
            if !self.body_accum.is_empty() {
                let body = String::from_utf8(self.body_accum.to_vec()).unwrap_or(String::from("cant read body"));
                info!("Response sent to {}\n{}\n{}", self.remote_addr, self.partial_log, body);
            } else {
                info!("Response sent to {}\n{}", self.remote_addr, self.partial_log);
            }
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
        .map(|(key, value)| format!("{}: {}", key, value.to_str().unwrap_or("cant read value")) )
        .join("\n")
}