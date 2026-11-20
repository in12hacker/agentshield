use super::*;
use std::os::unix::fs::PermissionsExt;
fn route() -> ProviderRoute {
    ProviderRoute {
        id: RouteId([4; 16]),
        origin: "https://api.example.test".into(),
        method: "POST".into(),
        path: "/v1/responses".into(),
        profile: ProviderProfile::OpenAiResponses,
        session: None,
    }
}
fn version() -> ConfigVersion {
    ConfigVersion::from_digest([5; 32])
}
fn key() -> as_types::process_tree::ProcessKey {
    as_types::process_tree::ProcessKey {
        boot_id: [6; 16],
        host_tgid: 100,
        birth_boottime_ns: 1000,
    }
}
async fn initialize(dir: &tempfile::TempDir) -> CredentialModule {
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    CredentialModule::initialize(CredentialInit {
        protected_root: dir.path().to_owned(),
        store_file: dir.path().join("data/credentials/store"),
        ca_public_file: dir.path().join("data/credentials/ca.crt"),
        bindings: Arc::new(CredentialSettings::default()),
        routes: vec![route()],
        config_version: version(),
    })
    .await
    .unwrap()
}
fn settings(secret: SecretRef) -> Arc<CredentialSettings> {
    Arc::new(CredentialSettings {
        bindings: vec![CredentialBinding {
            id: BindingId([7; 16]),
            secret,
            enabled: true,
            subjects: vec![CredentialSubject::Process(key())],
            routes: vec![route().id],
            slot: AuthSlot::AuthorizationBearer,
        }],
    })
}
fn source() -> IngressFacts {
    IngressFacts {
        connection_initiator: ActorFacts {
            process: Some(key()),
            info: as_types::dispatch_event::ProcessInfo::default(),
            tid: Some(100),
            thread_birth_boottime_ns: Some(1000),
            cgroup_id: None,
            observed_boottime_ns: 1000,
        },
        channel: ChannelRef {
            kind: ChannelKind::Socket,
            source_instance: [8; 16],
            object_id: 10,
            generation: 1,
        },
        original_remote_ip: "127.0.0.1".parse().unwrap(),
        original_remote_port: 443,
        plan_generation: 1,
        config_version: version(),
    }
}
async fn peer(
    module: &CredentialModule,
) -> (
    VerifiedUpstream,
    Arc<rustls::RootCertStore>,
    rustls::ClientConnection,
) {
    let ca = module.ensure_ca().await.unwrap();
    let identity = module
        .server_identity(ca.id, HttpsOrigin::parse(&route().origin).unwrap())
        .await
        .unwrap();
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(rustls::pki_types::CertificateDer::from(ca.certificate_der))
        .unwrap();
    let roots = Arc::new(roots);
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let client = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots.clone())
        .with_no_client_auth();
    let resolver = rustls::server::ResolvesServerCertUsingSni::new();
    let mut resolver = resolver;
    resolver
        .add("api.example.test", (*identity.certified_key).clone())
        .unwrap();
    let server = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(resolver));
    let mut c =
        rustls::ClientConnection::new(Arc::new(client), "api.example.test".try_into().unwrap())
            .unwrap();
    let mut s = rustls::ServerConnection::new(Arc::new(server)).unwrap();
    for _ in 0..16 {
        let mut b = Vec::new();
        c.write_tls(&mut b).unwrap();
        if !b.is_empty() {
            s.read_tls(&mut b.as_slice()).unwrap();
            s.process_new_packets().unwrap();
        }
        let mut b = Vec::new();
        s.write_tls(&mut b).unwrap();
        if !b.is_empty() {
            c.read_tls(&mut b.as_slice()).unwrap();
            c.process_new_packets().unwrap();
        }
        if !c.is_handshaking() && !s.is_handshaking() {
            break;
        }
    }
    let proof = VerifiedUpstream::from_tls(
        HttpsOrigin::parse(&route().origin).unwrap(),
        &c,
        roots.clone(),
    )
    .unwrap();
    (proof, roots, c)
}
fn request<'a>(
    source: &'a IngressFacts,
    phantom: &'a [u8],
    peer: &'a VerifiedUpstream,
) -> CredentialUseRequest<'a> {
    CredentialUseRequest {
        exchange: ExchangeKey {
            flow_instance: [9; 16],
            sequence: 1,
        },
        route: route().id,
        source,
        method: "POST",
        encoded_path: "/v1/responses",
        slot: AuthSlot::AuthorizationBearer,
        phantom,
        peer,
    }
}
#[tokio::test]
async fn persisted_rotation_revocation_and_authorized_header_replacement() {
    let d = tempfile::tempdir().unwrap();
    let module = initialize(&d).await;
    let receipt = module
        .import_secret(PrivateBytes::new(b"old-provider-key".to_vec()))
        .await
        .unwrap();
    module
        .apply_settings(settings(receipt.secret), vec![route()], version())
        .unwrap();
    let issued = module
        .issue_phantom(BindingId([7; 16]), version())
        .await
        .unwrap();
    let (proof, _, _) = peer(&module).await;
    let source = source();
    let lease = module
        .authorize_header(request(&source, issued.value.expose(), &proof))
        .unwrap();
    let mut headers = http::HeaderMap::new();
    lease.apply(&mut headers);
    assert_eq!(headers["authorization"], "Bearer old-provider-key");
    assert!(headers["authorization"].is_sensitive());
    module
        .replace_secret(
            receipt.secret,
            1,
            PrivateBytes::new(b"new-provider-key".to_vec()),
        )
        .await
        .unwrap();
    let mut headers = http::HeaderMap::new();
    module
        .authorize_header(request(&source, issued.value.expose(), &proof))
        .unwrap()
        .apply(&mut headers);
    assert_eq!(headers["authorization"], "Bearer new-provider-key");
    module.revoke_phantom(issued.token_id).await.unwrap();
    assert!(matches!(
        module.authorize_header(request(&source, issued.value.expose(), &proof)),
        Err(CredentialError::Revoked)
    ));
    drop(module);
    let loaded = initialize(&d).await;
    loaded
        .apply_settings(settings(receipt.secret), vec![route()], version())
        .unwrap();
    assert!(matches!(
        loaded.authorize_header(request(&source, issued.value.expose(), &proof)),
        Err(CredentialError::Revoked)
    ));
    let bytes = std::fs::read(d.path().join("data/credentials/store")).unwrap();
    assert!(
        !bytes
            .windows(issued.value.expose().len())
            .any(|s| s == issued.value.expose())
    );
    assert_eq!(
        std::fs::metadata(d.path().join("data/credentials/store"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}
#[tokio::test]
async fn real_tls_proof_cannot_be_relabeled_or_request_scope_forged() {
    let d = tempfile::tempdir().unwrap();
    let module = initialize(&d).await;
    let s = module
        .import_secret(PrivateBytes::new(b"a-key".to_vec()))
        .await
        .unwrap();
    module
        .apply_settings(settings(s.secret), vec![route()], version())
        .unwrap();
    let issue = module
        .issue_phantom(BindingId([7; 16]), version())
        .await
        .unwrap();
    let (proof, roots, c) = peer(&module).await;
    assert!(matches!(
        VerifiedUpstream::from_tls(
            HttpsOrigin::parse("https://attacker.test").unwrap(),
            &c,
            roots,
        ),
        Err(CredentialError::OriginDenied)
    ));
    let mut source = source();
    source
        .connection_initiator
        .process
        .as_mut()
        .unwrap()
        .birth_boottime_ns += 1;
    assert!(matches!(
        module.authorize_header(request(&source, issue.value.expose(), &proof)),
        Err(CredentialError::ScopeDenied)
    ));
    let source = super::tests::source();
    let mut r = request(&source, issue.value.expose(), &proof);
    r.encoded_path = "/v1/responses/other";
    assert!(matches!(
        module.authorize_header(r),
        Err(CredentialError::ScopeDenied)
    ));
    assert!(
        module
            .server_identity(
                module.ensure_ca().await.unwrap().id,
                HttpsOrigin::parse("https://attacker.test").unwrap()
            )
            .await
            .is_err()
    );
}
#[tokio::test]
async fn unrelated_config_change_preserves_connection_but_binding_removal_denies() {
    let d = tempfile::tempdir().unwrap();
    let module = initialize(&d).await;
    let r = module
        .import_secret(PrivateBytes::new(b"provider".to_vec()))
        .await
        .unwrap();
    module
        .apply_settings(settings(r.secret), vec![route()], version())
        .unwrap();
    let issue = module
        .issue_phantom(BindingId([7; 16]), version())
        .await
        .unwrap();
    let (proof, _, _) = peer(&module).await;
    let source = source();
    module
        .apply_settings(
            settings(r.secret),
            vec![route()],
            ConfigVersion::from_digest([99; 32]),
        )
        .unwrap();
    assert!(
        module
            .authorize_header(request(&source, issue.value.expose(), &proof))
            .is_ok()
    );
    module
        .apply_settings(
            Arc::new(CredentialSettings::default()),
            vec![route()],
            version(),
        )
        .unwrap();
    assert!(matches!(
        module.authorize_header(request(&source, issue.value.expose(), &proof)),
        Err(CredentialError::ScopeDenied)
    ));
}
#[tokio::test]
async fn ca_survives_restart_and_corrupt_store_is_never_recreated() {
    let d = tempfile::tempdir().unwrap();
    let m = initialize(&d).await;
    let ca = m.ensure_ca().await.unwrap();
    drop(m);
    let m = initialize(&d).await;
    assert_eq!(
        m.ensure_ca().await.unwrap().certificate_der,
        ca.certificate_der
    );
    drop(m);
    std::fs::write(d.path().join("data/credentials/store"), b"corrupt").unwrap();
    assert!(
        CredentialModule::initialize(CredentialInit {
            protected_root: d.path().to_owned(),
            store_file: d.path().join("data/credentials/store"),
            ca_public_file: d.path().join("data/credentials/ca.crt"),
            bindings: Arc::new(CredentialSettings::default()),
            routes: vec![route()],
            config_version: version()
        })
        .await
        .is_err()
    );
}
#[tokio::test]
async fn nofollow_and_exact_secret_bytes() {
    let d = tempfile::tempdir().unwrap();
    let m = initialize(&d).await;
    assert!(
        m.import_secret(PrivateBytes::new(b"bad\r\nvalue".to_vec()))
            .await
            .is_err()
    );
    let r = m
        .import_secret(PrivateBytes::new(b" exact key ".to_vec()))
        .await
        .unwrap();
    assert_eq!(
        m.store().unwrap().secrets[&r.secret].value.expose(),
        b" exact key "
    );
    let store = d.path().join("data/credentials/store");
    std::fs::remove_file(&store).unwrap();
    std::os::unix::fs::symlink(d.path().join("missing"), &store).unwrap();
    assert!(
        m.import_secret(PrivateBytes::new(b"key".to_vec()))
            .await
            .is_err()
    );
    assert!(!d.path().join("missing").exists());
}

type SaveGate = (std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>);
static SAVE_GATES: std::sync::OnceLock<std::sync::Mutex<HashMap<PathBuf, SaveGate>>> =
    std::sync::OnceLock::new();
static SYNC_FAILURES: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<PathBuf>>> =
    std::sync::OnceLock::new();
pub(super) fn before_save(path: &std::path::Path) {
    let gate = SAVE_GATES
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .remove(path);
    if let Some((started, release)) = gate {
        started.send(()).unwrap();
        release.recv().unwrap();
    }
}
pub(super) fn force_sync_failure(path: &std::path::Path) -> bool {
    SYNC_FAILURES
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .remove(path)
}
#[tokio::test]
async fn caller_cancellation_does_not_abandon_persisted_write_or_allow_stale_followup() {
    let d = tempfile::tempdir().unwrap();
    let m = initialize(&d).await;
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    SAVE_GATES
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .insert(
            d.path().join("data/credentials/store"),
            (started_tx, release_rx),
        );
    let owner = m.clone();
    let waiter = tokio::spawn(async move {
        owner
            .import_secret(PrivateBytes::new(b"first-owned-key".to_vec()))
            .await
    });
    tokio::task::spawn_blocking(move || started_rx.recv().unwrap())
        .await
        .unwrap();
    waiter.abort();
    release_tx.send(()).unwrap();
    {
        let _completed = m.0.writer.lock().await;
    }
    assert_eq!(m.store().unwrap().secrets.len(), 1);
    let second = m
        .import_secret(PrivateBytes::new(b"second-key".to_vec()))
        .await
        .unwrap();
    assert_eq!(m.store().unwrap().secrets.len(), 2);
    assert_eq!(
        m.store().unwrap().secrets[&second.secret].value.expose(),
        b"second-key"
    );
    drop(m);
    assert_eq!(initialize(&d).await.store().unwrap().secrets.len(), 2);
}
#[tokio::test]
async fn renamed_store_is_published_even_when_directory_durability_is_uncertain() {
    let d = tempfile::tempdir().unwrap();
    let m = initialize(&d).await;
    SYNC_FAILURES
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .insert(d.path().join("data/credentials/store"));
    assert!(matches!(
        m.import_secret(PrivateBytes::new(
            b"committed-but-durability-uncertain".to_vec()
        ))
        .await,
        Err(CredentialError::PersistenceUncertain)
    ));
    assert_eq!(m.store().unwrap().secrets.len(), 1);
    drop(m);
    assert_eq!(initialize(&d).await.store().unwrap().secrets.len(), 1);
}

#[tokio::test]
async fn fresh_install_creates_only_anchored_private_directories_and_rejects_ancestor_symlink() {
    let d = tempfile::tempdir().unwrap();
    let module = initialize(&d).await;
    assert_eq!(
        std::fs::metadata(d.path().join("data/credentials"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    drop(module);
    let other = tempfile::tempdir().unwrap();
    std::fs::set_permissions(other.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let external = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(external.path(), other.path().join("data")).unwrap();
    assert!(
        CredentialModule::initialize(CredentialInit {
            protected_root: other.path().to_owned(),
            store_file: other.path().join("data/credentials/store"),
            ca_public_file: other.path().join("data/credentials/ca.crt"),
            bindings: Arc::new(CredentialSettings::default()),
            routes: vec![route()],
            config_version: version()
        })
        .await
        .is_err()
    );
    assert!(!external.path().join("credentials").exists());
}

#[tokio::test]
async fn shutdown_waits_owned_write_and_refuses_new_management() {
    let d = tempfile::tempdir().unwrap();
    let m = initialize(&d).await;
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    SAVE_GATES
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .insert(
            d.path().join("data/credentials/store"),
            (started_tx, release_rx),
        );
    let owner = m.clone();
    let request = tokio::spawn(async move {
        owner
            .import_secret(PrivateBytes::new(b"owned-until-shutdown".to_vec()))
            .await
    });
    tokio::task::spawn_blocking(move || started_rx.recv().unwrap())
        .await
        .unwrap();
    let owner = m.clone();
    let shutdown = tokio::spawn(async move { owner.shutdown().await });
    tokio::task::yield_now().await;
    assert!(!shutdown.is_finished());
    release_tx.send(()).unwrap();
    request.await.unwrap().unwrap();
    shutdown.await.unwrap().unwrap();
    assert_eq!(m.store().unwrap().secrets.len(), 1);
    assert!(matches!(
        m.import_secret(PrivateBytes::new(b"later".to_vec())).await,
        Err(CredentialError::Stopped)
    ));
}

#[tokio::test]
async fn enabled_binding_requires_real_secret_before_start_or_policy_publication() {
    let dir = tempfile::tempdir().unwrap();
    let module = initialize(&dir).await;
    let missing = settings(SecretRef([99; 16]));
    assert_eq!(
        module.validate_settings(&missing, &[route()]),
        Err(CredentialError::SecretUnavailable)
    );
    assert_eq!(
        module.apply_settings(missing.clone(), vec![route()], version()),
        Err(CredentialError::SecretUnavailable)
    );
    module.shutdown().await.unwrap();
    drop(module);
    assert!(matches!(
        CredentialModule::initialize(CredentialInit {
            protected_root: dir.path().to_owned(),
            store_file: dir.path().join("data/credentials/store"),
            ca_public_file: dir.path().join("data/credentials/ca.crt"),
            bindings: missing,
            routes: vec![route()],
            config_version: version(),
        })
        .await,
        Err(CredentialError::SecretUnavailable)
    ));
}
