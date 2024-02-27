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

use crate::service::Signal;
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
    let (signal_sender, signal_receiver) = tokio::sync::mpsc::unbounded_channel();

    let event_handler = move |control| match control {
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        ServiceControl::Stop => {
            tracing::debug!("received stop request");
            signal_sender.send(Signal::Stop).unwrap();
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Shutdown => {
            tracing::debug!("received shutdown request");
            signal_sender.send(Signal::Shutdown).unwrap();
            ServiceControlHandlerResult::NoError
        }
        _ => ServiceControlHandlerResult::NotImplemented,
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

    rt.block_on(service::run(address, &rt, signal_receiver));

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file_appender =
        tracing_appender::rolling::hourly(LOG_DIRECTORY, "grpc-service.log");
    let subscriber = tracing_subscriber::fmt()
        .with_writer(file_appender)
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
