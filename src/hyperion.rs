use std::{future::Future, task::Context};
use std::task::Poll;

use futures::{FutureExt, ready};

pub trait Service<Request> {
    type Response;
    type Error;
    type ReadyFuture: Future<Output = Result<(), Self::Error>> + 'static;
    type CallFuture: Future<Output = Result<Self::Response, Self::Error>> + 'static;

    fn ready(&mut self) -> Self::ReadyFuture;

    fn call(&mut self, request: Request) -> Self::CallFuture;

    fn compat(self) -> HyperService<Self, Self::ReadyFuture>
    where
        Self: Sized,
    {
        HyperService::from(self)
    }
}

pub struct HyperService<Svc, ReadyFuture> {
    service: Svc,
    ready_future: Option<ReadyFuture>,
}

impl<Request, Svc, Response, Error, ReadyFuture> tower::Service<Request> for HyperService<Svc, ReadyFuture>
where
    Svc: Service<Request, Response = Response, Error = Error, ReadyFuture = ReadyFuture>,
    Svc::ReadyFuture: Future<Output = Result<(), Error>> + Unpin,
{
    type Response = Response;

    type Error = Error;

    type Future = Svc::CallFuture;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let service = &mut self.service;
        let future = self.ready_future.get_or_insert_with(|| service.ready());
        let result = ready!(future.poll_unpin(cx));

        self.ready_future.take();

        Poll::Ready(result)
    }

    fn call(&mut self, request: Request) -> Self::Future {
        self.service.call(request)
    }
}

impl<Svc, ReadyFuture> From<Svc> for HyperService<Svc, ReadyFuture> {
    fn from(service: Svc) -> Self {
        Self {
            service,
            ready_future: None,
        }
    }
}

// pub fn make_service_fn<F, MkRet, Target, Svc, MkErr, ReqBody, Ret, ResBody, E>(f: F)
// -> impl for<'a> tower::Service<&'a Target, Error = MkErr, Response = Svc, Future = MkRet>
// where
//     F: FnMut(&Target) -> MkRet,
//     MkRet: Future<Output = Result<Svc, MkErr>> + Send,
//     MkErr: Into<Box<dyn StdError + Send + Sync>>,
//     Svc: tower::Service<Request<ReqBody>, Future = Ret, Response = Response<ResBody>, Error = E>,
//     ReqBody: HttpBody,
//     Ret: Future<Output = Result<Response<ResBody>, E>> + Send,
//     E: Into<Box<dyn StdError + Send + Sync>>,
//     ResBody: HttpBody,
// {
//     service::make_service_fn(f)
// }

// pub fn service_fn<F, ReqBody, Ret, ResBody, E>(f: F)
// -> impl tower::Service<Request<ReqBody>, Future = Ret, Response = Response<ResBody>, Error = E>
// where
//     F: FnMut(Request<ReqBody>) -> Ret,
//     ReqBody: HttpBody,
//     Ret: Future<Output = Result<Response<ResBody>, E>> + Send,
//     E: Into<Box<dyn StdError + Send + Sync>>,
//     ResBody: HttpBody,
// {
//     service::service_fn(f)
// }


// pub trait MyService<Request>: Service<
//     Request,
//     Future = <Self as MyService<Request>>::Future,
//     Response = <Self as MyService<Request>>::Response,
//     Error = <Self as MyService<Request>>::Error,
// > {
//     type Future: Future<Output = Result<
//         <Self as MyService<Request>>::Response,
//         <Self as MyService<Request>>::Error,
//     >> + Send;
//     type Response;
//     type Error;
// }

// impl<Request, Svc> MyService<Request> for Svc
// where
//     Svc: Service<Request>,
//     for<> <Svc as Service<Request>>::Future: Send,
// {
//     type Future = Svc::Future;

//     type Response = Svc::Response;

//     type Error = Svc::Error;
// }
