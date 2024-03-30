use std::{ffi::OsString, net::SocketAddr, str::FromStr, time::Duration};

use windows_service::{
    define_windows_service,
    service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    },
    service_control_handler::{self, ServiceControlHandlerResult},
    service_dispatcher,
};
mod server;
mod service;

use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

const SERVICE_NAME: &str = "asset-service-rs";
const SERVICE_TYPE: ServiceType = ServiceType::OWN_PROCESS;
const LOG_DIRECTORY: &str = r"C:\Users\pronebird\Desktop";

define_windows_service!(ffi_service_main, service_main);

fn service_main(arguments: Vec<OsString>) {
    if let Err(e) = run_service(&arguments) {
        tracing::error!("error: {}", e);
    }
}

struct ShutdownSignal(oneshot::Sender<()>);
impl ShutdownSignal {
    fn new(sender: oneshot::Sender<()>) -> Self {
        Self(sender)
    }

    fn acknowledge(self) {
        if self.0.send(()).is_err() {
            tracing::debug!("Failed to acknowledge shutdown.");
        }
    }
}

#[tracing::instrument(err)]
fn run_service(arguments: &[OsString]) -> Result<(), Error> {
    let address = SocketAddr::from_str("[::1]:10000").unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();

    let (shutdown_tx, mut shutdown_rx) = mpsc::unbounded_channel();

    let event_handler = move |control: ServiceControl| {
        tracing::debug!("received a control command: {:?}", control);
        match control {
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            ServiceControl::Stop => {
                tracing::debug!("Sending shutdown...");

                // Create a oneshot channel that will be used to initiate the shutdown and receive the acknowledgment upon completion.
                let (completion_tx, completion_rx) = oneshot::channel();

                // Send a shutdown signal.
                match shutdown_tx.send(ShutdownSignal::new(completion_tx)) {
                    Ok(()) => {
                        tracing::debug!("Sent shutdown signal. Waiting for completion...");

                        // Block current thread until the service has shut down or unless the channel errors.
                        match completion_rx.blocking_recv() {
                            Ok(()) => {
                                tracing::debug!("Shutdown is complete. Return control to the service dispatcher.");
                            }
                            Err(e) => {
                                tracing::debug!("Couldn't receive a shutdown completion: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::debug!("Couldn't send a shutdown signal: {}", e);
                    }
                }

                ServiceControlHandlerResult::NoError
            }
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };

    let status_handle = service_control_handler::register(SERVICE_NAME, event_handler)?;

    tracing::info!("setting service status to 'running'...");
    status_handle.set_service_status(ServiceStatus {
        service_type: SERVICE_TYPE,
        current_state: ServiceState::Running,
        controls_accepted: ServiceControlAccept::STOP,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    })?;
    tracing::info!("service status is 'running'");

    rt.block_on(async {
        // Create cancellation token that will be used to shut down the service.
        let shutdown_token = CancellationToken::new();

        // Clone shutdown token and convert it into owned cancellation future,
        // which will complete when shutdown_token is cancelled.
        let shutdown_fut = shutdown_token.clone().cancelled_owned();
        let service_join_handle = tokio::spawn(service::run(address, shutdown_fut));

        // Wait for shutdown message.
        let shutdown = shutdown_rx.recv().await;

        // Cancel shutdown token to resolve the shutdown_fut.
        tracing::debug!("Shutdown triggered. Cancelling the service...");
        shutdown_token.cancel();
        if let Err(e) = service_join_handle.await {
            tracing::error!("Couldn't join on service: {}", e);
        }

        if let Some(shutdown) = shutdown {
            tracing::debug!(
                "The service has shut down. Send the reply back to control event handler."
            );
            shutdown.acknowledge();
        } else {
            tracing::debug!("The shutdown channel was dropped too soon!");
        }
    });

    tracing::debug!("gRPC server has shutdown");

    // Tell the system that service has stopped.
    status_handle.set_service_status(ServiceStatus {
        service_type: SERVICE_TYPE,
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    })?;

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file_appender = tracing_appender::rolling::daily(LOG_DIRECTORY, "grpc-service.log");
    let subscriber = tracing_subscriber::fmt()
        .with_writer(file_appender)
        .with_max_level(tracing::Level::DEBUG)
        .finish();

    // Set the subscriber globally for the application
    tracing::subscriber::set_global_default(subscriber)
        .expect("Failed to set the global default subscriber");

    service_dispatcher::start(SERVICE_NAME, ffi_service_main)?;
    Ok(())
}

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("gRPC transport error: {0}")]
    Transport(#[from] tonic::transport::Error),

    #[error("windows service error: {0}")]
    WindowsService(#[from] windows_service::Error),

    #[error("tokio runtime error: {0}")]
    Runtime(#[from] std::io::Error),
}
