use std::net::SocketAddr;

use hello_world::greeter_server::{Greeter, GreeterServer};
use hello_world::{HelloReply, HelloRequest};
use std::future::Future;
use tonic::{transport::Server, Request, Response, Status};

pub mod hello_world {
    tonic::include_proto!("helloworld");
}

#[derive(Default)]
pub struct MyGreeter {}

#[tonic::async_trait]
impl Greeter for MyGreeter {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        println!("Got a request from {:?}", request.remote_addr());

        let reply = hello_world::HelloReply {
            message: format!("Hello {}!", request.into_inner().name),
        };
        Ok(Response::new(reply))
    }
}

pub async fn serve<F>(addr: SocketAddr, shutdown_signal: F) -> Result<(), tonic::transport::Error>
where
    F: Future<Output = ()> + Send,
{
    let greeter = MyGreeter::default();

    Server::builder()
        .add_service(GreeterServer::new(greeter))
        .serve_with_shutdown(addr, shutdown_signal)
        .await
}
