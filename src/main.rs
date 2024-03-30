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

mod channel;
mod service;

const SERVICE_NAME: &str = "asset-service-rs";
const SERVICE_TYPE: ServiceType = ServiceType::OWN_PROCESS;
const LOG_DIRECTORY: &str = r"C:\Users\daniel.eades\Desktop";

define_windows_service!(ffi_service_main, service_main);

fn service_main(arguments: Vec<OsString>) {
    if let Err(e) = run_service(&arguments) {
        tracing::error!("error: {}", e);
    }
}

#[tracing::instrument(err)]
fn run_service(arguments: &[OsString]) -> Result<(), Error> {
    let address = SocketAddr::from_str("[::1]:10000").unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();

    let (grpc_shutdown_tx, grpc_shutdown_rx) = channel::channel();
    let grpc_shutdown_signal = async move { grpc_shutdown_rx.recv().await };

    let grpc_task_handle = rt.spawn(service::run(address, grpc_shutdown_signal));

    let event_handler = move |control: ServiceControl| {
        tracing::debug!("received a control command: {:?}", &control);
        match control {
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            ServiceControl::Stop => {
                tracing::debug!("received stop request");
                // send shutdown command to gRPC server
                tracing::debug!("sending gRPC shutdown command");
                rt.block_on(grpc_shutdown_tx.send());
                tracing::debug!("gRPC shutdown command sent");

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

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(grpc_task_handle)
        .unwrap();

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
    let file_appender = tracing_appender::rolling::hourly(LOG_DIRECTORY, "grpc-service.log");
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
