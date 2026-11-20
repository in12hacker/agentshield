use super::*;
use as_credentials::{CredentialInit, CredentialModule};
use as_types::{dispatch_event::ProcessInfo, process_tree::ProcessKey, workload::Workload};
use bytes::Bytes;
use http_body_util::{BodyExt, Full, StreamBody};
use hyper::{
    Request, Response,
    body::{Frame, Incoming},
    service::service_fn,
};
use hyper_util::rt::{TokioExecutor, TokioIo};
use std::{
    convert::Infallible,
    net::{IpAddr, Ipv4Addr},
    sync::atomic::AtomicUsize,
};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::{TlsAcceptor, TlsConnector};

struct LocalUpstream(std::net::SocketAddr);
#[async_trait::async_trait]
impl UpstreamConnect for LocalUpstream {
    async fn connect(&self) -> std::io::Result<std::pin::Pin<Box<dyn FlowIo>>> {
        Ok(Box::pin(TcpStream::connect(self.0).await?))
    }
}

fn process() -> ProcessKey {
    ProcessKey {
        boot_id: [1; 16],
        host_tgid: 123,
        birth_boottime_ns: 1,
    }
}
fn facts(version: ConfigVersion, port: u16) -> IngressFacts {
    IngressFacts {
        connection_initiator: ActorFacts {
            process: Some(process()),
            info: ProcessInfo {
                workload: Some(Workload::Host),
                ..ProcessInfo::default()
            },
            tid: Some(123),
            thread_birth_boottime_ns: Some(1),
            cgroup_id: None,
            observed_boottime_ns: 1,
        },
        channel: ChannelRef {
            kind: ChannelKind::Socket,
            source_instance: [2; 16],
            object_id: 7,
            generation: 1,
        },
        original_remote_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
        original_remote_port: port,
        plan_generation: 1,
        config_version: version,
    }
}
async fn setup(
    h2: bool,
    trusted: bool,
) -> (
    FlowModule,
    CredentialModule,
    PrivateBytes,
    tokio_rustls::client::TlsStream<TcpStream>,
    Arc<AtomicUsize>,
    tokio::task::JoinHandle<()>,
    tempfile::TempDir,
    mpsc::Sender<AdmittedConnection>,
    ChatReceiver,
) {
    setup_profile(h2, trusted, ProviderProfile::OpenAiChatCompletions).await
}
async fn setup_profile(
    h2: bool,
    trusted: bool,
    profile: ProviderProfile,
) -> (
    FlowModule,
    CredentialModule,
    PrivateBytes,
    tokio_rustls::client::TlsStream<TcpStream>,
    Arc<AtomicUsize>,
    tokio::task::JoinHandle<()>,
    tempfile::TempDir,
    mpsc::Sender<AdmittedConnection>,
    ChatReceiver,
) {
    let temp = tempfile::tempdir().expect("temp");
    std::fs::set_permissions(
        temp.path(),
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .expect("private root");
    let version = ConfigVersion::from_digest([1; 32]);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("upstream");
    let port = listener.local_addr().expect("address").port();
    let route = ProviderRoute {
        id: RouteId([4; 16]),
        origin: format!("https://localhost:{port}"),
        method: "POST".into(),
        path: if profile == ProviderProfile::OpenAiResponses {
            "/v1/responses".into()
        } else {
            "/v1/chat/completions".into()
        },
        profile,
        session: None,
    };
    let credentials = CredentialModule::initialize(CredentialInit {
        protected_root: temp.path().to_path_buf(),
        store_file: temp.path().join("private/store"),
        ca_public_file: temp.path().join("private/ca.crt"),
        bindings: Arc::new(CredentialSettings::default()),
        routes: vec![route.clone()],
        config_version: version,
    })
    .await
    .expect("credentials");
    let ca = credentials.admin_port().ensure_ca().await.expect("ca");
    let secret = credentials
        .admin_port()
        .import_secret(PrivateBytes::new(b"live-secret-ABCDE".to_vec()))
        .await
        .expect("secret");
    let binding = BindingId([5; 16]);
    credentials
        .apply_settings(
            Arc::new(CredentialSettings {
                bindings: vec![CredentialBinding {
                    id: binding,
                    secret: secret.secret,
                    enabled: true,
                    subjects: vec![CredentialSubject::Process(process())],
                    routes: vec![route.id],
                    slot: AuthSlot::AuthorizationBearer,
                }],
            }),
            vec![route.clone()],
            version,
        )
        .expect("bindings");
    let phantom = credentials
        .admin_port()
        .issue_phantom(binding, version)
        .await
        .expect("phantom")
        .value;
    let upstream_cert =
        rcgen::generate_simple_self_signed(vec!["localhost".into()]).expect("upstream cert");
    let mut roots = rustls::RootCertStore::empty();
    if trusted {
        roots.add(upstream_cert.cert.der().clone()).expect("root");
    }
    let crypto = Arc::new(rustls::crypto::ring::default_provider());
    let mut server = rustls::ServerConfig::builder_with_provider(crypto.clone())
        .with_safe_default_protocol_versions()
        .expect("versions")
        .with_no_client_auth()
        .with_single_cert(
            vec![upstream_cert.cert.der().clone()],
            rustls::pki_types::PrivatePkcs8KeyDer::from(upstream_cert.key_pair.serialize_der())
                .into(),
        )
        .expect("server");
    server.alpn_protocols = vec![if h2 {
        b"h2".to_vec()
    } else {
        b"http/1.1".to_vec()
    }];
    let hits = Arc::new(AtomicUsize::new(0));
    let count = hits.clone();
    let barrier = Arc::new(tokio::sync::Notify::new());
    let upstream = tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let acceptor = TlsAcceptor::from(Arc::new(server.clone()));
            let count = count.clone();
            let barrier = barrier.clone();
            tokio::spawn(async move {
                let Ok(tls) = acceptor.accept(stream).await else {
                    return;
                };
                let svc = service_fn(move |req: Request<Incoming>| {
                    let count = count.clone();
                    let barrier = barrier.clone();
                    async move {
                        let n = count.fetch_add(1, Ordering::SeqCst);
                        if req.headers().contains_key("x-pair-barrier") {
                            if n.is_multiple_of(2) {
                                barrier.notified().await;
                            } else {
                                barrier.notify_one();
                            }
                        }
                        assert_eq!(req.headers()["authorization"], "Bearer live-secret-ABCDE");
                        assert!(!req.headers().contains_key("x-api-key"));
                        let body = req.into_body().collect().await.expect("request").to_bytes();
                        if profile == ProviderProfile::OpenAiResponses {
                            let request: serde_json::Value =
                                serde_json::from_slice(&body).expect("real upstream JSON");
                            let has_tool_output = request
                                .get("input")
                                .and_then(serde_json::Value::as_array)
                                .is_some_and(|items| {
                                    items.iter().any(|item| {
                                        item.get("type").and_then(serde_json::Value::as_str)
                                            == Some("function_call_output")
                                    })
                                });
                            let output = if !has_tool_output {
                                serde_json::json!({"id":"resp_feedback","status":"completed","error":null,"output":[{"type":"function_call","id":"fc_feedback","call_id":"call_feedback","name":"fixture_tool","arguments":"{}"}]})
                            } else {
                                serde_json::json!({"id":"resp_result","status":"completed","output":[],"received":request})
                            };
                            let data = serde_json::to_vec(&output).expect("provider JSON");
                            let frames = vec![Ok::<_, Infallible>(Frame::data(Bytes::from(data)))];
                            let mut response =
                                Response::new(StreamBody::new(futures::stream::iter(frames)));
                            response.headers_mut().insert(
                                "content-type",
                                http::HeaderValue::from_static("application/json"),
                            );
                            return Ok::<_, Infallible>(response);
                        }
                        assert!(
                            serde_json::from_slice::<serde_json::Value>(&body)
                                .unwrap()
                                .get("messages")
                                .is_some_and(|v| v.is_array())
                        );
                        let chunks=[b"data: {\"id\":\"r1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hello \"}}]}\r\n\r\n".as_slice(),b"data: {\"id\":\"r1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"world\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n".as_slice()];
                        let frames: Vec<_> = chunks
                            .into_iter()
                            .map(|c| Ok::<_, Infallible>(Frame::data(Bytes::copy_from_slice(c))))
                            .collect();
                        let stream = futures::stream::iter(frames);
                        let mut response = Response::new(StreamBody::new(stream));
                        response.headers_mut().insert(
                            "content-type",
                            http::HeaderValue::from_static("text/event-stream"),
                        );
                        Ok::<_, Infallible>(response)
                    }
                });
                let _ = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
                    .serve_connection(TokioIo::new(tls), svc)
                    .await;
            });
        }
    });
    let settings = FlowSettings {
        enabled: true,
        provider_routes: vec![route],
        egress: Some(TlsEgressSettings {
            ca: ca.id,
            max_connections: 4,
            max_streams_per_connection: 8,
        }),
        ..FlowSettings::default()
    };
    let (tx, rx) = mpsc::channel(4);
    let (output, events) = ChatSender::channel(settings.event_queue_bytes as u32);
    let (flow, _) = FlowModule::with_roots(
        FlowInit {
            output,
            settings: Arc::new(settings),
            config_version: version,
            ingress_rx: rx,
            credentials: credentials.use_port(),
            certificates: credentials.certificate_port(),
            feedback_capacity: std::num::NonZeroUsize::new(8).expect("capacity"),
        },
        roots,
    )
    .await
    .expect("flow");
    let ingress = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("ingress fixture");
    let addr = ingress.local_addr().expect("addr");
    let client = TcpStream::connect(addr).await.expect("client");
    let (server, _) = ingress.accept().await.expect("accepted");
    tx.send(AdmittedConnection {
        stream: Box::pin(server),
        facts: facts(version, port),
        upstream: Arc::new(LocalUpstream((Ipv4Addr::LOCALHOST, port).into())),
    })
    .await
    .expect("source");
    let mut client_roots = rustls::RootCertStore::empty();
    client_roots
        .add(rustls::pki_types::CertificateDer::from(ca.certificate_der))
        .expect("client root");
    let mut client_config = rustls::ClientConfig::builder_with_provider(crypto)
        .with_safe_default_protocol_versions()
        .expect("versions")
        .with_root_certificates(client_roots)
        .with_no_client_auth();
    client_config.alpn_protocols = vec![if h2 {
        b"h2".to_vec()
    } else {
        b"http/1.1".to_vec()
    }];
    let tls = TlsConnector::from(Arc::new(client_config))
        .connect(
            rustls::pki_types::ServerName::try_from("localhost").expect("name"),
            client,
        )
        .await
        .expect("front tls");
    (
        flow,
        credentials,
        phantom,
        tls,
        hits,
        upstream,
        temp,
        tx,
        events,
    )
}

#[tokio::test]
async fn real_two_leg_tls_http1_and_http2_phantom_and_observations() {
    for h2 in [false, true] {
        let (flow, _creds, phantom, tls, hits, upstream, _temp, _source, output) =
            setup(h2, true).await;
        let port = flow.shared.settings.provider_routes[0].origin.clone();
        let mut subscription = output;
        let events = tokio::spawn(async move {
            let mut out = Vec::new();
            while let Some(event) = subscription.recv().await {
                out.push(event);
            }
            out
        });
        let request = Request::builder()
            .method("POST")
            .uri(format!("{port}/v1/chat/completions"))
            .header("host", port.strip_prefix("https://").expect("origin"))
            .header(
                "authorization",
                format!(
                    "Bearer {}",
                    std::str::from_utf8(phantom.expose()).expect("token")
                ),
            )
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from_static(
                br#"{"messages":[{"role":"user","content":"greetme"}],"stream":true}"#,
            )))
            .expect("request");
        let response = if h2 {
            let (mut send, conn) =
                hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(tls))
                    .await
                    .expect("h2");
            tokio::spawn(async move {
                let _ = conn.await;
            });
            send.send_request(request).await.expect("response")
        } else {
            let (mut send, conn) = hyper::client::conn::http1::handshake(TokioIo::new(tls))
                .await
                .expect("h1");
            tokio::spawn(async move {
                let _ = conn.await;
            });
            send.send_request(request).await.expect("response")
        };
        assert_eq!(response.status(), 200);
        let raw = response
            .into_body()
            .collect()
            .await
            .expect("response body")
            .to_bytes();
        assert!(raw.windows(5).any(|s| s == b"hello"));
        flow.stop().await.expect("actual Flow retirement");
        let events = events.await.expect("events");
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        assert!(events.iter().any(|e|matches!(e.event.as_ref(),ChatEvent::Observation(o) if o.source.side==ProtocolSide::ClientRequest)));
        assert!(events.iter().any(|e|matches!(e.event.as_ref(),ChatEvent::Finished(m) if m.observed_parts.iter().any(|p|matches!(p,ChatPart::Text{text,..}if text=="hello world")))));
        let encoded = serde_json::to_vec(&events).expect("public events");
        assert!(!encoded.windows(17).any(|w| w == b"live-secret-ABCDE"));
        assert!(
            !encoded
                .windows(phantom.expose().len())
                .any(|w| w == phantom.expose())
        );
        upstream.abort();
    }
}
#[test]
fn provider_boundaries_and_sse_chunking() {
    let bytes = b": heartbeat\r\ndata: {\"ok\":\r\ndata: true}\r\n\r\n";
    let mut parser = protocol::Sse::new(1024);
    let mut units = Vec::new();
    for byte in bytes {
        units.extend(
            parser
                .feed(&[*byte])
                .into_iter()
                .map(|unit| unit.expect("sse")),
        );
    }
    assert_eq!(units.len(), 1);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&units[0].data).expect("json"),
        serde_json::json!({"ok":true})
    );
    let mut codec = protocol::Codec::new(ProviderProfile::OpenAiResponses);
    codec.response(&serde_json::json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"delta":"ab"}),None,true,&[]);
    codec.response(&serde_json::json!({"type":"response.output_text.done","output_index":0,"content_index":0,"text":"ab"}),None,true,&[]);
    assert!(
        matches!(codec.items["output-0"].parts.get(&0), Some(ChatPart::Text { text, .. }) if text == "ab")
    );
    let mut codec = protocol::Codec::new(ProviderProfile::AnthropicMessages);
    codec.response(&serde_json::json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"tool-1","name":"exec","input":{}}}),None,true,&[]);
    codec.response(&serde_json::json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"a\":1}"}}),None,true,&[]);
    assert!(matches!(
        codec.items["message"].parts.get(&0),
        Some(ChatPart::ToolCall { arguments: Some(ChatContent::Json(value)), .. }) if value == &serde_json::json!({"a":1})
    ));
}

#[tokio::test]
async fn rejects_an_admitted_peer_outside_the_sni_origins_addresses() {
    let (flow, _creds, _token, _tls, hits, upstream, _temp, _source, _events) =
        setup(false, true).await;
    let origin =
        HttpsOrigin::parse(&flow.shared.settings.provider_routes[0].origin).expect("origin");
    let mut facts = facts(flow.shared.version, origin.port);
    facts.original_remote_ip = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1));
    let (client, server) = tokio::io::duplex(4096);
    let connector = TlsConnector::from(flow.shared.upstream.clone());
    let (result, handshake) = tokio::join!(
        tls::connection(
            flow.shared.clone(),
            AdmittedConnection {
                stream: Box::pin(server),
                facts,
                upstream: Arc::new(LocalUpstream((Ipv4Addr::LOCALHOST, origin.port).into())),
            },
        ),
        connector.connect(
            rustls::pki_types::ServerName::try_from("localhost").expect("name"),
            client
        ),
    );
    assert!(matches!(result, Err(FlowError::InvalidInput)));
    assert!(handshake.is_err());
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    upstream.abort();
    flow.stop().await.expect("stop flow");
}

#[tokio::test]
async fn invalid_token_route_duplicate_auth_and_untrusted_upstream_never_send() {
    for case in ["token", "route", "duplicate", "certificate"] {
        let (flow, _creds, phantom, tls, hits, upstream, _temp, _source, _output) =
            setup(false, case != "certificate").await;
        let origin = flow.shared.settings.provider_routes[0].origin.clone();
        let (mut send, conn) = hyper::client::conn::http1::handshake(TokioIo::new(tls))
            .await
            .expect("http");
        let driver = tokio::spawn(async move {
            let _ = conn.await;
        });
        let token = if case == "token" {
            "as_ph_invalid"
        } else {
            std::str::from_utf8(phantom.expose()).expect("utf8")
        };
        let path = if case == "route" {
            "/unconfigured"
        } else {
            "/v1/chat/completions"
        };
        let mut request = Request::builder()
            .method("POST")
            .uri(format!("{origin}{path}"))
            .header("host", origin.strip_prefix("https://").expect("origin"))
            .header("authorization", format!("Bearer {token}"));
        if case == "duplicate" {
            request = request.header("authorization", "Bearer second");
        }
        let request = request
            .body(Full::new(Bytes::from_static(
                br#"{"messages":[{"role":"user","content":"greetme"}]}"#,
            )))
            .expect("request");
        let response = send.send_request(request).await.expect("local denial");
        assert_eq!(
            response.status().as_u16(),
            match case {
                "route" => 403,
                "certificate" => 502,
                _ => 401,
            },
            "{case}"
        );
        let body = response
            .into_body()
            .collect()
            .await
            .expect("local error")
            .to_bytes();
        assert!(
            !body
                .windows(phantom.expose().len())
                .any(|v| v == phantom.expose())
        );
        assert_eq!(hits.load(Ordering::SeqCst), 0, "{case}");
        drop(send);
        driver.await.expect("driver");
        upstream.abort();
        flow.stop().await.expect("stop");
    }
}
#[test]
fn compression_complete_and_truncated_streams() {
    use std::io::Write;
    let raw = br#"{"messages":[{"role":"user","content":"hello"}]}"#;
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gzip.write_all(raw).expect("compress");
    let gzip = gzip.finish().expect("gzip");
    let mut brotli = Vec::new();
    {
        let mut w = brotli::CompressorWriter::new(&mut brotli, 4096, 5, 22);
        w.write_all(raw).expect("br");
    }
    for (encoding, encoded) in [("gzip", gzip), ("br", brotli)] {
        let mut decoder = content::Content::new(encoding, 1024).expect("decoder");
        let mut decoded = Vec::new();
        for part in encoded.chunks(3) {
            decoded.extend(decoder.feed(part).expect("decode"));
        }
        decoded.extend(decoder.finish().expect("complete"));
        assert_eq!(decoded, raw);
        let mut decoder = content::Content::new(encoding, 1024).expect("decoder");
        let result = decoder.feed(&encoded[..encoded.len() - 3]);
        assert!(
            result.is_err() || decoder.finish().is_err(),
            "{encoding} truncated input must not be complete"
        );
        let mut decoder = content::Content::new(encoding, 4).expect("limited");
        assert!(
            decoder.feed(&encoded).is_err() || decoder.finish().is_err(),
            "{encoding} output bound"
        );
    }
}
#[test]
fn responses_preserve_status_without_inventing_finish_reason_or_error() {
    for (status, state) in [
        ("completed", ChatState::Complete),
        ("incomplete", ChatState::Interrupted),
        ("failed", ChatState::Failed),
    ] {
        for streaming in [false, true] {
            let response = serde_json::json!({"object":"response","id":"response-one","status":status,"error":null,"model":"model-real","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"partial"}]}]});
            let input = if streaming {
                serde_json::json!({"type":format!("response.{status}"),"response":response})
            } else {
                response
            };
            let mut codec = protocol::Codec::new(ProviderProfile::OpenAiResponses);
            codec.response(&input, None, streaming, &[]);
            let items = &codec.items;
            assert_eq!(items[0].state, state);
            assert_eq!(items[0].provider_status.as_deref(), Some(status));
            assert_eq!(
                items[0].finish, None,
                "provider status is not finish_reason"
            );
        }
    }
    let input = serde_json::json!({"error":{"code":"server_error","message":"fixture"}});
    let mut codec = protocol::Codec::new(ProviderProfile::OpenAiResponses);
    codec.response(&input, None, false, &[]);
    assert!(codec.items.is_empty());
    assert!(codec.provider_failed);
}

// Delivery-only software fixture: these ports reject every real TLS/credential operation.
struct NoTransport;
impl CredentialPort for NoTransport {
    fn authorize_header(
        &self,
        _: CredentialUseRequest<'_>,
    ) -> Result<AuthHeaderLease, CredentialError> {
        Err(CredentialError::Stopped)
    }
}
#[async_trait::async_trait]
impl CertificatePort for NoTransport {
    async fn server_identity(
        &self,
        _: CaRef,
        _: HttpsOrigin,
    ) -> Result<TlsServerIdentity, CredentialError> {
        Err(CredentialError::Stopped)
    }
    fn public_ca(&self, _: CaRef) -> Result<CaPublicInfo, CredentialError> {
        Err(CredentialError::Stopped)
    }
}
pub(super) fn protocol_fixture() -> (Arc<Shared>, ChatReceiver) {
    let settings = FlowSettings::default();
    let (output, receiver) = ChatSender::channel(settings.event_queue_bytes as u32);
    let crypto = Arc::new(rustls::crypto::ring::default_provider());
    let roots = Arc::new(rustls::RootCertStore::empty());
    let upstream = rustls::ClientConfig::builder_with_provider(crypto.clone())
        .with_safe_default_protocol_versions()
        .expect("versions")
        .with_root_certificates((*roots).clone())
        .with_no_client_auth();
    (
        Arc::new(Shared {
            settings: Arc::new(settings),
            destinations: Default::default(),
            version: ConfigVersion::from_digest([1; 32]),
            instance: [9; 16],
            credentials: Arc::new(NoTransport),
            certificates: Arc::new(NoTransport),
            crypto,
            upstream: Arc::new(upstream),
            roots,
            tasks: TaskTracker::new(),
            force: CancellationToken::new(),
            exchanges: Arc::new(Semaphore::new(128)),
            intake: AtomicU8::new(0),
            next: AtomicU64::new(0),
            event_sequence: AtomicU64::new(0),
            lost: AtomicU64::new(0),
            parse_failures: AtomicU64::new(0),
            feedback: Mutex::new(feedback::Feedback::new(
                std::num::NonZeroUsize::new(8).expect("capacity"),
            )),
            feedback_stop: CancellationToken::new(),
            output: Mutex::new(Some(output)),
        }),
        receiver,
    )
}
fn sample_message() -> ChatMessage {
    let key = ExchangeKey {
        flow_instance: [9; 16],
        sequence: 1,
    };
    let original = facts(ConfigVersion::from_digest([1; 32]), 443);
    ChatMessage {
        id: ChatMessageId {
            exchange: key,
            sequence: 1,
        },
        revision: 1,
        source: Arc::new(ProtocolSource {
            actor: original.connection_initiator,
            channel: Some(original.channel),
            side: ProtocolSide::UpstreamResponse,
            request_traceparent: None,
            profile: ProviderProfile::OpenAiResponses,
            origin: "https://example.test".into(),
            route_label: "software".into(),
            config_version: original.config_version,
            observed_at_utc_ms: None,
            credential_use: None,
            actual_auth_scope: None,
        }),
        record_refs: Vec::new(),
        origin: ChatOrigin::ResponseOutput,
        role: ChatRole::Assistant,
        candidate_index: None,
        model: None,
        observed_parts: vec![ChatPart::Text {
            id: 0,
            kind: TextKind::Plain,
            text: "retained".into(),
        }],
        state: ChatState::Complete,
        quality: Vec::new(),
        provider_status: Some("completed".into()),
        finish_reason: None,
        usage: None,
        first_seen_boottime_ns: 1,
        last_seen_boottime_ns: 1,
    }
}

#[tokio::test]
async fn failed_provider_is_observed_without_syntax_failure_or_synthetic_message() {
    let (shared, mut subscription) = protocol_fixture();
    let e = shared.next_exchange().expect("exchange");
    let route = ProviderRoute {
        id: RouteId([1; 16]),
        origin: "https://example.test".into(),
        method: "POST".into(),
        path: "/v1/responses".into(),
        profile: ProviderProfile::OpenAiResponses,
        session: None,
    };
    let mut observer = observe::Observer::new(
        shared.clone(),
        Arc::new(facts(shared.version, 443)),
        e,
        route,
        ProtocolSide::UpstreamResponse,
        None,
        None,
        &http::HeaderMap::new(),
        Some(200),
    );
    observer.feed(br#"{"object":"response","status":"failed","error":{"code":"server_error","message":"fixture"},"output":[]}"#).await.expect("protocol output");
    observer.finish(true).await.expect("protocol output");
    let mut raw = false;
    let mut failure = false;
    while let Ok(event) = subscription.try_recv() {
        match event.event.as_ref() {
            ChatEvent::Observation(_) => raw = true,
            ChatEvent::ExchangeFailed {
                http_status: Some(200),
                ..
            } => failure = true,
            ChatEvent::Started(_) | ChatEvent::Finished(_) => {
                panic!("empty provider failure fabricated a message")
            }
            _ => {}
        }
    }
    assert!(raw && failure);
    assert_eq!(shared.parse_failures.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn observer_drop_and_explicit_finish_settle_the_same_message_once() {
    for mode in ["drop", "finish_interrupted", "finish_complete"] {
        let (shared, mut events) = protocol_fixture();
        let exchange = shared.next_exchange().expect("exchange");
        let route = ProviderRoute {
            id: RouteId([7; 16]),
            origin: "https://example.test".into(),
            method: "POST".into(),
            path: "/v1/chat/completions".into(),
            profile: ProviderProfile::OpenAiChatCompletions,
            session: None,
        };
        let mut headers = http::HeaderMap::new();
        headers.insert(
            "content-type",
            http::HeaderValue::from_static("text/event-stream"),
        );
        let mut observer = observe::Observer::new(
            shared.clone(),
            Arc::new(facts(shared.version, 443)),
            exchange,
            route,
            ProtocolSide::UpstreamResponse,
            None,
            None,
            &headers,
            Some(200),
        );
        observer.feed(b"data: {\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"partial\"}}]}\n\n").await.expect("protocol output");
        match mode {
            "drop" => drop(observer),
            "finish_interrupted" => observer.finish(false).await.expect("protocol output"),
            _ => {
                observer
                    .feed(b"data: [DONE]\n\n")
                    .await
                    .expect("protocol output");
                observer.finish(true).await.expect("protocol output");
            }
        }
        shared.tasks.close();
        shared.tasks.wait().await;
        let mut started = None;
        let mut finished = Vec::new();
        let mut dropped_gaps = 0;
        while let Ok(event) = events.try_recv() {
            match event.event.as_ref() {
                ChatEvent::Started(message) => started = Some(message.clone()),
                ChatEvent::Finished(message) => finished.push(message.clone()),
                ChatEvent::Gap(gap) if gap.reason == "body_dropped_before_eof" => dropped_gaps += 1,
                _ => {}
            }
        }
        let started = started.expect("actual streamed start");
        assert_eq!(finished.len(), 1, "{mode}: exactly one terminal event");
        assert_eq!(finished[0].id, started.id);
        assert_eq!(finished[0].revision, started.revision + 1);
        assert_eq!(finished[0].observed_parts, started.observed_parts);
        assert_eq!(
            finished[0].state,
            if mode == "finish_complete" {
                ChatState::Complete
            } else {
                ChatState::Interrupted
            }
        );
        assert_eq!(dropped_gaps, usize::from(mode == "drop"));
    }
}

#[tokio::test]
async fn cancelled_http_finish_keeps_the_terminal_owned_until_queue_admission() {
    let (shared, mut initial) = protocol_fixture();
    let mut observer = body_observer(&shared, ProtocolSide::UpstreamResponse, true);
    observer
        .feed(b"data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"partial\"}}]}\n\n")
        .await
        .expect("published start");
    let mut started = None;
    while let Ok(event) = initial.try_recv() {
        if let ChatEvent::Started(message) = event.event.as_ref() {
            started = Some(message.id);
        }
    }
    let started = started.expect("original message");

    let filler = ChatEvent::Gap(ObservationGap {
        kind: "test".into(),
        exchange: None,
        cursor_range: None,
        reason: "x".repeat(4096),
    });
    let charge = PreparedChatEvent::new(filler.clone())
        .expect("charge")
        .charged_bytes();
    let (sender, mut final_events) = ChatSender::channel(charge.try_into().expect("capacity"));
    shared.output.lock().expect("fixture").replace(sender);
    shared.publish(filler).await.expect("fill output");

    let mut finish = Box::pin(observer.finish(true));
    assert!(futures::poll!(&mut finish).is_pending());
    drop(finish);
    final_events.try_recv().expect("remove filler");
    shared.tasks.close();
    shared.tasks.wait().await;

    let event = final_events
        .try_recv()
        .expect("terminal survives cancelled waiter");
    assert!(matches!(event.event.as_ref(), ChatEvent::Finished(message)
        if message.id == started && message.revision == 2 && message.state == ChatState::Interrupted));
    assert!(
        final_events.try_recv().is_err(),
        "only one terminal is published"
    );
}

#[tokio::test]
async fn interrupted_http_preserves_obtained_bytes_without_projecting_complete_json() {
    let (shared, mut events) = protocol_fixture();
    let mut observer = body_observer(&shared, ProtocolSide::UpstreamResponse, false);
    let bytes = br#"{"choices":[{"index":0,"message":{"content":"valid JSON prefix"},"finish_reason":"stop"}]}"#;
    observer.feed(bytes).await.expect("body bytes");
    observer.finish(false).await.expect("interrupted transport");
    let raw = events.try_recv().expect("original prefix");
    assert!(matches!(raw.event.as_ref(), ChatEvent::Observation(o)
        if o.quality == [CaptureIssue::Gap]
            && o.body.original_length.is_none()
            && o.native_ids.is_empty()
            && matches!(o.body.segments.as_slice(), [EvidenceSegment::Bytes { offset: 0, data }] if data == bytes)));
    let gap = events.try_recv().expect("transport gap");
    assert!(matches!(gap.event.as_ref(), ChatEvent::Gap(g) if g.reason == "transport_interrupted"));
    assert!(
        events.try_recv().is_err(),
        "no projected Complete on a broken transport"
    );
}

#[tokio::test]
async fn local_body_parse_failure_keeps_the_obtained_prefix_and_triggering_input() {
    let (mut shared, mut events) = protocol_fixture();
    Arc::get_mut(&mut Arc::get_mut(&mut shared).expect("unique owner").settings)
        .expect("unique settings")
        .decode_limits
        .max_message_bytes = 8;
    let mut observer = body_observer(&shared, ProtocolSide::UpstreamResponse, false);
    observer.feed(b"{\"a\"").await.expect("first prefix");
    observer
        .feed(b":\"obtained\"}")
        .await
        .expect("original error facts");
    observer
        .finish(true)
        .await
        .expect("already retired observer");
    let raw = events.try_recv().expect("obtained bytes");
    assert!(matches!(raw.event.as_ref(), ChatEvent::Observation(o)
        if o.quality == [CaptureIssue::Gap]
            && o.body.original_length.is_none()
            && o.native_ids.is_empty()
            && matches!(o.body.segments.as_slice(), [EvidenceSegment::Bytes { offset: 0, data }]
                if data == b"{\"a\":\"obtained\"}")));
    let gap = events.try_recv().expect("actual parse gap");
    assert!(matches!(gap.event.as_ref(), ChatEvent::Gap(g) if g.reason == "json_body_too_large"));
    assert!(
        events.try_recv().is_err(),
        "no complete projection or duplicate retirement"
    );
}

#[tokio::test]
async fn cancelled_raw_publication_keeps_the_original_until_queue_admission() {
    let (shared, _) = protocol_fixture();
    let filler = ChatEvent::Gap(ObservationGap {
        kind: "test".into(),
        exchange: None,
        cursor_range: None,
        reason: "x".repeat(4096),
    });
    let charge = PreparedChatEvent::new(filler.clone())
        .expect("charge")
        .charged_bytes();
    let (sender, mut events) = ChatSender::channel(charge.try_into().expect("capacity"));
    shared.output.lock().expect("fixture").replace(sender);
    shared.publish(filler).await.expect("fill output");
    let mut observer = body_observer(&shared, ProtocolSide::UpstreamResponse, false);
    let bytes = br#"{"choices":[]}"#;
    observer.feed(bytes).await.expect("obtained body");
    let mut finish = Box::pin(observer.finish(true));
    assert!(futures::poll!(&mut finish).is_pending());
    drop(finish);
    events.try_recv().expect("release filler");
    shared.tasks.close();
    shared.tasks.wait().await;
    let raw = events
        .try_recv()
        .expect("original survives cancelled publisher");
    assert!(matches!(raw.event.as_ref(), ChatEvent::Observation(o)
        if matches!(o.body.segments.as_slice(), [EvidenceSegment::Bytes { offset: 0, data }] if data == bytes)));
    assert!(
        matches!(events.try_recv().expect("actual cancellation gap").event.as_ref(),
        ChatEvent::Gap(g) if g.reason == "body_finalization_cancelled")
    );
    assert!(
        events.try_recv().is_err(),
        "one original and one gap; no guessed projection"
    );
}

#[tokio::test]
async fn malformed_sse_keeps_preceding_frames_and_stops_after_the_first_failure() {
    let first = b"data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"accepted\"}}]}\n\n";
    for (invalid, reason, malformed_record) in [
        (b"event: \xff\n".as_slice(), "invalid_utf8", false),
        (b"data: malformed\n\n".as_slice(), "invalid_json", true),
        (
            b"data: {\"id\":\"a\",\"id\":\"b\"}\n\n".as_slice(),
            "invalid_json",
            true,
        ),
        (
            b"data: {\"metadata\":{\"session\":\"a\",\"session\":\"b\"}}\n\n".as_slice(),
            "invalid_json",
            true,
        ),
        (
            b"data: {\"metadata\":{\"session\":\"a\"},\"metadata\":{\"session\":\"b\"}}\n\n"
                .as_slice(),
            "invalid_json",
            true,
        ),
    ] {
        let (shared, mut events) = protocol_fixture();
        let mut observer = body_observer(&shared, ProtocolSide::UpstreamResponse, true);
        let batch = [first.as_slice(), invalid, b"data: [DONE]\n\n"].concat();
        observer
            .feed(&batch)
            .await
            .expect("reported capture failure");
        observer.finish(true).await.expect("retire");
        let mut raw = Vec::new();
        let mut gaps = Vec::new();
        let mut finished = Vec::new();
        while let Ok(event) = events.try_recv() {
            match event.event.as_ref() {
                ChatEvent::Observation(value) => raw.push(value.clone()),
                ChatEvent::Gap(value) => gaps.push(value.reason.clone()),
                ChatEvent::Finished(value) => finished.push(value.clone()),
                _ => {}
            }
        }
        assert_eq!(raw.len(), 1 + usize::from(malformed_record));
        assert_eq!(raw[0].body, EvidenceBody::from_bytes(first.to_vec()));
        if malformed_record {
            assert_eq!(raw[1].body, EvidenceBody::from_bytes(invalid.to_vec()));
            assert_eq!(raw[1].quality, [CaptureIssue::Gap]);
            assert!(raw[1].native_ids.is_empty());
        }
        assert_eq!(gaps, [reason]);
        assert_eq!(finished.len(), 1);
        assert_eq!(finished[0].state, ChatState::Failed);
        assert!(
            matches!(finished[0].observed_parts.as_slice(), [ChatPart::Text { text, .. }] if text == "accepted")
        );
    }
}

#[tokio::test]
async fn stop_waits_for_the_existing_consumer_to_drain_original_source_facts() {
    let (flow, _, _, client, _, upstream, _root, ingress, _old_events) = setup(true, true).await;
    drop(client);

    let filler = ChatEvent::Gap(ObservationGap {
        kind: "test".into(),
        exchange: None,
        cursor_range: None,
        reason: "x".repeat(4096),
    });
    let charge = PreparedChatEvent::new(filler.clone())
        .expect("charge")
        .charged_bytes();
    let (sender, mut consumer) = ChatSender::channel(charge.try_into().expect("capacity"));
    flow.shared.output.lock().expect("fixture").replace(sender);
    flow.shared.publish(filler).await.expect("fill output");
    let source = flow.status_port();
    drop(ingress);
    while source.status().intake_state == FlowIntakeState::Running {
        tokio::task::yield_now().await;
    }
    assert_eq!(source.status().intake_state, FlowIntakeState::SourceClosed);
    let mut stopping = Box::pin(flow.stop());
    assert!(futures::poll!(stopping.as_mut()).is_pending());
    consumer.recv().await.expect("original occupied record");
    let draining = tokio::spawn(async move {
        let mut events = Vec::new();
        while let Some(event) = consumer.recv().await {
            events.push(event);
        }
        events
    });
    stopping.await.expect("actual retirement");
    let events = draining.await.expect("original consumer exit");
    assert!(events.iter().any(|event| matches!(event.event.as_ref(),
        ChatEvent::Gap(gap) if gap.reason == "source_closed")));
    assert_eq!(source.status().intake_state, FlowIntakeState::Stopped);
    assert_eq!(source.status().active_exchanges, 0);
    upstream.abort();
    let _ = upstream.await;
}

#[path = "feedback_tests.rs"]
mod feedback_tests;

#[tokio::test]
async fn data_charge_tracks_owned_capacity_without_serializing_empty_text() {
    let mut message = sample_message();
    message.observed_parts = vec![ChatPart::Text {
        id: 0,
        kind: TextKind::Plain,
        text: String::with_capacity(256 * 1024),
    }];
    let event = PreparedChatEvent::new(ChatEvent::Started(Arc::new(message))).unwrap();
    assert!(event.charged_bytes() >= 256 * 1024);
    let ChatEvent::Started(message) = event.event() else {
        panic!("original typed event")
    };
    assert!(matches!(&message.observed_parts[0],ChatPart::Text{text,..}if text.is_empty()));
    let (queue, _) = ChatSender::channel(64 * 1024);
    assert!(matches!(
        queue
            .send(&event.into_envelope(ChatCursor {
                service_instance: [1; 16],
                sequence: 1
            }))
            .await,
        Err(FlowError::Busy)
    ));
}

#[tokio::test]
async fn concurrent_real_tls_requests_do_not_reserve_whole_exchange_capacity() {
    let (flow, _credentials, phantom, tls, hits, upstream, _temp, _source, mut output) =
        setup(true, true).await;

    let origin = flow.shared.settings.provider_routes[0].origin.clone();
    let (send, conn) =
        hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(tls))
            .await
            .unwrap();
    let driver = tokio::spawn(async move {
        let _ = conn.await;
    });
    let request = || {
        Request::builder().method("POST").uri(format!("{origin}/v1/chat/completions"))
        .header("host",origin.strip_prefix("https://").unwrap()).header("x-pair-barrier","present")
        .header("content-type","application/json")
        .header("authorization",format!("Bearer {}",std::str::from_utf8(phantom.expose()).unwrap()))
        .body(Full::new(Bytes::from_static(br#"{"messages":[{"role":"user","content":"one"},{"role":"user","content":"one"}],"stream":true}"#))).unwrap()
    };
    let mut first = send.clone();
    let mut second = send.clone();
    let pair = async {
        let (a, b) = tokio::join!(
            first.send_request(request()),
            second.send_request(request())
        );
        let a = a.unwrap();
        let b = b.unwrap();
        assert_eq!(a.status(), 200);
        assert_eq!(b.status(), 200);
        let (a, b) = tokio::join!(a.into_body().collect(), b.into_body().collect());
        a.unwrap();
        b.unwrap();
    };
    pair.await;
    assert_eq!(hits.load(Ordering::SeqCst), 2);
    let mut exchanges = Vec::new();
    let mut completed = Vec::new();
    while let Ok(envelope) = output.try_recv() {
        assert!(!matches!(envelope.event.as_ref(), ChatEvent::Gap(_)));
        if let ChatEvent::Finished(m) = envelope.event.as_ref() {
            completed.push(m.clone());
        }
        if let ChatEvent::Observation(p) = envelope.event.as_ref()
            && p.source.side == ProtocolSide::ClientRequest
        {
            exchanges.push(p.id.exchange);
        }
    }
    assert_eq!(exchanges.len(), 2);
    for key in exchanges {
        let request: Vec<_> = completed
            .iter()
            .filter(|m| m.id.exchange == key && m.origin == ChatOrigin::RequestContext)
            .collect();
        let response: Vec<_> = completed
            .iter()
            .filter(|m| m.id.exchange == key && m.origin == ChatOrigin::ResponseOutput)
            .collect();
        assert_eq!(request.len(), 2);
        assert_ne!(request[0].id, request[1].id);
        assert_eq!(request[0].observed_parts, request[1].observed_parts);
        assert_eq!(response.len(), 1);
        assert_eq!(response[0].state, ChatState::Complete);
    }
    drop(first);
    drop(second);
    drop(send);
    driver.abort();
    upstream.abort();
    flow.stop().await.unwrap();
}

#[tokio::test]
async fn realtime_queue_releases_on_dequeue_backpressures_and_closes_explicitly() {
    use futures::FutureExt;
    let event = PreparedChatEvent::new(ChatEvent::Gap(ObservationGap {
        kind: "fixture".into(),
        exchange: None,
        cursor_range: None,
        reason: "actual".into(),
    }))
    .expect("charge");
    let charge = event.charged_bytes() as u32;
    let (sender, mut receiver) = ChatSender::channel(charge * 2 - 1);
    let first = event.into_envelope(ChatCursor {
        service_instance: [1; 16],
        sequence: 1,
    });
    sender.send(&first).await.expect("queue");
    assert_eq!(sender.queued_bytes(), charge);
    let second = PreparedChatEvent::new(ChatEvent::Gap(ObservationGap {
        kind: "fixture".into(),
        exchange: None,
        cursor_range: None,
        reason: "actual".into(),
    }))
    .expect("charge")
    .into_envelope(ChatCursor {
        service_instance: [1; 16],
        sequence: 2,
    });
    assert!(
        sender.send(&second).now_or_never().is_none(),
        "real bounded backpressure"
    );
    let held = receiver.recv().await.expect("actual dequeue");
    assert_eq!(
        sender.queued_bytes(),
        0,
        "consumer-held Arc is not queue occupancy"
    );
    sender.send(&second).await.expect("capacity returned");
    assert_eq!(receiver.queued_bytes(), charge);
    assert_eq!(held.cursor.sequence, 1);
    drop(held);
    let waiting_sender = sender.clone();
    let waiting_event = first.clone();
    let waiting = tokio::spawn(async move { waiting_sender.send(&waiting_event).await });
    tokio::task::yield_now().await;
    assert!(!waiting.is_finished());
    assert_eq!(
        sender.queued_bytes(),
        charge,
        "waiting record is not queue occupancy despite unused partial space"
    );
    receiver.close();
    assert!(matches!(
        waiting.await.expect("sender task"),
        Err(FlowError::Stopped)
    ));
    assert_eq!(
        sender.queued_bytes(),
        charge,
        "closing does not consume buffered records"
    );
    let _ = receiver.recv().await.expect("drain already-admitted event");
    assert_eq!(sender.queued_bytes(), 0);
    assert!(matches!(sender.send(&first).await, Err(FlowError::Stopped)));
    let (small, _receiver) = ChatSender::channel(charge - 1);
    assert!(
        matches!(
            small.send(&first).now_or_never(),
            Some(Err(FlowError::Busy))
        ),
        "one oversized record never waits forever"
    );
    let (tx, mut rx) = ChatSender::channel(charge);
    let last = tx.clone();
    tx.send(&first)
        .await
        .expect("enqueue before final producer drop");
    drop(tx);
    let _ = rx.recv().await.expect("drain existing record");
    let end = tokio::spawn(async move { rx.recv().await });
    tokio::task::yield_now().await;
    assert!(!end.is_finished());
    drop(last);
    assert!(end.await.expect("receiver task").is_none());
    let (tx, rx) = ChatSender::channel(charge);
    tx.send(&first).await.expect("full queue");
    let waiter = tx.clone();
    let record = second.clone();
    let blocked = tokio::spawn(async move { waiter.send(&record).await });
    tokio::task::yield_now().await;
    assert!(!blocked.is_finished());
    drop(rx);
    assert!(matches!(
        blocked.await.expect("sender task"),
        Err(FlowError::Stopped)
    ));
    assert_eq!(tx.queued_bytes(), 0);
}

#[tokio::test]
async fn real_tls_closed_fact_consumer_returns_service_error_without_anonymous_fallback() {
    let (flow, _credentials, phantom, tls, hits, upstream, _temp, _source, mut output) =
        setup(false, true).await;
    output.close();
    let origin = flow.shared.settings.provider_routes[0].origin.clone();
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(tls))
        .await
        .expect("client HTTP");
    let driver = tokio::spawn(connection);
    let request = Request::builder()
        .method("POST")
        .uri(format!("{origin}/v1/chat/completions"))
        .header("host", origin.strip_prefix("https://").expect("host"))
        .header(
            "authorization",
            format!(
                "Bearer {}",
                std::str::from_utf8(phantom.expose()).expect("phantom")
            ),
        )
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from_static(
            br#"{"messages":[{"role":"user","content":"greetme"}],"stream":true}"#,
        )))
        .expect("request");
    let response = sender
        .send_request(request)
        .await
        .expect("explicit HTTP result");
    assert_eq!(response.status(), 503);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("finite failure body")
        .to_bytes();
    assert_eq!(body.as_ref(), b"event_output_unavailable");
    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "no anonymous or unobserved upstream HTTP request"
    );
    assert!(flow.status_port().status().lost_deliveries > 0);
    drop(sender);
    driver
        .await
        .expect("client task")
        .expect("client HTTP retirement");
    upstream.abort();
    let status = flow.status_port();
    flow.stop().await.expect("Flow stop");
    assert_eq!(status.status().intake_state, FlowIntakeState::Stopped);
}

fn body_observer(shared: &Arc<Shared>, side: ProtocolSide, streaming: bool) -> observe::Observer {
    let mut headers = http::HeaderMap::new();
    headers.insert(
        "content-type",
        http::HeaderValue::from_static(if streaming {
            "text/event-stream"
        } else {
            "application/json"
        }),
    );
    observe::Observer::new(
        shared.clone(),
        Arc::new(facts(shared.version, 443)),
        shared.next_exchange().expect("exchange"),
        ProviderRoute {
            id: RouteId([1; 16]),
            origin: "https://example.test".into(),
            method: "POST".into(),
            path: "/v1/chat/completions".into(),
            profile: ProviderProfile::OpenAiChatCompletions,
            session: None,
        },
        side,
        None,
        None,
        &headers,
        Some(200),
    )
}

#[tokio::test]
async fn raw_and_decoded_request_facts_preserve_escapes_and_credential_shaped_values() {
    let (shared, mut events) = protocol_fixture();
    let arguments = r#"{"command":"printf hello\n","token":"se\u0063ret-ABC","empty":"","zero":0,"flag":false,"items":[""],"null":null}"#;
    let mut body = serde_json::to_vec(&serde_json::json!({"messages":[
        {"role":"system","content":"fixed instructions."},
        {"role":"user","content":"ordinary text."},
        {"role":"secret-ABC","tool_calls":[{"id":"secret-ABC","function":{"name":"secret-ABC","arguments":arguments}}]}
    ]})).expect("body");
    body.extend_from_slice(b" \n");
    let mut observer = body_observer(&shared, ProtocolSide::ClientRequest, false);
    let modified = b"actual modified bytes: secret-ABC\\n";
    assert!(
        matches!(EvidenceBody::from_bytes(modified.to_vec()).segments.as_slice(),
        [EvidenceSegment::Bytes{offset:0,data}] if data == modified)
    );
    observer.feed(&body).await.expect("feed");
    observer.finish(true).await.expect("finish");
    let mut finished = Vec::new();
    let mut raw = Vec::new();
    while let Ok(event) = events.try_recv() {
        match event.event.as_ref() {
            ChatEvent::Observation(o) => raw.push(o.clone()),
            ChatEvent::Finished(m) => finished.push(m.clone()),
            ChatEvent::Gap(_) => panic!("ordinary parameters caused a capture gap"),
            _ => {}
        }
    }
    assert_eq!(raw.len(), 1);
    assert_eq!(raw[0].body.original_length, Some(body.len() as u64));
    assert!(
        matches!(raw[0].body.segments.as_slice(),[EvidenceSegment::Bytes{offset:0,data}] if data == &body)
    );
    assert!(
        raw[0]
            .native_ids
            .iter()
            .any(|id| id.value == NativeIdValue::JsonString("secret-ABC".into()))
    );
    assert_eq!(finished.len(), 3);
    assert!(
        matches!(finished[0].observed_parts.as_slice(),[ChatPart::Text{text,..}] if text == "fixed instructions.")
    );
    assert!(
        matches!(finished[1].observed_parts.as_slice(),[ChatPart::Text{text,..}] if text == "ordinary text.")
    );
    assert_eq!(finished[2].role, ChatRole::Other("secret-ABC".into()));
    let [
        ChatPart::ToolCall {
            native_id: Some(id),
            name,
            arguments: Some(ChatContent::Json(value)),
            ..
        },
    ] = finished[2].observed_parts.as_slice()
    else {
        panic!("actual tool fields missing")
    };
    assert_eq!(id.value, NativeIdValue::JsonString("secret-ABC".into()));
    assert_eq!(name, "secret-ABC");
    assert_eq!(value["command"], "printf hello\n");
    assert_eq!(value["token"], "secret-ABC");
    assert_eq!(value["empty"], "");
    assert_eq!(value["zero"], 0);
    assert_eq!(value["flag"], false);
    assert_eq!(value["items"], serde_json::json!([""]));
    assert!(value["null"].is_null());
    assert!(
        finished
            .iter()
            .all(|m| m.state == ChatState::Complete && m.quality.is_empty())
    );
}

#[tokio::test]
async fn streaming_facts_keep_every_original_unit_and_actual_text_json_id_and_metadata() {
    let (shared, mut events) = protocol_fixture();
    let mut observer = body_observer(&shared, ProtocolSide::UpstreamResponse, true);
    let units = [
        format!(
            "event: secret-ABC\ndata: {}\n\n",
            serde_json::json!({"id":"secret-ABC","model":"secret-ABC","choices":[{"index":0,"delta":{"role":"custom-secret-ABC","content":"sec","tool_calls":[{"index":0,"id":"secret-ABC","function":{"name":"secret-ABC","arguments":r#"{"x":"se\u"#}}]}}]})
        ),
        format!(
            "event: secret-ABC\ndata: {}\n\n",
            serde_json::json!({"choices":[{"index":0,"delta":{"content":"ret-ABC","tool_calls":[{"index":0,"function":{"arguments":r#"0063ret-ABC"}"#}}]},"finish_reason":"secret-ABC"}],"usage":{"secret-ABC":0}})
        ),
        "event: secret-ABC\ndata: [DONE]\n\n".into(),
    ];
    for unit in &units {
        for fragment in unit.as_bytes().chunks(3) {
            observer.feed(fragment).await.expect("fragment");
        }
    }
    observer.finish(true).await.expect("finish");
    let mut observations = Vec::new();
    let mut finished = Vec::new();
    let mut first_fragment = false;
    while let Ok(event) = events.try_recv() {
        match event.event.as_ref() {
            ChatEvent::Observation(o) => observations.push(o.clone()),
            ChatEvent::Started(m) => {
                first_fragment = m
                    .observed_parts
                    .iter()
                    .any(|p| matches!(p,ChatPart::Text{text,..} if text == "sec"))
            }
            ChatEvent::Finished(m) => finished.push(m.clone()),
            ChatEvent::Gap(_) | ChatEvent::ExchangeFailed { .. } => {
                panic!("faithful capture failed")
            }
            _ => {}
        }
    }
    assert!(first_fragment, "actual streaming prefix was withheld");
    assert_eq!(observations.len(), units.len());
    for (observation, unit) in observations.iter().zip(&units) {
        assert_eq!(observation.event_name.as_deref(), Some("secret-ABC"));
        assert_eq!(observation.body.original_length, Some(unit.len() as u64));
        assert!(
            matches!(observation.body.segments.as_slice(),[EvidenceSegment::Bytes{offset:0,data}] if data == unit.as_bytes())
        );
    }
    assert_eq!(finished.len(), 1);
    let message = &finished[0];
    assert!(
        observations
            .iter()
            .all(|unit| Arc::ptr_eq(&unit.source, &message.source)),
        "the immutable direction source must be shared"
    );
    assert_eq!(message.role, ChatRole::Other("custom-secret-ABC".into()));
    assert_eq!(message.model.as_deref(), Some("secret-ABC"));
    assert_eq!(message.finish_reason.as_deref(), Some("secret-ABC"));
    assert_eq!(message.state, ChatState::Complete);
    assert!(message.quality.is_empty());
    assert!(
        message
            .observed_parts
            .iter()
            .any(|p| matches!(p,ChatPart::Text{text,..} if text == "secret-ABC"))
    );
    assert!(message.observed_parts.iter().any(|p| matches!(p,
        ChatPart::ToolCall{native_id:Some(id),name,arguments:Some(ChatContent::Json(value)),..}
        if id.value == NativeIdValue::JsonString("secret-ABC".into()) && name == "secret-ABC" && value["x"] == "secret-ABC")));
    assert!(
        message
            .usage
            .as_ref()
            .expect("actual usage")
            .iter()
            .any(|counter| counter.name == "secret-ABC" && counter.value == 0)
    );
    assert_eq!(shared.parse_failures.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn rejected_tool_arguments_leave_the_previous_projection_intact() {
    let (shared, mut events) = protocol_fixture();
    let mut observer = body_observer(&shared, ProtocolSide::UpstreamResponse, true);
    observer
        .feed(b"data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"accepted\"}}]}\n\n")
        .await
        .expect("accepted frame");
    let levels = shared.settings.decode_limits.max_json_depth as usize;
    let argument = format!("{}0{}", "[".repeat(levels + 1), "]".repeat(levels + 1));
    let frame = serde_json::json!({"choices":[{"index":0,"delta":{
        "content":" rejected",
        "tool_calls":[{"index":0,"id":"tool","function":{"name":"tool","arguments":argument}}]
    },"finish_reason":"tool_calls"}]});
    observer
        .feed(format!("data: {frame}\n\n").as_bytes())
        .await
        .expect("rejected projection");
    observer.finish(true).await.expect("finish");
    let mut gaps = 0;
    let mut observations = 0;
    let mut finished = Vec::new();
    while let Ok(event) = events.try_recv() {
        match event.event.as_ref() {
            ChatEvent::Observation(_) => observations += 1,
            ChatEvent::Gap(gap) if gap.reason == "decoded_json_depth_budget" => gaps += 1,
            ChatEvent::Finished(message) => finished.push(message.clone()),
            _ => {}
        }
    }
    assert_eq!((observations, gaps, finished.len()), (2, 1, 1));
    assert_eq!(finished[0].state, ChatState::Failed);
    assert!(finished[0].quality.contains(&CaptureIssue::Gap));
    assert!(
        matches!(finished[0].observed_parts.as_slice(), [ChatPart::Text { text, .. }] if text == "accepted")
    );
}
