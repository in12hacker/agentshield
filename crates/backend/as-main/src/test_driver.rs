//! Explicit software-only thread provider; no native protection is claimed.
#[path = "../examples/support/software_driver.rs"]
mod fixture;
pub(crate) use fixture::SoftwareDriver;
use std::sync::Arc;
pub(crate) async fn start(
    context: as_types::context::MainContext,
    queues: as_types::configuration::QueueSettings,
    adapter: Arc<dyn as_types::driver::DriverAdapter>,
) -> std::io::Result<crate::MainThreads> {
    let mut threads = crate::MainThreads::prepare(context.clone(), queues).await?;
    let input = as_types::driver::DriverConnectInput {
        platform: context.platform(),
        host_boot_id: [1; 16],
        driver_assets_directory: context.paths().driver_assets_directory().to_path_buf(),
    };
    match threads
        .dispatch()
        .spawn(crate::ProcessingThread::Fast, async move {
            adapter.connect(&input).await
        })
        .await
    {
        Ok(Ok(session)) => {
            threads.bind_driver(crate::messages::driver::DriverPort::bind(session))?;
            Ok(threads)
        }
        _ => {
            let _ = threads.shutdown().await;
            Err(std::io::Error::other(
                "driver initialization failed: test connection rejected",
            ))
        }
    }
}

/// Explicit parameter fixture, not a production path-discovery interface.
pub(crate) fn environment() -> (
    tempfile::TempDir,
    as_types::context::MainContext,
    as_types::configuration::QueueSettings,
) {
    use as_types::context::*;
    let root = tempfile::tempdir().expect("test layout");
    let base = root.path();
    std::fs::create_dir(base.join("config")).expect("config");
    let paths = std::sync::Arc::new(ConfigPaths::from_configuration(ResolvedPaths {
        backend_executable: base.join("bin/as-service"),
        self_protection_root: base.into(),
        configuration_file: base.join("config/as.yaml"),
        rules_file: base.join("config/rules.yaml"),
        driver_assets_directory: base.join("driver"),
        middleware_endpoint: base.join("run/middleware.sock"),
        data_directory: base.join("data"),
        middleware_database_file: base.join("data/middleware/business.sqlite"),
        credential_store_file: base.join("data/credentials/store"),
        ca_public_file: base.join("data/credentials/ca.crt"),
        security_log_file: base.join("log/security.sqlite"),
    }));
    let document: as_types::configuration::ConfigDocument =
        serde_saphyr::from_str(include_str!("../../../../config/as.yaml")).expect("current sample");
    (
        root,
        MainContext::new(paths, TargetPlatform::current()),
        document.queues,
    )
}
