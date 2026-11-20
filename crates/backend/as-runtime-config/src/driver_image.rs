//! Configuration-owned native projection. No file discovery, policy compilation or driver calls.
use super::{ConfigError, ConfigModule};
use as_types::{
    configuration::DriverConfigVersion,
    context::TargetPlatform,
    driver_control::{DriverCapabilities, DriverFeature},
    driver_image::DriverConfigImage,
};
use sha2::{Digest, Sha256};
use std::{os::unix::ffi::OsStrExt, sync::Arc};

impl ConfigModule {
    /// Export one immutable native projection only when every mandatory requirement is supported.
    pub fn prepare_driver_config(
        &self,
        capabilities: &DriverCapabilities,
    ) -> Result<DriverConfigImage, ConfigError> {
        if !capabilities.supports_feature(DriverFeature::SelfProtection) {
            return Err(ConfigError::Unsupported(
                "complete fixed-file self-protection is required",
            ));
        }
        let snapshot = self.snapshot();
        let settings = &snapshot.document().driver;
        if settings.limits.max_pending.get() > capabilities.max_pending
            || settings.limits.max_hold_ms.get() > capabilities.max_hold_ms
        {
            return Err(ConfigError::Unsupported(
                "configured original-operation limits exceed native capacities",
            ));
        }
        // Initialization-only observation scope comes from the same startup
        // document. Ordinary rule/config publication must not rebind directories.
        let directories = &self.startup.file_close_audit;
        if !directories.is_empty() && !capabilities.supports_feature(DriverFeature::FileCloseAudit)
        {
            return Err(ConfigError::Unsupported(
                "native write-close audit is unavailable",
            ));
        }
        let close_size = directories.iter().try_fold(0usize, |total, path| {
            let length = path.as_os_str().as_bytes().len();
            u32::try_from(length)
                .map_err(|_| ConfigError::Unsupported("audit directory path is too large"))?;
            total
                .checked_add(4)
                .and_then(|n| n.checked_add(length))
                .ok_or(ConfigError::Unsupported(
                    "audit directory projection length overflow",
                ))
        })?;
        let close_count = u32::try_from(directories.len())
            .map_err(|_| ConfigError::Unsupported("audit directory count overflow"))?;
        let close_bytes = u32::try_from(close_size)
            .map_err(|_| ConfigError::Unsupported("audit directory projection is too large"))?;
        let root = snapshot
            .paths()
            .self_protection_root()
            .as_os_str()
            .as_bytes();
        let executable = snapshot.paths().backend_executable().as_os_str().as_bytes();
        let loader_path = snapshot
            .paths()
            .driver_assets_directory()
            .join(as_types::driver::LOADER_FILE_NAME);
        let loader = loader_path.as_os_str().as_bytes();
        let root_len = u32::try_from(root.len())
            .map_err(|_| ConfigError::Unsupported("root binding is too large"))?;
        let executable_len = u32::try_from(executable.len())
            .map_err(|_| ConfigError::Unsupported("executable binding is too large"))?;
        let loader_len = u32::try_from(loader.len())
            .map_err(|_| ConfigError::Unsupported("loader binding is too large"))?;
        let total = 48usize
            .checked_add(root.len())
            .and_then(|n| n.checked_add(executable.len()))
            .and_then(|n| n.checked_add(loader.len()))
            .and_then(|n| n.checked_add(close_size))
            .ok_or(ConfigError::Unsupported(
                "native projection length overflow",
            ))?;
        if total as u64 > capabilities.max_transfer_bytes {
            return Err(ConfigError::Unsupported(
                "native projection exceeds the actual transfer capacity",
            ));
        }
        let defaults = u32::from(settings.startup_protection.process)
            | (u32::from(settings.startup_protection.file) << 1)
            | (u32::from(settings.startup_protection.network) << 2);
        let (group_ports, gateway_ws_port) =
            self.startup_group_observation().map_or((0, 0), |scope| {
                let port = |value: Option<std::num::NonZeroU16>| {
                    value.map_or(0, |port| u32::from(port.get()))
                };
                (
                    port(scope.a2a_port) | (port(scope.otlp_http_port) << 16),
                    port(scope.gateway_ws_port),
                )
            });
        let mut bytes = Vec::with_capacity(total);
        for value in [
            defaults,
            settings.limits.max_pending.get(),
            settings.limits.max_hold_ms.get(),
            DriverFeature::SelfProtection as u32
                | if directories.is_empty() {
                    0
                } else {
                    DriverFeature::FileCloseAudit as u32
                },
            root_len,
            executable_len,
            loader_len,
            u32::from(settings.self_protection.log),
            group_ports,
            gateway_ws_port,
            close_count,
            close_bytes,
        ] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(root);
        bytes.extend(executable);
        bytes.extend(loader);
        for path in directories {
            let raw = path.as_os_str().as_bytes();
            // Lengths were checked before reserving the one complete image.
            bytes.extend((raw.len() as u32).to_le_bytes());
            bytes.extend_from_slice(raw);
        }
        let digest = Sha256::digest(&bytes).into();
        Ok(DriverConfigImage {
            config_version: snapshot.version(),
            driver_config_version: DriverConfigVersion(digest),
            platform: TargetPlatform::current(),
            encoding_revision: 1,
            immutable_buffer: Arc::from(bytes),
        })
    }
}
