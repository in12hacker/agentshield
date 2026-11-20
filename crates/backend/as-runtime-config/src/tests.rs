use super::*;
fn fixture() -> (tempfile::TempDir, Arc<ConfigModule>) {
    let root = tempfile::tempdir().expect("layout");
    std::fs::create_dir(root.path().join("bin")).expect("bin");
    std::fs::create_dir(root.path().join("config")).expect("config");
    let executable = root.path().join("bin/as-service");
    std::fs::write(&executable, b"fixture executable").expect("executable");
    std::fs::write(
        root.path().join("config/as.yaml"),
        include_str!("../../../../config/as.yaml"),
    )
    .expect("current document");
    let module = Arc::new(ConfigModule::from_executable(executable).expect("fixed binding"));
    (root, module)
}
#[tokio::test]
async fn one_combined_log_limit_is_saved_and_requires_original_owner_reinitialization() {
    let (_root, module) = fixture();
    let before = module.snapshot();
    assert_eq!(before.document().log_storage.total_bytes.get(), 1 << 30);
    let mut changed = (**before.document()).clone();
    changed.log_storage.total_bytes =
        std::num::NonZeroU64::new(2 << 30).expect("positive configured budget");
    let receipt = module
        .update(before.version(), changed.clone())
        .await
        .expect("same config writer");
    assert_eq!(receipt.changed_sections, vec![ConfigSection::LogStorage]);
    assert_eq!(
        receipt.required_effects,
        vec![ConfigEffect::ReinitializeMain]
    );
    let reloaded = module
        .reload(receipt.saved_version)
        .await
        .expect("same fixed source");
    assert_eq!(reloaded.document().log_storage, changed.log_storage);
    assert!(module.reinitialization_pending(&reloaded));
}
#[tokio::test]
async fn writes_are_real_versioned_and_paths_are_resolved_once() {
    let (root, module) = fixture();
    let paths = module.shared_paths();
    let before = module.snapshot();
    let mut edit = (**before.document()).clone();
    edit.driver.startup_protection.process = false;
    let saved = module
        .update(before.version(), edit.clone())
        .await
        .expect("real save");
    assert!(saved.required_effects.contains(&ConfigEffect::ApplyDriver));
    let text = std::fs::read_to_string(root.path().join("config/as.yaml")).expect("actual file");
    let disk: ConfigDocument = serde_saphyr::from_str(&text).expect("typed disk");
    assert_eq!(disk, edit);
    assert_eq!(module.snapshot().version(), saved.saved_version);
    assert!(Arc::ptr_eq(&paths, &module.shared_paths()));
    assert!(matches!(
        module.update(before.version(), edit).await,
        Err(ConfigError::Conflict)
    ));
    module
        .reload(saved.saved_version)
        .await
        .expect("same bound source");
    assert!(Arc::ptr_eq(&paths, &module.shared_paths()));
    assert!(
        std::fs::read_dir(root.path().join("config"))
            .expect("directory")
            .all(|x| x.expect("entry").file_name() == "as.yaml")
    );
}
#[tokio::test]
async fn legacy_paths_invalid_reload_and_symlink_escape_are_rejected() {
    use std::os::unix::fs::symlink;
    let (root, module) = fixture();
    let good = module.snapshot();
    let original = std::fs::read(root.path().join("config/as.yaml")).expect("source");
    std::fs::write(
        root.path().join("config/as.yaml"),
        "rules_directory: other\n",
    )
    .expect("old format");
    assert!(module.reload(good.version()).await.is_err());
    assert_eq!(module.snapshot().version(), good.version());
    std::fs::write(root.path().join("config/as.yaml"), &original).expect("restore");
    let outside = tempfile::tempdir().expect("outside");
    let victim = outside.path().join("victim.yaml");
    std::fs::write(&victim, &original).expect("victim");
    std::fs::remove_file(root.path().join("config/as.yaml")).expect("source");
    symlink(&victim, root.path().join("config/as.yaml")).expect("symlink");
    assert!(
        module
            .update(good.version(), (**good.document()).clone())
            .await
            .is_err()
    );
    assert_eq!(std::fs::read(&victim).expect("outside unchanged"), original);
}

#[tokio::test]
async fn native_projection_requires_real_self_capability_and_only_hashes_driver_inputs() {
    use as_types::driver_control::{DriverCapabilities, DriverFeature};
    let (_root, module) = fixture();
    let mut capabilities = DriverCapabilities {
        max_frame_bytes: 16384,
        max_pending: 64,
        max_transfer_bytes: 4 * 1024 * 1024,
        max_hold_ms: 300000,
        operation_mask: 127,
        condition_mask: (1 << 25) - 1,
        features: 0,
    };
    assert!(matches!(
        module.prepare_driver_config(&capabilities),
        Err(ConfigError::Unsupported(_))
    ));
    capabilities.features = DriverFeature::SelfProtection as u32;
    let before = module
        .prepare_driver_config(&capabilities)
        .expect("declared software capability");
    let snapshot = module.snapshot();
    let mut changed = (**snapshot.document()).clone();
    changed.queues.messages = std::num::NonZeroUsize::new(65).expect("capacity");
    module
        .update(snapshot.version(), changed)
        .await
        .expect("saved queue change");
    let same = module
        .prepare_driver_config(&capabilities)
        .expect("projection");
    assert_ne!(before.config_version, same.config_version);
    assert_eq!(before.driver_config_version, same.driver_config_version);
    let snapshot = module.snapshot();
    let mut changed = (**snapshot.document()).clone();
    changed.driver.self_protection.log = !changed.driver.self_protection.log;
    module
        .update(snapshot.version(), changed)
        .await
        .expect("actual log selection");
    let after = module
        .prepare_driver_config(&capabilities)
        .expect("projection");
    assert_ne!(before.driver_config_version, after.driver_config_version);
    assert_eq!(
        u32::from_le_bytes(
            after.immutable_buffer[28..32]
                .try_into()
                .expect("self log field")
        ),
        0
    );
}

#[tokio::test]
async fn saved_group_scope_never_leaks_into_a_live_driver_replacement() {
    use as_types::driver_control::{DriverCapabilities, DriverFeature};
    let (root, module) = fixture();
    let capabilities = DriverCapabilities {
        max_frame_bytes: 16384,
        max_pending: 64,
        max_transfer_bytes: 4 * 1024 * 1024,
        max_hold_ms: 300000,
        operation_mask: 127,
        condition_mask: (1 << 25) - 1,
        features: DriverFeature::SelfProtection as u32,
    };
    let baseline = module
        .prepare_driver_config(&capabilities)
        .expect("boot image");
    let before = module.snapshot();
    let mut next = (**before.document()).clone();
    next.group_observation = Some(GroupObservation {
        a2a_port: None,
        otlp_http_port: None,
        gateway_ws_port: Some(19090.try_into().expect("nonzero port")),
    });
    let saved = module
        .update(before.version(), next.clone())
        .await
        .expect("persist");
    let for_rule_update = module
        .prepare_driver_config(&capabilities)
        .expect("replacement image");
    assert_eq!(
        for_rule_update.driver_config_version,
        baseline.driver_config_version
    );
    assert!(module.reinitialization_pending(&module.snapshot()));
    module
        .reload(saved.saved_version)
        .await
        .expect("repeat saved source");
    assert!(module.reinitialization_pending(&module.snapshot()));
    next.driver.self_protection.log = !next.driver.self_protection.log;
    module
        .update(saved.saved_version, next)
        .await
        .expect("change live driver settings");
    let for_driver_update = module
        .prepare_driver_config(&capabilities)
        .expect("replacement image");
    assert_ne!(
        for_driver_update.driver_config_version,
        baseline.driver_config_version
    );
    assert_eq!(
        &for_driver_update.immutable_buffer[32..40],
        &baseline.immutable_buffer[32..40]
    );
    let reopened =
        ConfigModule::from_executable(root.path().join("bin/as-service")).expect("reinitialize");
    assert!(!reopened.reinitialization_pending(&reopened.snapshot()));
    let new_scope = reopened
        .prepare_driver_config(&capabilities)
        .expect("new boot image");
    assert_eq!(&new_scope.immutable_buffer[32..36], &0u32.to_le_bytes());
    assert_eq!(&new_scope.immutable_buffer[36..40], &19090u32.to_le_bytes());
}

#[test]
fn group_sources_are_explicit_optional_distinct_ports() {
    let (_root, module) = fixture();
    let current = serde_json::to_value(module.snapshot().document()).expect("current document");
    for scope in [
        serde_json::json!({"a2a_port":1,"otlp_http_port":null,"gateway_ws_port":null}),
        serde_json::json!({"a2a_port":null,"otlp_http_port":65535,"gateway_ws_port":null}),
        serde_json::json!({"a2a_port":null,"otlp_http_port":null,"gateway_ws_port":18789}),
        serde_json::json!({"a2a_port":8001,"otlp_http_port":4318,"gateway_ws_port":18789}),
    ] {
        let mut value = current.clone();
        value["group_observation"] = scope.clone();
        let document: ConfigDocument =
            serde_json::from_value(value).expect("current explicit fields");
        assert!(super::validate(&document).is_ok());
        assert_eq!(
            serde_json::to_value(document.group_observation).expect("original ports"),
            scope
        );
    }
    for scope in [
        serde_json::json!({"a2a_port":null,"otlp_http_port":null,"gateway_ws_port":null}),
        serde_json::json!({"a2a_port":8001,"otlp_http_port":8001,"gateway_ws_port":null}),
        serde_json::json!({"a2a_port":8001,"otlp_http_port":null,"gateway_ws_port":8001}),
        serde_json::json!({"a2a_port":null,"otlp_http_port":4318,"gateway_ws_port":4318}),
    ] {
        let mut value = current.clone();
        value["group_observation"] = scope;
        let document: ConfigDocument = serde_json::from_value(value).expect("typed values");
        assert!(super::validate(&document).is_err());
    }
    for scope in [
        serde_json::json!({"a2a_port":8001,"otlp_http_port":4318}),
        serde_json::json!({"a2a_port":null,"otlp_http_port":null,"gateway_ws_port":0}),
        serde_json::json!({"a2a_port":null,"otlp_http_port":null,"gateway_ws_port":65536}),
    ] {
        let mut value = current.clone();
        value["group_observation"] = scope;
        assert!(serde_json::from_value::<ConfigDocument>(value).is_err());
    }
}
#[test]
fn queue_limits_reject_runtime_panics_before_initialization() {
    let mut document: as_types::configuration::ConfigDocument =
        serde_saphyr::from_str(include_str!("../../../../config/as.yaml")).expect("current config");
    let max = tokio::sync::Semaphore::MAX_PERMITS;
    document.queues.review = std::num::NonZeroUsize::new(max + 1).expect("positive");
    assert!(super::validate(&document).is_err());
    document.queues.review = std::num::NonZeroUsize::new(max).expect("positive");
    assert!(super::validate(&document).is_ok());
    document.queues.callbacks = std::num::NonZeroUsize::new(max).expect("positive");
    assert!(super::validate(&document).is_err());
    document.queues.callbacks = std::num::NonZeroUsize::new(max - 1).expect("positive");
    assert!(super::validate(&document).is_ok());
}
#[tokio::test]
async fn process_tree_changes_have_one_effect_contract_for_save_and_reload() {
    let (_root, module) = fixture();
    let before = module.snapshot();
    let mut document = (**before.document()).clone();
    document.process_tree.query_page_size =
        std::num::NonZeroUsize::new(document.process_tree.query_page_size.get() + 1)
            .expect("positive page");
    let receipt = module
        .update(before.version(), document)
        .await
        .expect("saved source");
    let loaded = module
        .reload(receipt.saved_version)
        .await
        .expect("same bound reload");
    assert!(
        receipt
            .required_effects
            .contains(&ConfigEffect::ReinitializeMain)
    );
    assert_eq!(
        loaded.required_effects_from(&before),
        receipt.required_effects
    );
}

#[tokio::test]
async fn content_permission_revokes_sessions_and_credential_paths_are_fixed() {
    let (_root, module) = fixture();
    let before = module.snapshot();
    assert_eq!(
        before.paths().credential_store_file(),
        before
            .paths()
            .self_protection_root()
            .join("data/credentials/store")
    );
    let mut document = (**before.document()).clone();
    document.middleware.chat_content = true;
    let receipt = module
        .update(before.version(), document)
        .await
        .expect("save grant");
    assert_eq!(
        receipt.required_effects,
        vec![ConfigEffect::ReconnectMiddleware]
    );
}

#[test]
fn route_aliases_and_invalid_credential_references_are_rejected() {
    use as_types::flow::*;
    let mut document: ConfigDocument =
        serde_saphyr::from_str(include_str!("../../../../config/as.yaml")).expect("config");
    let route = ProviderRoute {
        id: RouteId([1; 16]),
        origin: "https://EXAMPLE.com".into(),
        method: "POST".into(),
        path: "/v1/messages".into(),
        profile: ProviderProfile::AnthropicMessages,
        session: None,
    };
    let flow = FlowSettings {
        provider_routes: vec![route.clone()],
        ..FlowSettings::default()
    };
    document.flow = Some(flow);
    assert!(validate(&document).is_ok());
    let mut duplicate = route.clone();
    duplicate.id = RouteId([2; 16]);
    duplicate.origin = "https://example.com:443".into();
    document
        .flow
        .as_mut()
        .expect("flow")
        .provider_routes
        .push(duplicate);
    assert!(validate(&document).is_err());
    document.flow.as_mut().expect("flow").provider_routes.pop();
    document.credentials.bindings.push(CredentialBinding {
        id: BindingId([3; 16]),
        secret: SecretRef([4; 16]),
        enabled: true,
        subjects: vec![],
        routes: vec![route.id],
        slot: AuthSlot::XApiKey,
    });
    assert!(validate(&document).is_err());
}

#[tokio::test]
async fn write_close_scope_exports_from_the_owner_startup_document() {
    use as_types::driver_control::{DriverCapabilities, DriverFeature};
    let (root, module) = fixture();
    let mut capabilities = DriverCapabilities {
        max_frame_bytes: 16384,
        max_pending: 64,
        max_transfer_bytes: 4 * 1024 * 1024,
        max_hold_ms: 300000,
        operation_mask: DriverCapabilities::OPERATION_MASK,
        condition_mask: (1 << 25) - 1,
        features: DriverFeature::SelfProtection as u32,
    };
    let before = module.snapshot();
    let directory = std::path::PathBuf::from("/tmp/agent-audit");
    let mut document = (**before.document()).clone();
    document.file_close_audit = vec![directory.clone()];
    let saved = module
        .update(before.version(), document)
        .await
        .expect("saved source scope");
    assert!(
        saved
            .required_effects
            .contains(&ConfigEffect::ReinitializeMain)
    );
    let pending = module
        .prepare_driver_config(&capabilities)
        .expect("old scope remains active");
    assert_eq!(&pending.immutable_buffer[40..48], &[0; 8]);
    let next = ConfigModule::from_executable(root.path().join("bin/as-service"))
        .expect("full initialization");
    assert!(matches!(
        next.prepare_driver_config(&capabilities),
        Err(ConfigError::Unsupported(_))
    ));
    capabilities.features |= DriverFeature::FileCloseAudit as u32;
    let image = next
        .prepare_driver_config(&capabilities)
        .expect("native scope projection");
    assert_eq!(&image.immutable_buffer[40..44], &1u32.to_le_bytes());
    let mut encoded = (directory.as_os_str().as_encoded_bytes().len() as u32)
        .to_le_bytes()
        .to_vec();
    encoded.extend(directory.as_os_str().as_encoded_bytes());
    assert_eq!(
        &image.immutable_buffer[44..48],
        &(encoded.len() as u32).to_le_bytes()
    );
    assert!(image.immutable_buffer.ends_with(&encoded));
}
