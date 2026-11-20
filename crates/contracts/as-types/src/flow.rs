//! Shared controlled-egress values and narrow ports. No implementation module dependencies.
use crate::{
    configuration::ConfigVersion,
    dispatch_event::ProcessInfo,
    process_tree::ProcessKey,
    workload::{ContainerInstance, Workload},
};
use serde::{Deserialize, Serialize};
use std::{net::IpAddr, pin::Pin, sync::Arc};
use tokio::io::{AsyncRead, AsyncWrite};
use zeroize::Zeroizing;

macro_rules! identifier {
    ($name:ident, $doc:literal) => {
        #[doc=$doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub [u8; 16]);
    };
}
identifier!(RouteId, "Configured provider route identity.");
identifier!(
    SecretRef,
    "Nonsecret reference to provider authentication material."
);
identifier!(BindingId, "Nonsecret credential scope binding identity.");
identifier!(
    PhantomTokenId,
    "Nonsecret phantom record identity, never its token."
);
identifier!(CaRef, "Persistent interception CA identity.");
/// Canonical HTTPS origin, with ASCII hostname and explicit port.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpsOrigin {
    /// Normalized ASCII DNS name or IP literal, without brackets.
    pub ascii_host: String,
    /// Effective TCP port, never zero.
    pub port: u16,
}
impl HttpsOrigin {
    /// Parse an origin only; paths, credentials, queries and fragments are rejected.
    pub fn parse(value: &str) -> Result<Self, CredentialError> {
        let u = url::Url::parse(value).map_err(|_| CredentialError::OriginDenied)?;
        if u.scheme() != "https"
            || !u.username().is_empty()
            || u.password().is_some()
            || u.path() != "/"
            || u.query().is_some()
            || u.fragment().is_some()
        {
            return Err(CredentialError::OriginDenied);
        }
        let host = u.host().ok_or(CredentialError::OriginDenied)?;
        let ascii_host = match host {
            url::Host::Domain(s) => s.to_owned(),
            url::Host::Ipv4(v) => v.to_string(),
            url::Host::Ipv6(v) => v.to_string(),
        };
        let port = u
            .port_or_known_default()
            .ok_or(CredentialError::OriginDenied)?;
        if port == 0 {
            return Err(CredentialError::OriginDenied);
        }
        Ok(Self { ascii_host, port })
    }
    /// Canonical URL origin suitable for display and exact routing.
    pub fn url(&self) -> String {
        let host = if self.ascii_host.contains(':') {
            format!("[{}]", self.ascii_host)
        } else {
            self.ascii_host.clone()
        };
        format!("https://{host}:{}", self.port)
    }
}
/// The only supported credential header slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthSlot {
    /// Exactly one Authorization: Bearer value.
    AuthorizationBearer,
    /// Exactly one x-api-key value.
    XApiKey,
}
impl AuthSlot {
    /// Exact header selected by the binding.
    pub fn header_name(self) -> http::header::HeaderName {
        match self {
            Self::AuthorizationBearer => http::header::AUTHORIZATION,
            Self::XApiKey => http::header::HeaderName::from_static("x-api-key"),
        }
    }
}
/// Exact subject lifetime, not a PID or container name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum CredentialSubject {
    /// Actual original process lifetime.
    Process(ProcessKey),
    /// Actual runtime container instance.
    Container(ContainerInstance),
}
/// A deterministic route; method and encoded path are exact matches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderRoute {
    /// Stable route identifier.
    pub id: RouteId,
    /// HTTPS origin, parsed and validated by configuration.
    pub origin: String,
    /// Uppercase HTTP method.
    pub method: String,
    /// Original encoded path without query.
    pub path: String,
    /// Explicit schema; never inferred from response text.
    pub profile: ProviderProfile,
    /// Explicit original application session field on this route.
    pub session: Option<SessionField>,
}

/// One declared application session field, independent of the provider conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SessionField {
    /// Original client HTTP field, with a lowercase field name.
    Header { name: String },
    /// RFC 6901 position in the original client JSON body.
    JsonPointer { pointer: String },
}

/// Original application session value with its actual encoding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SessionValue {
    /// Exact HTTP field bytes, with no trimming or truncation.
    Header(Vec<u8>),
    /// Original JSON string or numeric lexical value.
    Json(NativeIdValue),
}

/// A capture issue prevents confirmed session grouping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionCaptureIssue {
    /// The selected field was absent from the acquired request.
    Missing,
    /// More than one original HTTP field value was present.
    MultipleValues,
    /// The selected JSON value was not a string or number.
    UnsupportedValue,
    /// A complete unambiguous JSON body was not decoded.
    UnparsedBody,
}

/// Original selected field and all values acquired within this request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionObservation {
    /// Exact configured location in this original request.
    pub field: SessionField,
    /// All acquired values; empty means no supported value was obtained.
    pub values: Vec<SessionValue>,
    /// Capture defect; None means the single value was acquired unambiguously.
    pub issue: Option<SessionCaptureIssue>,
}
/// Nonsecret scope policy; provider key and phantom bytes never appear here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialBinding {
    /// Stable policy identity.
    pub id: BindingId,
    /// Secret record reference.
    pub secret: SecretRef,
    /// Whether new leases are permitted.
    pub enabled: bool,
    /// Exact allowed subjects.
    pub subjects: Vec<CredentialSubject>,
    /// Exact allowed route identifiers.
    pub routes: Vec<RouteId>,
    /// Header slot requiring a phantom.
    pub slot: AuthSlot,
}
/// All nonsecret credential bindings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialSettings {
    /// Unique binding identifiers.
    pub bindings: Vec<CredentialBinding>,
}
/// Explicit transport mode; no failure-driven fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlowMode {
    /// Passive mode, not implemented by controlled egress.
    ObserveTls,
    /// Two independently validated TLS legs.
    MediateTls,
}
/// Provider schema fixed by a configured route.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderProfile {
    /// POST /v1/responses.
    OpenAiResponses,
    /// POST /v1/chat/completions.
    OpenAiChatCompletions,
    /// POST /v1/messages.
    AnthropicMessages,
}
/// Protected-source selection; unknown identity does not match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "values",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum FlowScope {
    /// All positively identified protected subjects.
    AllProtected,
    /// Exact workloads.
    Workloads(Vec<Workload>),
    /// Exact process lifetimes.
    Processes(Vec<ProcessKey>),
}
/// Decoder bounds, all bytes measured after HTTP decoding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecodeLimits {
    /// Per-connection buffer budget.
    pub max_connection_buffer_bytes: u64,
    /// Per JSON body or SSE unit.
    pub max_message_bytes: u64,
    /// Concurrent HTTP exchanges.
    pub max_active_exchanges: u32,
    /// Maximum JSON nesting.
    pub max_json_depth: u32,
}
/// Explicit two-leg TLS trust and capacity settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TlsEgressSettings {
    /// Existing CA record.
    pub ca: CaRef,
    /// Admitted connections.
    pub max_connections: u32,
    /// In-flight streams on one connection.
    pub max_streams_per_connection: u32,
}
/// Sole persisted flow settings; secrets are always separate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlowSettings {
    /// Explicit enablement.
    pub enabled: bool,
    /// Source subject selection.
    pub scope: FlowScope,
    /// Explicit transport selection.
    pub mode: FlowMode,
    /// Unique exact routes.
    pub provider_routes: Vec<ProviderRoute>,
    /// Whether content is observed.
    pub capture_content: bool,
    /// Parsing budgets.
    pub decode_limits: DecodeLimits,
    /// Bytes charged only while facts wait in the unique real-time output queue.
    pub event_queue_bytes: u64,
    /// Required for enabled mediation.
    pub egress: Option<TlsEgressSettings>,
}
impl Default for FlowSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            scope: FlowScope::AllProtected,
            mode: FlowMode::MediateTls,
            provider_routes: Vec::new(),
            capture_content: true,
            decode_limits: DecodeLimits {
                max_connection_buffer_bytes: 2 * 1024 * 1024,
                max_message_bytes: 1024 * 1024,
                max_active_exchanges: 128,
                max_json_depth: 64,
            },
            event_queue_bytes: 2 * 1024 * 1024,
            egress: None,
        }
    }
}
/// Actual original connection-initiator observations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorFacts {
    /// Strong process identity, if obtained.
    pub process: Option<ProcessKey>,
    /// Obtained common process fields.
    pub info: ProcessInfo,
    /// Actual calling task identifier.
    pub tid: Option<u32>,
    /// Actual task birth BOOTTIME nanoseconds.
    #[serde(with = "crate::decimal::optional")]
    pub thread_birth_boottime_ns: Option<u64>,
    /// Obtained cgroup numeric ID.
    #[serde(with = "crate::decimal::optional")]
    pub cgroup_id: Option<u64>,
    /// Actual sample BOOTTIME nanoseconds.
    #[serde(with = "crate::decimal")]
    pub observed_boottime_ns: u64,
}
/// Actual original object represented by this source binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelKind {
    /// Original kernel socket; passive TLS-session identities are not implemented.
    Socket,
}
/// Session-bound original channel identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelRef {
    /// Actual kernel object class.
    pub kind: ChannelKind,
    /// Source instance.
    pub source_instance: [u8; 16],
    /// Source-owned object identity.
    #[serde(with = "crate::decimal")]
    pub object_id: u64,
    /// Object lifetime generation.
    #[serde(with = "crate::decimal")]
    pub generation: u64,
}
/// Facts supplied only by the trusted ingress owner, not client headers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngressFacts {
    /// Original connection initiator.
    pub connection_initiator: ActorFacts,
    /// Original socket/session.
    pub channel: ChannelRef,
    /// Original remote address.
    pub original_remote_ip: IpAddr,
    /// Original remote TCP port.
    pub original_remote_port: u16,
    /// Adopted redirection plan generation.
    pub plan_generation: u64,
    /// Adopted configuration digest.
    pub config_version: ConfigVersion,
}
/// Owned asynchronous duplex transport accepted by a trusted ingress implementation.
pub trait FlowIo: AsyncRead + AsyncWrite + Send + Unpin {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin> FlowIo for T {}
/// Connection-bound authority to dial the exact original peer in its network domain.
#[async_trait::async_trait]
pub trait UpstreamConnect: Send + Sync {
    /// Create one independent upstream transport to this admission's original peer.
    async fn connect(&self) -> std::io::Result<Pin<Box<dyn FlowIo>>>;
}
/// Transport ownership and original source travel together.
pub struct AdmittedConnection {
    /// Owned stream; no shared writer.
    pub stream: Pin<Box<dyn FlowIo>>,
    /// Real ingress facts.
    pub facts: IngressFacts,
    /// Authority from the same source as the admitted stream and original peer.
    pub upstream: std::sync::Arc<dyn UpstreamConnect>,
}
/// Successfully completed rustls handshake bound to an exact upstream origin.
pub struct VerifiedUpstream {
    origin: HttpsOrigin,
}
impl VerifiedUpstream {
    /// Called by trusted TLS transport after the ordinary verifier completed. Never accepts a DTO boolean.
    pub fn from_tls(
        origin: HttpsOrigin,
        connection: &rustls::ClientConnection,
        roots: Arc<rustls::RootCertStore>,
    ) -> Result<Self, CredentialError> {
        use rustls::client::danger::ServerCertVerifier;
        if connection.is_handshaking() {
            return Err(CredentialError::OriginDenied);
        }
        let chain = connection
            .peer_certificates()
            .filter(|v| !v.is_empty())
            .ok_or(CredentialError::OriginDenied)?;
        let verifier = rustls::client::WebPkiServerVerifier::builder_with_provider(
            roots,
            Arc::new(rustls::crypto::ring::default_provider()),
        )
        .build()
        .map_err(|_| CredentialError::OriginDenied)?;
        let name = rustls::pki_types::ServerName::try_from(origin.ascii_host.clone())
            .map_err(|_| CredentialError::OriginDenied)?;
        verifier
            .verify_server_cert(
                &chain[0],
                &chain[1..],
                &name,
                &[],
                rustls::pki_types::UnixTime::now(),
            )
            .map_err(|_| CredentialError::OriginDenied)?;
        Ok(Self { origin })
    }
    /// Origin actually used by the trusted TLS sender.
    pub fn origin(&self) -> &HttpsOrigin {
        &self.origin
    }
}
/// Maximum provider authentication material accepted by the private management path.
pub const MAX_SECRET_BYTES: usize = 16 * 1024;

/// Secret-bearing bounded memory; deliberately no Debug or Serialize.
pub struct PrivateBytes(Zeroizing<Vec<u8>>);
impl PrivateBytes {
    /// Move private bytes into zeroizing storage.
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(Zeroizing::new(bytes))
    }
    /// Explicit trusted-only byte access.
    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}
/// Credential authorization receives one actual request and its verified peer.
pub struct CredentialUseRequest<'a> {
    /// Exchange identity.
    pub exchange: ExchangeKey,
    /// Selected route.
    pub route: RouteId,
    /// Actual source facts.
    pub source: &'a IngressFacts,
    /// Actual HTTP method.
    pub method: &'a str,
    /// Actual encoded path, without query.
    pub encoded_path: &'a str,
    /// Selected header slot.
    pub slot: AuthSlot,
    /// Exact phantom token bytes, without Bearer prefix.
    pub phantom: &'a [u8],
    /// Completed upstream TLS verification.
    pub peer: &'a VerifiedUpstream,
}
/// One-shot authentication header replacement; no global current credential.
pub struct AuthHeaderLease {
    /// Nonsecret use identity.
    pub id: CredentialUseId,
    /// Binding chosen by the actual token mapping.
    pub binding: BindingId,
    /// Actual secret selected by the authorized binding.
    pub secret: SecretRef,
    /// Secret revision used by this request.
    pub secret_revision: u64,
    slot: AuthSlot,
    header: http::HeaderValue,
}
impl AuthHeaderLease {
    /// Created only by a trusted credential provider after authorization.
    pub fn new(
        id: CredentialUseId,
        binding: BindingId,
        secret: SecretRef,
        secret_revision: u64,
        slot: AuthSlot,
        mut header: http::HeaderValue,
    ) -> Self {
        header.set_sensitive(true);
        Self {
            id,
            binding,
            secret,
            secret_revision,
            slot,
            header,
        }
    }
    /// Actual authorized authentication slot, without exposing its secret header.
    pub fn slot(&self) -> AuthSlot {
        self.slot
    }
    /// Consume the lease, replacing exactly the designated authentication header.
    pub fn apply(self, headers: &mut http::HeaderMap) {
        headers.insert(self.slot.header_name(), self.header);
    }
}
/// Public CA information; never the signing key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaPublicInfo {
    /// Persistent identity.
    pub id: CaRef,
    /// X.509 DER certificate.
    pub certificate_der: Vec<u8>,
    /// DER SHA256 fingerprint.
    pub fingerprint_sha256: [u8; 32],
}
/// Signing handle and chain, not raw CA private-key access.
pub struct TlsServerIdentity {
    /// Certified leaf with rustls signing handle.
    pub certified_key: Arc<rustls::sign::CertifiedKey>,
}
/// Safe finite error codes; no key/token/error dump.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialError {
    /// The existing bounded management admission queue is occupied.
    #[error("busy")]
    Busy,
    /// Token not recognized.
    #[error("invalid_token")]
    InvalidToken,
    /// Token has been revoked.
    #[error("revoked")]
    Revoked,
    /// Actual subject or request not in binding scope.
    #[error("scope_denied")]
    ScopeDenied,
    /// Verified upstream origin is not allowed.
    #[error("origin_denied")]
    OriginDenied,
    /// Authentication value is malformed.
    #[error("invalid_header")]
    InvalidHeader,
    /// Secret/CA material unavailable.
    #[error("secret_unavailable")]
    SecretUnavailable,
    /// Owner is stopped.
    #[error("stopped")]
    Stopped,
    /// I/O, invalid store or cryptographic operation failed.
    #[error("internal_failure")]
    InternalFailure,
    /// Atomic rename completed but directory durability could not be confirmed.
    #[error("persistence_uncertain")]
    PersistenceUncertain,
}
/// Actual authorization port, independently evaluated for every HTTP stream.
pub trait CredentialPort: Send + Sync {
    /// Authorize without disk/network I/O.
    fn authorize_header(
        &self,
        request: CredentialUseRequest<'_>,
    ) -> Result<AuthHeaderLease, CredentialError>;
}
/// CA signing port; origins are restricted by credential-owner settings.
#[async_trait::async_trait]
pub trait CertificatePort: Send + Sync {
    /// Obtain allowed origin's bounded-lifetime leaf identity.
    async fn server_identity(
        &self,
        ca: CaRef,
        origin: HttpsOrigin,
    ) -> Result<TlsServerIdentity, CredentialError>;
    /// Read public CA only.
    fn public_ca(&self, ca: CaRef) -> Result<CaPublicInfo, CredentialError>;
}
/// Receipt of an actual persisted secret write.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretWriteReceipt {
    /// Referenced secret.
    pub secret: SecretRef,
    /// Persisted material revision.
    #[serde(with = "crate::decimal")]
    pub revision: u64,
}
/// Issued token returned exactly once; not serializable through ordinary DTO paths.
pub struct PhantomIssue {
    /// Nonsecret record identifier.
    pub token_id: PhantomTokenId,
    /// Binding for this token.
    pub binding: BindingId,
    /// Random token value.
    pub value: PrivateBytes,
    /// Persisted store revision.
    pub revision: u64,
}
/// Authenticated manager-only commands; the message owner enforces ManageService.
#[async_trait::async_trait]
pub trait CredentialAdminPort: Send + Sync {
    /// Create only if no CA has ever existed; otherwise reuse or fail.
    async fn ensure_ca(&self) -> Result<CaPublicInfo, CredentialError>;
    /// Persist new provider secret.
    async fn import_secret(
        &self,
        value: PrivateBytes,
    ) -> Result<SecretWriteReceipt, CredentialError>;
    /// Replace one exact secret revision.
    async fn replace_secret(
        &self,
        secret: SecretRef,
        expected: u64,
        value: PrivateBytes,
    ) -> Result<SecretWriteReceipt, CredentialError>;
    /// Issue random phantom once, only for current configuration.
    async fn issue_phantom(
        &self,
        binding: BindingId,
        expected_config: ConfigVersion,
    ) -> Result<PhantomIssue, CredentialError>;
    /// Persist revocation before returning.
    async fn revoke_phantom(&self, token: PhantomTokenId) -> Result<u64, CredentialError>;
}
/// A single actual HTTP request/response exchange.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ExchangeKey {
    /// Flow runtime identity.
    pub flow_instance: [u8; 16],
    /// Runtime-local sequence.
    #[serde(with = "crate::decimal")]
    pub sequence: u64,
}
macro_rules! exchange_id {
    ($n:ident,$doc:literal) => {
        #[doc=$doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub struct $n {
            /// Owning actual HTTP exchange.
            pub exchange: ExchangeKey,
            /// Exchange-local monotonic sequence.
            #[serde(with = "crate::decimal")]
            pub sequence: u64,
        }
    };
}
exchange_id!(ProtocolRecordId, "One raw HTTP JSON or SSE unit.");
exchange_id!(CredentialUseId, "One credential authorization attempt.");
exchange_id!(
    ChatMessageId,
    "One provider-message projection within an exchange."
);
/// Which side of the two-leg transport supplied this observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolSide {
    /// Original client request.
    ClientRequest,
    /// Sent upstream request.
    UpstreamRequest,
    /// Actual provider response.
    UpstreamResponse,
    /// Response submitted to client.
    ClientResponse,
}
/// Nonsecret true provenance; no inferred business conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolSource {
    /// Real original actor.
    pub actor: ActorFacts,
    /// Actual channel lifetime.
    pub channel: Option<ChannelRef>,
    /// Actual transport side.
    pub side: ProtocolSide,
    /// Original, unique W3C traceparent on the forwarded client request, if valid.
    pub request_traceparent: Option<String>,
    /// Configured protocol schema.
    pub profile: ProviderProfile,
    /// Canonical upstream origin.
    pub origin: String,
    /// Configured route identifier label.
    pub route_label: String,
    /// Adopted configuration digest.
    pub config_version: ConfigVersion,
    /// Actual collection time, Unix milliseconds.
    pub observed_at_utc_ms: Option<i64>,
    /// SHA256 of the actual authorized credential scope; not a provider account or session ID.
    /// None means this source did not acquire an authorized credential scope.
    pub actual_auth_scope: Option<[u8; 32]>,
    /// Actual credential attempt on this exchange.
    pub credential_use: Option<CredentialUseId>,
}

/// Parse the W3C version-00 header without rewriting its original IDs.
pub fn traceparent_ids(value: &str) -> Option<(&str, &str)> {
    let mut parts = value.split('-');
    let (version, trace, span, flags) =
        (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    let hex = |id: &str, length| {
        id.len() == length
            && id
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    };
    (version == "00"
        && hex(trace, 32)
        && trace.bytes().any(|byte| byte != b'0')
        && hex(span, 16)
        && span.bytes().any(|byte| byte != b'0')
        && hex(flags, 2)
        && parts.next().is_none())
    .then_some((trace, span))
}
/// Explicit incompleteness; never replaced by guessed bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureIssue {
    /// Unit exceeds configured budget.
    Truncated,
    /// Bytes or events were lost.
    Gap,
    /// Capture began after protocol start.
    AttachMidStream,
    /// True ordering was not obtained.
    OrderUnknown,
    /// Strong identity was not obtained.
    IdentityUncertain,
}
/// One contiguous original byte range, or explicit omission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EvidenceSegment {
    /// Original unmodified public bytes.
    Bytes {
        /// Offset in decompressed protocol unit.
        #[serde(with = "crate::decimal")]
        offset: u64,
        /// Exact original bytes.
        data: Vec<u8>,
    },
    /// Unavailable original bytes.
    Missing {
        /// Known start offset.
        #[serde(with = "crate::decimal")]
        offset: u64,
        /// Known missing length, otherwise None.
        #[serde(with = "crate::decimal::optional")]
        length: Option<u64>,
        /// Actual reason.
        reason: String,
    },
}
/// Original protocol evidence, without content masking or rewritten bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceBody {
    /// Original decompressed unit length, if known.
    #[serde(with = "crate::decimal::optional")]
    pub original_length: Option<u64>,
    /// Disjoint ranges in original coordinates.
    pub segments: Vec<EvidenceSegment>,
}
impl EvidenceBody {
    /// Transfer one complete protocol unit without copying its original bytes.
    pub fn from_bytes(data: Vec<u8>) -> Self {
        Self {
            original_length: Some(data.len() as u64),
            segments: vec![EvidenceSegment::Bytes { offset: 0, data }],
        }
    }
}
/// Native provider namespace, distinct from AS exchange/event IDs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeIdNamespace {
    /// Request identifier.
    Request,
    /// Response identifier.
    Response,
    /// Message identifier.
    Message,
    /// Explicit provider conversation.
    Conversation,
    /// Explicit previous-response reference.
    PreviousResponse,
    /// Provider tool call.
    ToolCall,
    /// Provider tool result reference.
    ToolResultReference,
    /// Provider event.
    ProviderEvent,
}
/// Retains the JSON scalar type; numeric spelling follows the locked decoder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum NativeIdValue {
    /// JSON string value.
    JsonString(String),
    /// Arbitrary-precision numeric representation, never round-tripped through float.
    JsonNumber(String),
}
/// Provider ID projection with its actual field path; original text stays in protocol evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeProtocolId {
    /// Actual namespace.
    pub namespace: NativeIdNamespace,
    /// JSON path of source field.
    pub field_path: String,
    /// Original scalar.
    pub value: NativeIdValue,
}
/// Raw unit framing, distinct from TLS/network fragments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolUnit {
    /// Complete or explicitly incomplete JSON HTTP body.
    HttpJsonBody,
    /// One dispatchable server-sent event.
    SseEvent,
}
/// Exact decompressed JSON/SSE bytes with safe provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolObservation {
    /// Actual record identity.
    pub id: ProtocolRecordId,
    /// Real source.
    pub source: Arc<ProtocolSource>,
    /// Framing unit.
    pub unit: ProtocolUnit,
    /// SSE event field, if provided.
    pub event_name: Option<String>,
    /// Public original evidence; no resserialized substitute.
    pub body: EvidenceBody,
    /// Original provider references.
    pub native_ids: Vec<NativeProtocolId>,
    /// Explicit collection defects.
    pub quality: Vec<CaptureIssue>,
    /// Selected application session field acquired from this original client request.
    pub request_session: Option<SessionObservation>,
}
/// Actual credential executor state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialUseState {
    /// Scope rejected before sending.
    ScopeRejected,
    /// Lease issued.
    Prepared,
    /// Header set by actual sender.
    AppliedToRequest,
    /// Given to actual HTTP sending stack.
    Submitted,
    /// Actual failure.
    Failed,
}
/// Safe credential result with no secret-bearing values.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialUseFact {
    /// Actual attempt.
    pub id: CredentialUseId,
    /// Attempt-local state revision.
    #[serde(with = "crate::decimal")]
    pub revision: u64,
    /// Real source actor.
    pub actor: ActorFacts,
    /// Selected binding, if known.
    pub binding_ref: Option<BindingId>,
    /// Verified destination.
    pub destination_origin: String,
    /// Actual stage.
    pub state: CredentialUseState,
    /// Header name only.
    pub auth_field: Option<String>,
    /// Safe error code only.
    pub reason: Option<String>,
    /// Actual collection BOOTTIME nanoseconds.
    #[serde(with = "crate::decimal")]
    pub observed_boottime_ns: u64,
}
/// Original provider role, preserving unknown role text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ChatRole {
    /// System instructions.
    System,
    /// Developer instructions.
    Developer,
    /// User message.
    User,
    /// Model output.
    Assistant,
    /// Application-supplied tool result.
    Tool,
    /// Other explicit role.
    Other(String),
}
/// Text or structured content decoded from the original protocol evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ChatContent {
    /// Provider text.
    Text(String),
    /// Explicit provider JSON.
    Json(serde_json::Value),
}
/// Provider content block; its identifier is only message-local.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatPart {
    /// Actual public text.
    Text {
        /// Message-local content index.
        id: u32,
        /// Text/refusal/visible reasoning type.
        kind: TextKind,
        /// Actual provider text.
        text: String,
    },
    /// Provider-requested call, not proof of syscall execution.
    ToolCall {
        /// Content index.
        id: u32,
        /// Original call reference, if present.
        native_id: Option<NativeProtocolId>,
        /// Actual tool name.
        name: String,
        /// Complete arguments only; partial bytes stay in evidence.
        arguments: Option<ChatContent>,
    },
    /// Application-reported tool result.
    ToolResult {
        /// Content index.
        id: u32,
        /// Original matching reference.
        native_id: Option<NativeProtocolId>,
        /// Public actual result.
        content: ChatContent,
    },
    /// Content type not decoded by current profile.
    Opaque {
        /// Content index.
        id: u32,
        /// Original provider type.
        provider_type: String,
        /// Explicit unsupported code.
        reason: String,
    },
    /// Media reference without fetching external content.
    MediaRef {
        /// Content index.
        id: u32,
        /// Provider media type.
        kind: String,
        /// Safe display label, never credentials.
        safe_label: String,
    },
}
/// Text visibility/type explicitly supplied by provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextKind {
    /// Ordinary text.
    Plain,
    /// Provider refusal.
    Refusal,
    /// Public reasoning summary only.
    VisibleReasoning,
}
/// Whether content came from repeated request context or new response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatOrigin {
    /// Actual request history/instructions.
    RequestContext,
    /// Provider response output.
    ResponseOutput,
}
/// Observed message state, independent of transport write success.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatState {
    /// In-progress provider message.
    Streaming,
    /// Known protocol terminal reached.
    Complete,
    /// Incomplete provider message or transport ending without normal completion.
    Interrupted,
    /// Explicit provider or parsing failure.
    Failed,
}
/// Provider usage counter; cumulative values replace, never add blindly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageCounter {
    /// Exact counter name.
    pub name: String,
    /// Exact unsigned value.
    #[serde(with = "crate::decimal")]
    pub value: u64,
}
/// Same-exchange message projection; no middleware conversation/causal graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    /// Message identity.
    pub id: ChatMessageId,
    /// Monotonic message revision.
    #[serde(with = "crate::decimal")]
    pub revision: u64,
    /// Original source.
    pub source: Arc<ProtocolSource>,
    /// Actual evidence references.
    pub record_refs: Vec<ProtocolRecordId>,
    /// Request context or new output.
    pub origin: ChatOrigin,
    /// Provider role.
    pub role: ChatRole,
    /// Actual candidate index, if supplied.
    pub candidate_index: Option<u32>,
    /// Actual model name.
    pub model: Option<String>,
    /// Public content observed so far.
    pub observed_parts: Vec<ChatPart>,
    /// State of this observed message; provider status and capture defects remain separate.
    pub state: ChatState,
    /// Defects remain across finalization.
    pub quality: Vec<CaptureIssue>,
    /// Original provider status.
    pub provider_status: Option<String>,
    /// Actual finish reason.
    pub finish_reason: Option<String>,
    /// Actual cumulative usage if supplied.
    pub usage: Option<Vec<UsageCounter>>,
    /// First actual BOOTTIME nanoseconds.
    #[serde(with = "crate::decimal")]
    pub first_seen_boottime_ns: u64,
    /// Latest actual BOOTTIME nanoseconds.
    #[serde(with = "crate::decimal")]
    pub last_seen_boottime_ns: u64,
}
/// Content update tied to a specific revision.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChatDelta {
    /// Append actual text to one existing part.
    AppendText {
        /// Message-local part index.
        part_id: u32,
        /// Newly observed text only.
        text: String,
    },
    /// Replace a complete current content part.
    PutPart {
        /// Current observed part.
        part: ChatPart,
    },
}
/// Real-time source event identity; it provides no replay, history lookup or acknowledgement API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatCursor {
    /// Flow delivery runtime.
    pub service_instance: [u8; 16],
    /// Monotonic delivery sequence.
    #[serde(with = "crate::decimal")]
    pub sequence: u64,
}
/// Exact read filter; authorization remains with the message owner.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatFilter {
    /// Allowed actual process lifetimes.
    pub processes: Option<Vec<ProcessKey>>,
    /// Allowed actual workloads.
    pub workloads: Option<Vec<Workload>>,
    /// Selected exchanges.
    pub exchanges: Option<Vec<ExchangeKey>>,
}
/// Explicit observation/delivery omission.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservationGap {
    /// Capture or real-time delivery category.
    pub kind: String,
    /// Specific affected exchange, if known.
    pub exchange: Option<ExchangeKey>,
    /// Actual missing cursor interval, if known.
    pub cursor_range: Option<(ChatCursor, ChatCursor)>,
    /// Safe reason code.
    pub reason: String,
}
/// Actual intake lifecycle, independent of already-admitted HTTP streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlowIntakeState {
    /// Admission owner is still connected.
    Running,
    /// Admission owner closed or was cancelled; existing streams keep their real outcomes.
    SourceClosed,
    /// Explicit stop has joined all owned work.
    Stopped,
}
/// Bounded runtime status; no synthetic ready capability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowStatus {
    /// Actual intake owner lifecycle.
    pub intake_state: FlowIntakeState,
    /// Actual selected mode.
    pub mode: FlowMode,
    /// Profiles actually decoded.
    pub decoder_profiles: Vec<ProviderProfile>,
    /// Real admitted active exchanges.
    #[serde(with = "crate::decimal")]
    pub active_exchanges: u64,
    /// Actual undelivered events.
    #[serde(with = "crate::decimal")]
    pub lost_deliveries: u64,
    /// Actual parser failures.
    #[serde(with = "crate::decimal")]
    pub parse_failures: u64,
}
/// One position in an original provider message obtained from the fact consumer; never a global tool ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatPartRef {
    /// Original message identity.
    pub message: ChatMessageId,
    /// Message-local content position.
    pub part_id: u32,
}
/// Bind a real completed denial to an explicitly selected original tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleFeedbackRequest {
    /// Original completed kernel/backend event.
    pub denial: crate::process_tree::EventKey,
    /// Original provider tool call.
    pub call: ChatPartRef,
}
/// Fixed feedback execution progress; it does not change the original HIPS decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleFeedbackFact {
    /// Original denial identity.
    pub denial: crate::process_tree::EventKey,
    /// Original selected call.
    pub call: ChatPartRef,
    /// Actual execution stage.
    pub state: RuleFeedbackState,
}
/// Progress of a single immutable denial/call binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuleFeedbackState {
    /// Unfinished plan admitted; nothing changed or sent.
    Prepared,
    /// Modified JSON generated before sending; evidence coordinates belong to that JSON.
    Applied {
        /// Actual original request record.
        original: ProtocolRecordId,
        /// Exact modified request bytes, distinct from the original request.
        modified_body: Arc<EvidenceBody>,
    },
    /// The actual upstream writer confirmed the whole modified request was written.
    Submitted {
        /// Actual original request record.
        original: ProtocolRecordId,
    },
    /// Actual failure, independent of the confirmed HIPS denial.
    Failed {
        /// Original request record if already acquired.
        original: Option<ProtocolRecordId>,
        /// Finite safe cause.
        code: RuleFeedbackFailure,
    },
}
/// Finite feedback execution failure; never includes content or credentials.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleFeedbackFailure {
    /// Original capture source ended.
    SourceUnavailable,
    /// No matching actual result was present.
    TargetNotFound,
    /// More than one actual result matched.
    AmbiguousTarget,
    /// The corresponding request already started submission.
    TooLate,
    /// Current body encoding or result shape cannot be modified.
    UnsupportedBody,
    /// Existing budget is occupied.
    Capacity,
    /// The backend withdrew this unfinished feedback context.
    Cancelled,
    /// Actual upstream write failed.
    TransportError,
}
/// Original facts used only while validating an explicitly requested write transaction.
/// Runtime-only: a frontend or target Agent cannot deserialize this execution capability.
pub struct RuleFeedbackPrepareInput {
    /// True completed denial, not an OriginalOperation capability.
    pub notice: Arc<crate::main_ports::MainNotice>,
    /// Original provider call selected from the verified business execution relation.
    pub call: ChatPartRef,
    /// Actual original call projection from the business fact owner.
    pub message: Arc<ChatMessage>,
    /// Actual original records; Flow retains none of their bodies after validation.
    pub observations: Vec<Arc<ProtocolObservation>>,
    /// Cancellation of this unfinished feedback context; independent of any frontend session.
    pub cancelled: tokio_util::sync::CancellationToken,
    /// Opaque business-fact custody moved into the actual unfinished transaction.
    /// Only Drop releases it; Flow neither reads the store nor rebuilds this from history.
    pub retention_guard: Option<Box<dyn Send + Sync>>,
}
/// Explicit in-flight feedback only; completed/history ownership belongs to middleware.
#[async_trait::async_trait]
pub trait FlowFeedbackPort: Send + Sync {
    /// Validate original facts and retain only the unfinished write context.
    async fn prepare(&self, input: RuleFeedbackPrepareInput)
    -> Result<RuleFeedbackFact, FlowError>;
}
/// Real observation or projection updates; no inferred tool/syscall association.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum ChatEvent {
    /// Exact public protocol evidence.
    Observation(Arc<ProtocolObservation>),
    /// Actual credential executor stage.
    CredentialUse(Arc<CredentialUseFact>),
    /// Actual fixed rule-denial feedback progress.
    RuleFeedback(RuleFeedbackFact),
    /// New actual message projection.
    Started(Arc<ChatMessage>),
    /// Revision-bound incremental update.
    Delta {
        /// Message identity.
        id: ChatMessageId,
        /// Required prior revision.
        #[serde(with = "crate::decimal")]
        base_revision: u64,
        /// New revision.
        #[serde(with = "crate::decimal")]
        revision: u64,
        /// New observations.
        changes: Vec<ChatDelta>,
    },
    /// Terminal projection, replacing prior revision.
    Finished(Arc<ChatMessage>),
    /// Actual upstream/transport failure.
    ExchangeFailed {
        /// Failed actual exchange.
        exchange: ExchangeKey,
        /// Actual HTTP status, if received.
        http_status: Option<u16>,
        /// Safe finite error code.
        reason: String,
    },
    /// Explicit missing evidence/events.
    Gap(ObservationGap),
    /// Actual runtime status.
    Status(FlowStatus),
}
/// One real-time fact with its source event identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatEnvelope {
    /// Delivery identity.
    pub cursor: ChatCursor,
    /// Actual event.
    pub event: Arc<ChatEvent>,
}
/// Finite read errors with no body or secret dump.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FlowError {
    /// Requested transport shape is outside the implemented ingress range.
    #[error("unsupported")]
    Unsupported,
    /// Source not ready.
    #[error("unavailable")]
    Unavailable,
    /// Invalid filter or identity.
    #[error("invalid_input")]
    InvalidInput,
    /// This denial or call is already bound to a different counterpart.
    #[error("conflict")]
    Conflict,
    /// Configured capacity exhausted.
    #[error("busy")]
    Busy,
    /// Runtime stopped.
    #[error("stopped")]
    Stopped,
    /// Actual internal failure.
    #[error("failed")]
    Failed,
}
/// Actual current transport/parser state; no historical lookup, subscription or replay API.
pub trait FlowStatusPort: Send + Sync {
    /// Actual runtime state.
    fn status(&self) -> FlowStatus;
}

/// Typed event prepared once with a reachable-data charge, without JSON encoding.
pub struct PreparedChatEvent {
    event: Arc<ChatEvent>,
    charged_bytes: u32,
}
impl PreparedChatEvent {
    /// Charge the inline record and owned data before sharing the immutable event.
    pub fn new(event: ChatEvent) -> Result<Self, FlowError> {
        let event = Arc::new(event);
        let size = std::mem::size_of::<ChatEnvelope>().saturating_add(event.data_bytes());
        let charged_bytes = u32::try_from(size).map_err(|_| FlowError::Busy)?;
        Ok(Self {
            event,
            charged_bytes,
        })
    }
    /// Inspect the same typed event; never expose an encoded substitute.
    pub fn event(&self) -> &ChatEvent {
        &self.event
    }
    /// Reachable in-memory data charge, excluding allocator bookkeeping; not a wire/RSS bound.
    pub fn charged_bytes(&self) -> usize {
        self.charged_bytes as usize
    }
    /// Attach the actual cursor while preserving the single immutable event allocation.
    pub fn into_envelope(self, cursor: ChatCursor) -> PreparedChatEnvelope {
        PreparedChatEnvelope {
            envelope: Arc::new(ChatEnvelope {
                cursor,
                event: self.event,
            }),
            charged_bytes: self.charged_bytes,
        }
    }
}
/// One immutable queue payload with a precomputed byte charge.
#[derive(Clone)]
pub struct PreparedChatEnvelope {
    envelope: Arc<ChatEnvelope>,
    charged_bytes: u32,
}
impl PreparedChatEnvelope {
    /// Actual public envelope; no encoding or deep copy.
    pub fn envelope(&self) -> &Arc<ChatEnvelope> {
        &self.envelope
    }
    /// In-memory data charge computed before queue admission.
    pub fn charged_bytes(&self) -> usize {
        self.charged_bytes as usize
    }
}
/// One pure bounded FIFO. Waiting producers retain their own records outside this container.
pub struct ChatSender {
    queue: Arc<ChatQueue>,
}
/// The unique FIFO consumer; pop removes its charge before returning ownership.
pub struct ChatReceiver {
    queue: Arc<ChatQueue>,
}
struct ChatQueue {
    capacity: u32,
    state: std::sync::Mutex<ChatQueueState>,
    readable: tokio::sync::Notify,
    writable: tokio::sync::Notify,
}
struct ChatQueueState {
    records: std::collections::VecDeque<(Arc<ChatEnvelope>, u32)>,
    bytes: u32,
    senders: usize,
    receiver_open: bool,
}
impl ChatQueue {
    // This lock protects only primitive counters and immutable DTO queue operations.
    // It never calls user code, serializes, performs I/O or waits while held.
    fn state(&self) -> std::sync::MutexGuard<'_, ChatQueueState> {
        match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}
impl Clone for ChatSender {
    fn clone(&self) -> Self {
        let queue = self.queue.clone();
        {
            let mut state = queue.state();
            state.senders += 1;
        }
        Self { queue }
    }
}
impl Drop for ChatSender {
    fn drop(&mut self) {
        let last = {
            let mut state = self.queue.state();
            state.senders -= 1;
            state.senders == 0
        };
        if last {
            self.queue.readable.notify_waiters();
        }
    }
}
impl Drop for ChatReceiver {
    fn drop(&mut self) {
        {
            let mut state = self.queue.state();
            state.receiver_open = false;
            state.records.clear();
            state.bytes = 0;
        }
        self.queue.writable.notify_waiters();
    }
}
impl ChatSender {
    /// Create one FIFO with an in-memory data charge, not a serialized-size or RSS limit.
    pub fn channel(bytes: u32) -> (Self, ChatReceiver) {
        let queue = Arc::new(ChatQueue {
            capacity: bytes,
            state: std::sync::Mutex::new(ChatQueueState {
                records: std::collections::VecDeque::new(),
                bytes: 0,
                senders: 1,
                receiver_open: true,
            }),
            readable: tokio::sync::Notify::new(),
            writable: tokio::sync::Notify::new(),
        });
        (
            Self {
                queue: queue.clone(),
            },
            ChatReceiver { queue },
        )
    }
    /// Whether the unique consumer stopped admitting records.
    pub fn is_closed(&self) -> bool {
        !self.queue.state().receiver_open
    }
    /// Actual fixed data capacity of this container.
    pub fn capacity_bytes(&self) -> u32 {
        self.queue.capacity
    }
    /// Actual queued data; waiters and dequeued records are excluded.
    pub fn queued_bytes(&self) -> u32 {
        self.queue.state().bytes
    }
    /// Enqueue, waiting for actual space; the caller owns explicit execution cancellation.
    /// A single record larger than the whole container fails immediately.
    pub async fn send(&self, event: &PreparedChatEnvelope) -> Result<(), FlowError> {
        if event.charged_bytes > self.queue.capacity {
            return Err(FlowError::Busy);
        }
        loop {
            let notified = self.queue.writable.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let mut state = self.queue.state();
                if !state.receiver_open {
                    return Err(FlowError::Stopped);
                }
                if state.bytes <= self.queue.capacity - event.charged_bytes {
                    state
                        .records
                        .push_back((event.envelope.clone(), event.charged_bytes));
                    state.bytes += event.charged_bytes;
                    drop(state);
                    self.queue.readable.notify_one();
                    return Ok(());
                }
            }
            notified.await;
        }
    }
}
impl ChatReceiver {
    /// Actual queued data; records already handed to the consumer are excluded.
    pub fn queued_bytes(&self) -> u32 {
        self.queue.state().bytes
    }
    /// Stop admission while preserving queued records for an explicit drain.
    pub fn close(&mut self) {
        {
            self.queue.state().receiver_open = false;
        }
        self.queue.writable.notify_waiters();
    }
    /// Consume one record exactly once, releasing its charge before returning it.
    pub async fn recv(&mut self) -> Option<Arc<ChatEnvelope>> {
        let queue = self.queue.clone();
        loop {
            let notified = queue.readable.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            match self.try_recv() {
                Ok(event) => return Some(event),
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => return None,
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {}
            }
            notified.await;
        }
    }
    /// Nonblocking consume; counters change only on actual push/pop, never while waiting.
    pub fn try_recv(
        &mut self,
    ) -> Result<Arc<ChatEnvelope>, tokio::sync::mpsc::error::TryRecvError> {
        let mut state = self.queue.state();
        if let Some((event, charge)) = state.records.pop_front() {
            state.bytes -= charge;
            drop(state);
            self.queue.writable.notify_waiters();
            return Ok(event);
        }
        if !state.receiver_open || state.senders == 0 {
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected)
        } else {
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        }
    }
}

/// Charge decoded JSON storage without formatting JSON or allocating a string.
pub use crate::flow_memory::json_data_bytes;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn secret_revision_wire_preserves_values_above_javascript_integer_precision() {
        let receipt = SecretWriteReceipt {
            secret: SecretRef([3; 16]),
            revision: u64::MAX,
        };
        let value = serde_json::to_value(&receipt).expect("encode receipt");
        assert_eq!(value["revision"], u64::MAX.to_string());
        let decoded: SecretWriteReceipt =
            serde_json::from_value(value.clone()).expect("exact decimal wire");
        assert_eq!(decoded.revision, receipt.revision);
        let mut numeric = value;
        numeric["revision"] = serde_json::json!(u64::MAX);
        assert!(serde_json::from_value::<SecretWriteReceipt>(numeric).is_err());
    }
}
