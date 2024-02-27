use std::net::SocketAddr;

use futures_util::FutureExt;
use crate::server::serve;
use tokio::{runtime::Runtime, sync::mpsc, task::JoinHandle};

struct Stopped {
    address: SocketAddr,
}
struct Running {
    address: SocketAddr,
    shutdown_sender: tokio::sync::mpsc::Sender<()>,
    join_handle: JoinHandle<()>,
}

enum Service {
    Stopped(Stopped),
    Running(Running),
}

impl Stopped {
    pub fn run(self, rt: &Runtime) -> Running {
        let (shutdown_sender, mut shutdown_receiver) = tokio::sync::mpsc::channel::<()>(1);

        // If this task returns, it's because we sent a shutdown signal. Otherwise it'll just keep restarting and looping
        let server_task = async move {
            loop {
                let shutdown_signal = shutdown_receiver.recv().map(|option| option.unwrap());
                match serve(self.address, shutdown_signal).await {
                    Ok(()) => {
                        // this indicates a shutdown was recieved and the server shut down gracefully
                        break;
                    }
                    Err(_e) => {
                        // something went wrong with the gRPC server. restart
                    }
                }
            }
        };

        let join_handle = rt.spawn(server_task);

        Running {
            address: self.address,
            shutdown_sender,
            join_handle,
        }
    }
}

impl Running {
    pub fn stop(self, rt: &Runtime) -> Stopped {
        self.shutdown_sender.blocking_send(()).unwrap();
        rt.block_on(self.join_handle).unwrap();

        Stopped {
            address: self.address,
        }
    }
}

pub enum Signal {
    Start,
    Stop,
    Shutdown,
}

pub async fn run(
    address: SocketAddr,
    rt: &Runtime,
    mut signal_receiver: mpsc::UnboundedReceiver<Signal>,
) {
    let mut service = Service::Running(Stopped { address }.run(rt));
    let mut shutdown = false;
    loop {
        service = match (service, signal_receiver.recv().await.unwrap()) {
            (Service::Stopped(state), Signal::Start) => Service::Running(state.run(rt)),
            (Service::Stopped(state), Signal::Stop) => Service::Stopped(state),
            (Service::Stopped(state), Signal::Shutdown) => {
                shutdown = true;
                Service::Stopped(state)
            }
            (Service::Running(state), Signal::Start) => Service::Running(state),
            (Service::Running(state), Signal::Stop) => Service::Stopped(state.stop(rt)),
            (Service::Running(state), Signal::Shutdown) => {
                shutdown = true;
                Service::Stopped(state.stop(rt))
            }
        };
        if shutdown {
            break;
        }
    }
}
