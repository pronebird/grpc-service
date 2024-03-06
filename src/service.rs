use std::net::SocketAddr;

use crate::server::serve;
use futures_util::Future;

pub async fn run(address: SocketAddr, shutdown_signal: impl Future<Output = ()> + Send) {
    // If this task returns, it's because we sent a shutdown signal. Otherwise it'll just keep restarting and looping
    serve(address, shutdown_signal)
        .await
        .expect("gRPC server error!");
}
