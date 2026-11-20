//! Persistent provider credentials, phantom scope and interception CA ownership.
#![forbid(unsafe_code)]
mod certificate;
mod storage;
use as_types::{configuration::ConfigVersion, flow::*};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, RwLock},
};
use tokio::sync::Mutex;
/// Initialization borrows fixed configuration-owner paths and validated policy snapshots.
pub struct CredentialInit {
    /// Configuration-owner root used once to anchor all relative file I/O.
    pub protected_root: PathBuf,
    /// Fixed private store file under the protected root.
    pub store_file: PathBuf,
    /// Fixed export path for public CA PEM.
    pub ca_public_file: PathBuf,
    /// Nonsecret current scope policy.
    pub bindings: Arc<CredentialSettings>,
    /// Actual configured routes; methods and paths remain exact.
    pub routes: Vec<ProviderRoute>,
    /// Adopted configuration digest.
    pub config_version: ConfigVersion,
}
#[derive(Clone)]
struct Secret {
    revision: u64,
    value: Arc<PrivateBytes>,
}
#[derive(Clone)]
struct Token {
    binding: BindingId,
    digest: [u8; 32],
    enabled: bool,
}
#[derive(Clone)]
struct Ca {
    id: CaRef,
    der: Arc<Vec<u8>>,
    key: Arc<PrivateBytes>,
}
#[derive(Clone)]
struct Store {
    instance: [u8; 16],
    revision: u64,
    secrets: HashMap<SecretRef, Secret>,
    tokens: HashMap<PhantomTokenId, Token>,
    ca: Option<Ca>,
}
struct Runtime {
    stopped: bool,
    store: Arc<Store>,
    bindings: Arc<CredentialSettings>,
    routes: Vec<ProviderRoute>,
    config_version: ConfigVersion,
}
type CertificateCache =
    HashMap<(CaRef, HttpsOrigin), (std::time::Instant, Arc<rustls::sign::CertifiedKey>)>;
struct Inner {
    tasks: tokio_util::task::TaskTracker,
    state: RwLock<Runtime>,
    writer: Mutex<()>,
    storage: Arc<storage::Storage>,
    certificates: Mutex<CertificateCache>,
}
/// One cohesive owner; clones share the same state and serialized persistent writer.
#[derive(Clone)]
pub struct CredentialModule(Arc<Inner>);
impl CredentialModule {
    /// Read or create the protected store. Corrupt existing material always fails.
    pub async fn initialize(input: CredentialInit) -> Result<Self, CredentialError> {
        let opened = tokio::task::spawn_blocking(move || {
            let storage = storage::Storage::open(
                &input.protected_root,
                &input.store_file,
                &input.ca_public_file,
            )?;
            let store = storage::load(&storage)?;
            Ok::<_, CredentialError>((
                Arc::new(storage),
                store,
                input.bindings,
                input.routes,
                input.config_version,
            ))
        })
        .await
        .map_err(|_| CredentialError::InternalFailure)??;
        let (storage, store, bindings, routes, config_version) = opened;
        check_references(&store, &bindings, &routes)?;
        Ok(Self(Arc::new(Inner {
            tasks: tokio_util::task::TaskTracker::new(),
            state: RwLock::new(Runtime {
                stopped: false,
                store: Arc::new(store),
                bindings,
                routes,
                config_version,
            }),
            writer: Mutex::new(()),
            storage,
            certificates: Mutex::new(HashMap::new()),
        })))
    }
    /// Request authorization only, without exposing administrative material.
    pub fn use_port(&self) -> Arc<dyn CredentialPort> {
        Arc::new(self.clone())
    }
    /// CA signing/public certificate port.
    pub fn certificate_port(&self) -> Arc<dyn CertificatePort> {
        Arc::new(self.clone())
    }
    /// Manager-only mutations; the authenticated caller owns access control.
    pub fn admin_port(&self) -> Arc<dyn CredentialAdminPort> {
        Arc::new(self.clone())
    }
    /// Publish an already-validated configuration atomically with request authorization.
    pub fn validate_settings(
        &self,
        bindings: &CredentialSettings,
        routes: &[ProviderRoute],
    ) -> Result<(), CredentialError> {
        let state = self
            .0
            .state
            .read()
            .map_err(|_| CredentialError::InternalFailure)?;
        if state.stopped {
            return Err(CredentialError::Stopped);
        }
        check_references(&state.store, bindings, routes)
    }
    /// Publish validated policy only when all enabled bindings reference actual material.
    pub fn apply_settings(
        &self,
        bindings: Arc<CredentialSettings>,
        routes: Vec<ProviderRoute>,
        version: ConfigVersion,
    ) -> Result<ConfigVersion, CredentialError> {
        let mut s = self
            .0
            .state
            .write()
            .map_err(|_| CredentialError::InternalFailure)?;
        if s.stopped {
            return Err(CredentialError::Stopped);
        }
        check_references(&s.store, &bindings, &routes)?;
        s.bindings = bindings;
        s.routes = routes;
        s.config_version = version;
        Ok(version)
    }
    /// Reject new authorization/management and await all already-owned persistent writes.
    pub async fn shutdown(&self) -> Result<(), CredentialError> {
        {
            let mut s = self
                .0
                .state
                .write()
                .map_err(|_| CredentialError::InternalFailure)?;
            s.stopped = true;
            self.0.tasks.close();
        }
        self.0.tasks.wait().await;
        Ok(())
    }
    fn spawn_owned<T: Send + 'static>(
        &self,
        future: impl std::future::Future<Output = Result<T, CredentialError>> + Send + 'static,
    ) -> Result<tokio::task::JoinHandle<Result<T, CredentialError>>, CredentialError> {
        let s = self
            .0
            .state
            .read()
            .map_err(|_| CredentialError::InternalFailure)?;
        if s.stopped {
            return Err(CredentialError::Stopped);
        }
        Ok(self.0.tasks.spawn(future))
    }
    fn store(&self) -> Result<Store, CredentialError> {
        Ok((*self
            .0
            .state
            .read()
            .map_err(|_| CredentialError::InternalFailure)?
            .store)
            .clone())
    }
    async fn commit(&self, mut next: Store) -> Result<u64, CredentialError> {
        next.revision = next
            .revision
            .checked_add(1)
            .ok_or(CredentialError::InternalFailure)?;
        let next = Arc::new(next);
        let disk = next.clone();
        let storage = self.0.storage.clone();
        let durable = tokio::task::spawn_blocking(move || storage::save(&storage, &disk))
            .await
            .map_err(|_| CredentialError::InternalFailure)??;
        let revision = next.revision;
        self.0
            .state
            .write()
            .map_err(|_| CredentialError::InternalFailure)?
            .store = next;
        if durable {
            Ok(revision)
        } else {
            Err(CredentialError::PersistenceUncertain)
        }
    }
}
fn check_references(
    store: &Store,
    settings: &CredentialSettings,
    routes: &[ProviderRoute],
) -> Result<(), CredentialError> {
    for binding in settings.bindings.iter().filter(|b| b.enabled) {
        if !store.secrets.contains_key(&binding.secret) {
            return Err(CredentialError::SecretUnavailable);
        }
        if binding.subjects.is_empty()
            || binding.routes.is_empty()
            || binding
                .routes
                .iter()
                .any(|id| !routes.iter().any(|r| r.id == *id))
        {
            return Err(CredentialError::ScopeDenied);
        }
    }
    Ok(())
}
fn random<const N: usize>() -> Result<[u8; N], CredentialError> {
    let mut v = [0; N];
    getrandom::fill(&mut v).map_err(|_| CredentialError::InternalFailure)?;
    Ok(v)
}
fn validate_secret(v: &[u8]) -> Result<(), CredentialError> {
    if v.is_empty()
        || v.len() > MAX_SECRET_BYTES
        || v.iter().any(|b| matches!(b, 0 | b'\r' | b'\n'))
        || http::HeaderValue::from_bytes(v).is_err()
    {
        return Err(CredentialError::InvalidHeader);
    }
    Ok(())
}
fn subject_matches(subject: &CredentialSubject, source: &IngressFacts) -> bool {
    match subject {
        CredentialSubject::Process(k) => source.connection_initiator.process.as_ref() == Some(k),
        CredentialSubject::Container(k) => {
            matches!(source.connection_initiator.info.workload.as_ref(),Some(as_types::workload::Workload::Container(v)) if v==k)
        }
    }
}
impl CredentialPort for CredentialModule {
    fn authorize_header(
        &self,
        r: CredentialUseRequest<'_>,
    ) -> Result<AuthHeaderLease, CredentialError> {
        if r.phantom.len() != 49 || !r.phantom.starts_with(b"as_ph_") {
            return Err(CredentialError::InvalidToken);
        }
        let digest: [u8; 32] = Sha256::digest(r.phantom).into();
        let s = self
            .0
            .state
            .read()
            .map_err(|_| CredentialError::InternalFailure)?;
        if s.stopped {
            return Err(CredentialError::Stopped);
        }
        let token = s
            .store
            .tokens
            .values()
            .find(|t| t.digest == digest)
            .ok_or(CredentialError::InvalidToken)?;
        if !token.enabled {
            return Err(CredentialError::Revoked);
        }
        let b = s
            .bindings
            .bindings
            .iter()
            .find(|b| b.id == token.binding && b.enabled)
            .ok_or(CredentialError::ScopeDenied)?;
        let route = s
            .routes
            .iter()
            .find(|v| v.id == r.route)
            .ok_or(CredentialError::ScopeDenied)?;
        if !b.routes.contains(&r.route)
            || b.slot != r.slot
            || !b.subjects.iter().any(|v| subject_matches(v, r.source))
            || route.method != r.method
            || route.path != r.encoded_path
        {
            return Err(CredentialError::ScopeDenied);
        }
        if HttpsOrigin::parse(&route.origin)? != *r.peer.origin()
            || r.source.original_remote_port != r.peer.origin().port
        {
            return Err(CredentialError::OriginDenied);
        }
        let secret = s
            .store
            .secrets
            .get(&b.secret)
            .ok_or(CredentialError::SecretUnavailable)?;
        let mut bytes = zeroize::Zeroizing::new(Vec::new());
        if b.slot == AuthSlot::AuthorizationBearer {
            bytes.extend_from_slice(b"Bearer ")
        }
        bytes.extend_from_slice(secret.value.expose());
        let header =
            http::HeaderValue::from_bytes(&bytes).map_err(|_| CredentialError::InvalidHeader)?;
        Ok(AuthHeaderLease::new(
            CredentialUseId {
                exchange: r.exchange,
                sequence: 1,
            },
            b.id,
            b.secret,
            secret.revision,
            b.slot,
            header,
        ))
    }
}
impl CredentialModule {
    async fn ensure_ca_owned(&self) -> Result<CaPublicInfo, CredentialError> {
        let _writer = self.0.writer.lock().await;
        let mut store = self.store()?;
        if store.ca.is_none() {
            store.ca = Some(certificate::generate_ca()?);
            self.commit(store).await?;
        }
        let ca = self.store()?.ca.ok_or(CredentialError::SecretUnavailable)?;
        let public = certificate::public(&ca);
        let storage = self.0.storage.clone();
        let der = ca.der.clone();
        tokio::task::spawn_blocking(move || storage::save_public(&storage, &der))
            .await
            .map_err(|_| CredentialError::InternalFailure)??;
        Ok(public)
    }
    async fn import_secret_owned(
        &self,
        value: PrivateBytes,
    ) -> Result<SecretWriteReceipt, CredentialError> {
        validate_secret(value.expose())?;
        let _writer = self.0.writer.lock().await;
        let mut store = self.store()?;
        if store.secrets.len() >= 1024 {
            return Err(CredentialError::InternalFailure);
        }
        let secret = SecretRef(random()?);
        if store.secrets.contains_key(&secret) {
            return Err(CredentialError::InternalFailure);
        }
        store.secrets.insert(
            secret,
            Secret {
                revision: 1,
                value: Arc::new(value),
            },
        );
        self.commit(store).await?;
        Ok(SecretWriteReceipt {
            secret,
            revision: 1,
        })
    }
    async fn replace_secret_owned(
        &self,
        secret: SecretRef,
        expected: u64,
        value: PrivateBytes,
    ) -> Result<SecretWriteReceipt, CredentialError> {
        validate_secret(value.expose())?;
        let _writer = self.0.writer.lock().await;
        let mut store = self.store()?;
        let prior = store
            .secrets
            .get(&secret)
            .filter(|v| v.revision == expected)
            .ok_or(CredentialError::ScopeDenied)?;
        let revision = prior
            .revision
            .checked_add(1)
            .ok_or(CredentialError::InternalFailure)?;
        store.secrets.insert(
            secret,
            Secret {
                revision,
                value: Arc::new(value),
            },
        );
        self.commit(store).await?;
        Ok(SecretWriteReceipt { secret, revision })
    }
    async fn issue_phantom_owned(
        &self,
        binding: BindingId,
        expected_config: ConfigVersion,
    ) -> Result<PhantomIssue, CredentialError> {
        let _writer = self.0.writer.lock().await;
        let mut store = self.store()?;
        {
            let s = self
                .0
                .state
                .read()
                .map_err(|_| CredentialError::InternalFailure)?;
            if s.config_version != expected_config {
                return Err(CredentialError::ScopeDenied);
            }
            let b = s
                .bindings
                .bindings
                .iter()
                .find(|b| b.id == binding && b.enabled)
                .ok_or(CredentialError::ScopeDenied)?;
            if !store.secrets.contains_key(&b.secret)
                || b.routes.is_empty()
                || !b
                    .routes
                    .iter()
                    .all(|id| s.routes.iter().any(|r| r.id == *id))
            {
                return Err(CredentialError::SecretUnavailable);
            }
        }
        if store.tokens.len() >= 8192 {
            return Err(CredentialError::InternalFailure);
        }
        let token_id = PhantomTokenId(random()?);
        if store.tokens.contains_key(&token_id) {
            return Err(CredentialError::InternalFailure);
        }
        let value = PrivateBytes::new(
            format!("as_ph_{}", URL_SAFE_NO_PAD.encode(random::<32>()?)).into_bytes(),
        );
        store.tokens.insert(
            token_id,
            Token {
                binding,
                digest: Sha256::digest(value.expose()).into(),
                enabled: true,
            },
        );
        let revision = self.commit(store).await?;
        if self
            .0
            .state
            .read()
            .map_err(|_| CredentialError::InternalFailure)?
            .config_version
            != expected_config
        {
            return Err(CredentialError::ScopeDenied);
        }
        Ok(PhantomIssue {
            token_id,
            binding,
            value,
            revision,
        })
    }
    async fn revoke_phantom_owned(&self, token: PhantomTokenId) -> Result<u64, CredentialError> {
        let _writer = self.0.writer.lock().await;
        let mut store = self.store()?;
        let token = store
            .tokens
            .get_mut(&token)
            .ok_or(CredentialError::InvalidToken)?;
        token.enabled = false;
        self.commit(store).await
    }
}
#[async_trait::async_trait]
impl CredentialAdminPort for CredentialModule {
    async fn ensure_ca(&self) -> Result<CaPublicInfo, CredentialError> {
        let owner = self.clone();
        self.spawn_owned(async move { owner.ensure_ca_owned().await })?
            .await
            .map_err(|_| CredentialError::InternalFailure)?
    }
    async fn import_secret(
        &self,
        value: PrivateBytes,
    ) -> Result<SecretWriteReceipt, CredentialError> {
        let owner = self.clone();
        self.spawn_owned(async move { owner.import_secret_owned(value).await })?
            .await
            .map_err(|_| CredentialError::InternalFailure)?
    }
    async fn replace_secret(
        &self,
        secret: SecretRef,
        expected: u64,
        value: PrivateBytes,
    ) -> Result<SecretWriteReceipt, CredentialError> {
        let owner = self.clone();
        self.spawn_owned(async move { owner.replace_secret_owned(secret, expected, value).await })?
            .await
            .map_err(|_| CredentialError::InternalFailure)?
    }
    async fn issue_phantom(
        &self,
        binding: BindingId,
        expected_config: ConfigVersion,
    ) -> Result<PhantomIssue, CredentialError> {
        let owner = self.clone();
        self.spawn_owned(async move { owner.issue_phantom_owned(binding, expected_config).await })?
            .await
            .map_err(|_| CredentialError::InternalFailure)?
    }
    async fn revoke_phantom(&self, token: PhantomTokenId) -> Result<u64, CredentialError> {
        let owner = self.clone();
        self.spawn_owned(async move { owner.revoke_phantom_owned(token).await })?
            .await
            .map_err(|_| CredentialError::InternalFailure)?
    }
}
#[async_trait::async_trait]
impl CertificatePort for CredentialModule {
    async fn server_identity(
        &self,
        ca: CaRef,
        origin: HttpsOrigin,
    ) -> Result<TlsServerIdentity, CredentialError> {
        {
            let s = self
                .0
                .state
                .read()
                .map_err(|_| CredentialError::InternalFailure)?;
            if s.stopped {
                return Err(CredentialError::Stopped);
            }
            if !s
                .routes
                .iter()
                .any(|r| HttpsOrigin::parse(&r.origin).ok().as_ref() == Some(&origin))
            {
                return Err(CredentialError::OriginDenied);
            }
        }
        if let Some((created, v)) = self.0.certificates.lock().await.get(&(ca, origin.clone()))
            && created.elapsed() < std::time::Duration::from_secs(3600)
        {
            return Ok(TlsServerIdentity {
                certified_key: v.clone(),
            });
        }
        let saved = self
            .0
            .state
            .read()
            .map_err(|_| CredentialError::InternalFailure)?
            .store
            .ca
            .as_ref()
            .filter(|v| v.id == ca)
            .cloned()
            .ok_or(CredentialError::SecretUnavailable)?;
        let host = origin.ascii_host.clone();
        let certified = tokio::task::spawn_blocking(move || certificate::leaf(&saved, &host))
            .await
            .map_err(|_| CredentialError::InternalFailure)??;
        let mut cache = self.0.certificates.lock().await;
        if cache.len() >= 256 {
            cache.clear()
        }
        cache.insert((ca, origin), (std::time::Instant::now(), certified.clone()));
        Ok(TlsServerIdentity {
            certified_key: certified,
        })
    }
    fn public_ca(&self, ca: CaRef) -> Result<CaPublicInfo, CredentialError> {
        let s = self
            .0
            .state
            .read()
            .map_err(|_| CredentialError::InternalFailure)?;
        if s.stopped {
            return Err(CredentialError::Stopped);
        }
        s.store
            .ca
            .as_ref()
            .filter(|v| v.id == ca)
            .map(certificate::public)
            .ok_or(CredentialError::SecretUnavailable)
    }
}
#[cfg(test)]
mod tests;
