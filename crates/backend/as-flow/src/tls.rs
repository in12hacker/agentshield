//! Two-leg TLS and HTTP forwarding. Every upstream request is authorized independently.
use crate::{FlowError, Shared, observe::Observer};
use as_types::flow::*;
use bytes::Bytes;
use futures::stream;
use http::{HeaderMap, Request, Response, StatusCode, header};
use http_body_util::{BodyExt, Full, StreamBody, combinators::UnsyncBoxBody};
use hyper::{
    body::{Body as _, Frame, Incoming},
    service::service_fn,
};
use hyper_util::rt::TokioIo;
use std::{
    collections::{BTreeSet, HashMap},
    convert::Infallible,
    net::{IpAddr, SocketAddr},
    sync::Arc,
};
use tokio_rustls::{LazyConfigAcceptor, TlsConnector};

type Body = UnsyncBoxBody<Bytes, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Clone)]
struct FlowExecutor(Arc<Shared>);

impl<F> hyper::rt::Executor<F> for FlowExecutor
where
    F: Future<Output = ()> + Send + 'static,
{
    fn execute(&self, future: F) {
        self.0
            .tasks
            .spawn(self.0.force.clone().run_until_cancelled_owned(future));
    }
}

fn plain(status: StatusCode, code: &'static str) -> Response<Body> {
    let mut r = Response::new(
        Full::new(Bytes::from_static(code.as_bytes()))
            .map_err(|e: Infallible| match e {})
            .boxed_unsync(),
    );
    *r.status_mut() = status;
    r
}
fn scrub_hop(headers: &mut HeaderMap) {
    let nominated: Vec<String> = headers
        .get_all(header::CONNECTION)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(',').map(|s| s.trim().to_owned()))
        .collect();
    for h in nominated {
        headers.remove(h);
    }
    for h in [
        "connection",
        "proxy-connection",
        "keep-alive",
        "transfer-encoding",
        "upgrade",
        "proxy-authorization",
        "proxy-authenticate",
    ] {
        headers.remove(h);
    }
}
pub(crate) type Destinations = HashMap<HttpsOrigin, BTreeSet<SocketAddr>>;

// Keep the original failure projection ahead of the optional stdio write. This
// awaits the existing owner and the shared async writer, never a new log task.
pub(crate) async fn diagnosed<T, E>(
    shared: Option<&Shared>,
    exchange: Option<ExchangeKey>,
    result: Result<T, E>,
    describe: impl FnOnce(&E) -> String,
) -> Result<T, FlowError> {
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            let diagnostic = describe(&error);
            if let Some(shared) = shared {
                let projection = shared
                    .publish(ChatEvent::Gap(ObservationGap {
                        kind: "transport".into(),
                        exchange,
                        cursor_range: None,
                        reason: diagnostic.clone(),
                    }))
                    .await;
                if projection.is_ok() {
                    // The same complete diagnostic is already held by the original
                    // fact consumer. Do not delay HTTP failure on a duplicate echo.
                    return Err(FlowError::Unavailable);
                }
            }
            as_types::diagnostics::record_stderr(diagnostic).await;
            Err(FlowError::Unavailable)
        }
    }
}

/// Resolve each configured origin once before enabling ingress.
pub(crate) async fn destinations(settings: &FlowSettings) -> Result<Destinations, FlowError> {
    let mut destinations = HashMap::new();
    for route in &settings.provider_routes {
        let origin = HttpsOrigin::parse(&route.origin).map_err(|_| FlowError::InvalidInput)?;
        if destinations.contains_key(&origin) {
            continue;
        }
        let resolved = diagnosed(None, None,
            tokio::net::lookup_host((origin.ascii_host.as_str(), origin.port)).await,
            |error| format!("AS_FLOW_ERROR boottime_ns={} direction=resolution origin={origin:?} stage=resolve error={error:?}", crate::boottime()),
        ).await?;
        let addresses = resolved
            .map(|addr| match addr.ip() {
                IpAddr::V6(ip) => ip
                    .to_ipv4_mapped()
                    .map_or(addr, |ip| SocketAddr::new(ip.into(), addr.port())),
                IpAddr::V4(_) => addr,
            })
            .collect::<BTreeSet<_>>();
        if addresses.is_empty() {
            return Err(FlowError::Unsupported);
        }
        destinations.insert(origin, addresses);
    }
    Ok(destinations)
}
pub(crate) async fn connection(
    shared: Arc<Shared>,
    admitted: AdmittedConnection,
) -> Result<(), FlowError> {
    let egress = shared
        .settings
        .egress
        .as_ref()
        .ok_or(FlowError::InvalidInput)?;
    let facts = Arc::new(admitted.facts);
    if facts.config_version != shared.version || facts.connection_initiator.process.is_none() {
        return Err(FlowError::InvalidInput);
    }
    let original_peer = SocketAddr::new(facts.original_remote_ip, facts.original_remote_port);
    let upstream = admitted.upstream;
    let handshake = async {
        let hello = diagnosed(Some(shared.as_ref()), None,
            LazyConfigAcceptor::new(rustls::server::Acceptor::default(), admitted.stream).await,
            |error| format!("AS_FLOW_ERROR boottime_ns={} direction=agent channel={:?} initiator={:?} stage=client_hello error={error:?}", crate::boottime(), facts.channel, facts.connection_initiator.process),
        ).await?;
        let sni = hello
            .client_hello()
            .server_name()
            .ok_or(FlowError::InvalidInput)?
            .to_ascii_lowercase();
        let origin = HttpsOrigin {
            ascii_host: sni,
            port: original_peer.port(),
        };
        if !shared
            .destinations
            .get(&origin)
            .is_some_and(|addresses| addresses.contains(&original_peer))
        {
            return Err(FlowError::InvalidInput);
        }
        let identity = diagnosed(Some(shared.as_ref()), None,
            shared.certificates.server_identity(egress.ca, origin.clone()).await,
            |error| format!("AS_FLOW_ERROR boottime_ns={} direction=agent channel={:?} initiator={:?} stage=server_identity error={error:?}", crate::boottime(), facts.channel, facts.connection_initiator.process),
        ).await?;
        let mut resolver = rustls::server::ResolvesServerCertUsingSni::new();
        resolver
            .add(&origin.ascii_host, (*identity.certified_key).clone())
            .map_err(|_| FlowError::InvalidInput)?;
        let mut config = rustls::ServerConfig::builder_with_provider(shared.crypto.clone())
            .with_safe_default_protocol_versions()
            .map_err(|_| FlowError::Unavailable)?
            .with_no_client_auth()
            .with_cert_resolver(Arc::new(resolver));
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        config.max_early_data_size = 0;
        let tls = diagnosed(Some(shared.as_ref()), None,
            hello.into_stream(Arc::new(config)).await,
            |error| format!("AS_FLOW_ERROR boottime_ns={} direction=agent channel={:?} initiator={:?} stage=client_tls error={error:?}", crate::boottime(), facts.channel, facts.connection_initiator.process),
        ).await?;
        Ok::<_, FlowError>((tls, origin))
    };
    let (tls, origin) = handshake.await?;
    let gate = Arc::new(tokio::sync::Semaphore::new(
        egress.max_streams_per_connection as usize,
    ));
    let executor = FlowExecutor(shared.clone());
    let connection_channel = facts.channel.clone();
    let connection_initiator = facts.connection_initiator.process;
    let diagnostic_source = shared.clone();
    let svc = service_fn(move |req| {
        let shared = shared.clone();
        let facts = facts.clone();
        let origin = origin.clone();
        let upstream = upstream.clone();
        let gate = gate.clone();
        async move {
            let response = match gate.try_acquire_owned() {
                Ok(permit) => match exchange(shared, facts, origin, upstream, req, permit).await {
                    Ok(r) => r,
                    Err((status, code)) => plain(status, code),
                },
                Err(_) => plain(StatusCode::SERVICE_UNAVAILABLE, "stream_capacity"),
            };
            Ok::<_, Infallible>(response)
        }
    });
    let builder = hyper_util::server::conn::auto::Builder::new(executor);
    diagnosed(Some(diagnostic_source.as_ref()), None,
        builder.serve_connection(TokioIo::new(tls), svc).await,
        |error| format!(
                "AS_FLOW_CONNECTION boottime_ns={} direction=agent channel={connection_channel:?} initiator={connection_initiator:?} stage=serve_error error={error:?}",
                crate::boottime(),
            ),
    ).await
}
async fn exchange(
    shared: Arc<Shared>,
    facts: Arc<IngressFacts>,
    origin: HttpsOrigin,
    upstream: Arc<dyn UpstreamConnect>,
    mut req: Request<Incoming>,
    stream_permit: tokio::sync::OwnedSemaphorePermit,
) -> Result<Response<Body>, (StatusCode, &'static str)> {
    let exchange_permit = shared
        .exchanges
        .clone()
        .try_acquire_owned()
        .map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "exchange_capacity"))?;
    // Authority is checked before any secret is obtained. Absolute-form and Host must agree.
    let hosts: Vec<_> = req.headers().get_all(header::HOST).iter().collect();
    if hosts.len() > 1 {
        return Err((StatusCode::BAD_REQUEST, "duplicate_host"));
    }
    let uri_auth = req.uri().authority().map(|v| v.as_str());
    let host = hosts
        .first()
        .map(|v| v.to_str())
        .transpose()
        .map_err(|_| (StatusCode::BAD_REQUEST, "invalid_host"))?;
    let normalize = |a: &str| HttpsOrigin::parse(&format!("https://{a}"));
    if let (Some(a), Some(b)) = (uri_auth, host)
        && normalize(a).ok() != normalize(b).ok()
    {
        return Err((StatusCode::BAD_REQUEST, "authority_conflict"));
    }
    let actual = uri_auth
        .or(host)
        .ok_or((StatusCode::BAD_REQUEST, "missing_authority"))?;
    if normalize(actual).ok().as_ref() != Some(&origin) {
        return Err((StatusCode::FORBIDDEN, "origin_mismatch"));
    }
    if req.uri().scheme_str().is_some_and(|s| s != "https") {
        return Err((StatusCode::BAD_REQUEST, "invalid_scheme"));
    }
    let path = req.uri().path();
    let method = req.method().as_str();
    let mut route = shared
        .settings
        .provider_routes
        .iter()
        .find(|r| {
            r.method == method
                && r.path == path
                && HttpsOrigin::parse(&r.origin).ok().as_ref() == Some(&origin)
        })
        .cloned()
        .ok_or((StatusCode::FORBIDDEN, "unconfigured_route"))?;
    route.origin = origin.url();
    let auth = req.headers().get_all(header::AUTHORIZATION).iter().count();
    let api = req.headers().get_all("x-api-key").iter().count();
    if auth + api != 1 {
        return Err((StatusCode::UNAUTHORIZED, "ambiguous_auth"));
    }
    let (slot, phantom) = if auth == 1 {
        let value = req
            .headers()
            .get(header::AUTHORIZATION)
            .ok_or((StatusCode::UNAUTHORIZED, "missing_auth"))?
            .as_bytes();
        (
            AuthSlot::AuthorizationBearer,
            value
                .strip_prefix(b"Bearer ")
                .ok_or((StatusCode::UNAUTHORIZED, "invalid_bearer"))?
                .to_vec(),
        )
    } else {
        (
            AuthSlot::XApiKey,
            req.headers()
                .get("x-api-key")
                .ok_or((StatusCode::UNAUTHORIZED, "missing_auth"))?
                .as_bytes()
                .to_vec(),
        )
    };
    let phantom = PrivateBytes::new(phantom);
    let key = shared
        .next_exchange()
        .map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "sequence_exhausted"))?;
    // The original dial and TLS verifier finish before a real credential is obtained.
    let tcp = diagnosed(Some(shared.as_ref()), Some(key), upstream.connect().await,
        |error| format!("AS_FLOW_ERROR boottime_ns={} direction=upstream flow_instance={:?} exchange_sequence={} stage=upstream_connect error={error:?}", crate::boottime(), key.flow_instance, key.sequence),
    ).await.map_err(|_| (StatusCode::BAD_GATEWAY, "upstream_tls"))?;
    let name = rustls::pki_types::ServerName::try_from(origin.ascii_host.clone())
        .map_err(|_| (StatusCode::BAD_GATEWAY, "upstream_tls"))?;
    let tls = diagnosed(Some(shared.as_ref()), Some(key),
        TlsConnector::from(shared.upstream.clone()).connect(name, tcp).await,
        |error| format!("AS_FLOW_ERROR boottime_ns={} direction=upstream flow_instance={:?} exchange_sequence={} stage=upstream_tls error={error:?}", crate::boottime(), key.flow_instance, key.sequence),
    ).await.map_err(|_| (StatusCode::BAD_GATEWAY, "upstream_tls"))?;
    let peer = diagnosed(Some(shared.as_ref()), Some(key),
        VerifiedUpstream::from_tls(origin.clone(), tls.get_ref().1, shared.roots.clone()),
        |error| format!("AS_FLOW_ERROR boottime_ns={} direction=upstream flow_instance={:?} exchange_sequence={} stage=verified_upstream error={error:?}", crate::boottime(), key.flow_instance, key.sequence),
    ).await.map_err(|_| (StatusCode::BAD_GATEWAY, "upstream_tls"))?;
    let lease = match shared.credentials.authorize_header(CredentialUseRequest {
        exchange: key,
        route: route.id,
        source: &facts,
        method: req.method().as_str(),
        encoded_path: req.uri().path(),
        slot,
        phantom: phantom.expose(),
        peer: &peer,
    }) {
        Ok(lease) => lease,
        Err(e) => {
            shared
                .publish(ChatEvent::CredentialUse(Arc::new(CredentialUseFact {
                    id: CredentialUseId {
                        exchange: key,
                        sequence: 1,
                    },
                    revision: 1,
                    actor: facts.connection_initiator.clone(),
                    binding_ref: None,
                    destination_origin: origin.url(),
                    state: CredentialUseState::ScopeRejected,
                    auth_field: Some(slot.header_name().as_str().into()),
                    reason: Some(e.to_string()),
                    observed_boottime_ns: crate::boottime(),
                })))
                .await
                .map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "event_output_unavailable"))?;
            return Err(match e {
                CredentialError::InvalidToken
                | CredentialError::Revoked
                | CredentialError::InvalidHeader => {
                    (StatusCode::UNAUTHORIZED, "invalid_credential")
                }
                CredentialError::ScopeDenied | CredentialError::OriginDenied => {
                    (StatusCode::FORBIDDEN, "credential_scope")
                }
                _ => (StatusCode::SERVICE_UNAVAILABLE, "credential_unavailable"),
            });
        }
    };
    // Record the authentication context that will really be forwarded.
    scrub_hop(req.headers_mut());
    let use_id = lease.id;
    let binding = lease.binding;
    let auth_scope =
        crate::feedback::auth_scope(&origin, route.profile, route.id, &lease, req.headers());
    shared
        .credential_fact(
            &facts,
            use_id,
            binding,
            origin.url(),
            CredentialUseState::Prepared,
            slot,
        )
        .await
        .map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "event_output_unavailable"))?;
    let mut attempt = CredentialAttempt {
        shared: shared.clone(),
        facts: facts.clone(),
        use_id,
        binding,
        origin: origin.url(),
        submitted: false,
        slot,
    };
    lease.apply(req.headers_mut());
    shared
        .credential_fact(
            &facts,
            use_id,
            binding,
            origin.url(),
            CredentialUseState::AppliedToRequest,
            slot,
        )
        .await
        .map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "event_output_unavailable"))?;
    let request_observer = Observer::new(
        shared.clone(),
        facts.clone(),
        key,
        route.clone(),
        ProtocolSide::ClientRequest,
        Some(use_id),
        Some(auth_scope),
        req.headers(),
        None,
    );
    // Origin-form on HTTP/1, absolute authority on HTTP/2, controlled by this sender.
    let uri = format!(
        "{}{}",
        origin.url(),
        req.uri()
            .path_and_query()
            .map(|x| x.as_str())
            .unwrap_or("/")
    );
    *req.uri_mut() = uri
        .parse()
        .map_err(|_| (StatusCode::BAD_REQUEST, "invalid_target"))?;
    let (mut parts, body) = req.into_parts();
    let body = body
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
        .boxed_unsync();
    let (body, guard, length) = feedback_body(
        &shared,
        Some(&auth_scope),
        body,
        request_observer,
        &mut parts.headers,
    )
    .await
    .map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "event_output_unavailable"))?;
    let req = Request::from_parts(parts, body);
    let h2 = tls.get_ref().1.alpn_protocol() == Some(b"h2");
    let tls = crate::feedback_write::Witness::new(tls, guard, length, h2);
    let cancellations = tls.cancellations();
    let response = if h2 {
        let (mut sender, conn) =
            hyper::client::conn::http2::handshake(FlowExecutor(shared.clone()), TokioIo::new(tls))
                .await
                .map_err(|_| (StatusCode::BAD_GATEWAY, "upstream_http"))?;
        let force = shared.force.clone();
        let diagnostic_source = shared.clone();
        shared.tasks.spawn(async move {
            tokio::select! {
                _=force.cancelled()=>{},
                _=crate::feedback_write::cancelled(cancellations)=>{},
                result=conn=>{ let _terminal = diagnosed(Some(diagnostic_source.as_ref()), Some(key), result,
                    |error| format!("AS_FLOW_CONNECTION_ERROR boottime_ns={} direction=upstream flow_instance={:?} exchange_sequence={} error={error:?}", crate::boottime(), key.flow_instance, key.sequence)).await; },
            }
        });
        sender.send_request(req).await
    } else {
        let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(tls))
            .await
            .map_err(|_| (StatusCode::BAD_GATEWAY, "upstream_http"))?;
        let force = shared.force.clone();
        let diagnostic_source = shared.clone();
        shared.tasks.spawn(async move {
            tokio::select! {
                _=force.cancelled()=>{},
                _=crate::feedback_write::cancelled(cancellations)=>{},
                result=conn=>{ let _terminal = diagnosed(Some(diagnostic_source.as_ref()), Some(key), result,
                    |error| format!("AS_FLOW_CONNECTION_ERROR boottime_ns={} direction=upstream flow_instance={:?} exchange_sequence={} error={error:?}", crate::boottime(), key.flow_instance, key.sequence)).await; },
            }
        });
        sender.send_request(req).await
    }
    .map_err(|_| (StatusCode::BAD_GATEWAY, "upstream_send"))?;
    shared
        .credential_fact(
            &facts,
            use_id,
            binding,
            origin.url(),
            CredentialUseState::Submitted,
            slot,
        )
        .await
        .map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "event_output_unavailable"))?;
    attempt.submitted = true;
    let observer = Observer::new(
        shared.clone(),
        facts,
        key,
        route,
        ProtocolSide::UpstreamResponse,
        Some(use_id),
        Some(auth_scope),
        response.headers(),
        Some(response.status().as_u16()),
    );
    let (mut parts, body) = response.into_parts();
    scrub_hop(&mut parts.headers);
    Ok(Response::from_parts(
        parts,
        observe_body(
            body.map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
                .boxed_unsync(),
            observer,
            Some((exchange_permit, stream_permit)),
        ),
    ))
}
/// Buffer only when this exact authorized route has a prepared plan. Ordinary requests stream.
pub(crate) async fn feedback_body(
    shared: &Arc<Shared>,
    auth: Option<&[u8; 32]>,
    mut body: Body,
    mut observer: Observer,
    headers: &mut HeaderMap,
) -> Result<(Body, Option<crate::feedback::WriteGuard>, usize), FlowError> {
    let Some(auth) = auth else {
        return Ok((observe_body(body, observer, None), None, 0));
    };
    if !shared.has_feedback(auth) {
        return Ok((observe_body(body, observer, None), None, 0));
    }
    if headers
        .get(header::CONTENT_ENCODING)
        .is_some_and(|v| v != "identity")
    {
        observer.unmodified_submission(RuleFeedbackFailure::UnsupportedBody);
        return Ok((observe_body(body, observer, None), None, 0));
    }
    let limit = shared.settings.decode_limits.max_message_bytes as usize;
    let mut bytes = Vec::new();
    while let Some(frame) = body.frame().await {
        if let Ok(frame) = &frame
            && let Some(data) = frame.data_ref()
            && bytes.len().saturating_add(data.len()) <= limit
        {
            bytes.extend_from_slice(data);
            continue;
        }
        // Preserve original data, trailers and errors exactly if complete modification is unavailable.
        let reason = if frame.as_ref().is_ok_and(|f| f.is_trailers()) {
            RuleFeedbackFailure::UnsupportedBody
        } else if frame.is_err() {
            RuleFeedbackFailure::TransportError
        } else {
            RuleFeedbackFailure::Capacity
        };
        let prefix = stream::iter(vec![Ok(Frame::data(Bytes::from(bytes))), frame]);
        let rest = http_body_util::BodyStream::new(body);
        let stream = futures::StreamExt::chain(prefix, rest);
        let body = StreamBody::new(stream).boxed_unsync();
        observer.unmodified_submission(reason);
        return Ok((observe_body(body, observer, None), None, 0));
    }
    observer.feed(&bytes).await?;
    let original = observer.finish_record(true).await?;
    if let Some(original) = original
        && let Some(mut modified) = shared.modify_feedback(original).await?
    {
        if modified.guard.cancelled() {
            modified.guard.failed(RuleFeedbackFailure::Cancelled);
        } else {
            let length = modified.bytes.len();
            headers.remove("content-md5");
            headers.remove("digest");
            headers.remove("content-digest");
            headers.remove(header::CONTENT_ENCODING);
            if let Ok(value) = http::HeaderValue::from_str(&length.to_string()) {
                headers.insert(header::CONTENT_LENGTH, value);
            }
            let body = Full::new(Bytes::from(modified.bytes))
                .map_err(|e: Infallible| match e {})
                .boxed_unsync();
            return Ok((body, Some(modified.guard), length));
        }
    }
    observer.unmodified_submission(RuleFeedbackFailure::TooLate);
    let body = Full::new(Bytes::from(bytes))
        .map_err(|e: Infallible| match e {})
        .boxed_unsync();
    Ok((body, None, 0))
}
fn observe_body(
    body: Body,
    observer: Observer,
    permits: Option<(
        tokio::sync::OwnedSemaphorePermit,
        tokio::sync::OwnedSemaphorePermit,
    )>,
) -> Body {
    let (exchange, side) = observer.transport_identity();
    let stream = stream::unfold(
        (body, Some(observer), permits),
        move |(mut body, mut observer, permits)| async move {
            if let Some(observer) = observer.as_mut() {
                observer.unmodified_submission(RuleFeedbackFailure::TooLate);
            }
            match body.frame().await {
                Some(Ok(frame)) => {
                    if let Some(data) = frame.data_ref()
                        && let Some(o) = observer.as_mut()
                        && let Err(error) = o.feed(data).await
                    {
                        return Some((
                            Err(Box::new(error) as Box<dyn std::error::Error + Send + Sync>),
                            (body, None, permits),
                        ));
                    }
                    if body.is_end_stream()
                        && let Some(o) = observer.take()
                        && let Err(error) = o.finish(true).await
                    {
                        return Some((
                            Err(Box::new(error) as Box<dyn std::error::Error + Send + Sync>),
                            (body, None, permits),
                        ));
                    }
                    Some((Ok(frame), (body, observer, permits)))
                }
                Some(Err(error)) => {
                    if let Some(o) = observer.take()
                        && let Err(failure) = o.finish(false).await
                    {
                        as_types::diagnostics::record_stderr(format!(
                            "AS_FLOW_ERROR boottime_ns={} exchange={exchange:?} side={side:?} stage=body_finish error={failure:?}",
                            crate::boottime()
                        )).await;
                    }
                    Some((Err(error), (body, None, permits)))
                }
                None => {
                    if let Some(o) = observer.take()
                        && let Err(error) = o.finish(true).await
                    {
                        return Some((
                            Err(Box::new(error) as Box<dyn std::error::Error + Send + Sync>),
                            (body, None, permits),
                        ));
                    }
                    None
                }
            }
        },
    );
    StreamBody::new(stream).boxed_unsync()
}

struct CredentialAttempt {
    shared: Arc<Shared>,
    facts: Arc<IngressFacts>,
    use_id: CredentialUseId,
    binding: BindingId,
    origin: String,
    submitted: bool,
    slot: AuthSlot,
}
impl Drop for CredentialAttempt {
    fn drop(&mut self) {
        if self.submitted {
            return;
        }
        let shared = self.shared.clone();
        let facts = self.facts.clone();
        let id = self.use_id;
        let binding = self.binding;
        let origin = self.origin.clone();
        let slot = self.slot;
        self.shared.tasks.spawn(async move {
            let _ = shared
                .credential_fact(
                    &facts,
                    id,
                    binding,
                    origin,
                    CredentialUseState::Failed,
                    slot,
                )
                .await;
        });
    }
}
