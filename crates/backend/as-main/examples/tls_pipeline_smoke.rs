//! Disposable-guest proof of the real ingress → TLS → credential → chat pipeline.
use as_credentials::{CredentialInit, CredentialModule};
use as_flow::{FlowInit, FlowModule};
use as_main::{MainThreads, assembly::MainAssembly};
use as_types::{
    driver::TlsIngressInput, flow::*, process_source::ProcessInspection, process_tree::ProcessKey,
};
use bytes::Bytes;
use http_body_util::{BodyExt, Full, StreamBody};
use hyper::{
    Request, Response,
    body::{Frame, Incoming},
    service::service_fn,
};
use hyper_util::rt::TokioIo;
use serde_json::{Value, json};
use std::{
    convert::Infallible,
    io,
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    process::Command,
    sync::mpsc,
};
use tokio_rustls::{TlsAcceptor, TlsConnector};
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
fn require(value: bool, reason: &str) -> Result {
    if value {
        Ok(())
    } else {
        Err(io::Error::other(reason).into())
    }
}
struct TrustFixture {
    path: PathBuf,
    previous: Option<Vec<u8>>,
}
impl TrustFixture {
    fn install(pem: &str) -> Result<Self> {
        require(
            std::fs::read_to_string("/proc/cmdline")?
                .split_ascii_whitespace()
                .any(|s| s == "as_native_main_smoke=1"),
            "guest marker missing",
        )?;
        let path = PathBuf::from("/etc/ssl/certs/ca-certificates.crt");
        std::fs::create_dir_all(
            path.parent()
                .ok_or_else(|| io::Error::other("trust parent"))?,
        )?;
        let previous = match std::fs::read(&path) {
            Ok(v) => Some(v),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        let mut bytes = previous.clone().unwrap_or_default();
        bytes.push(b'\n');
        bytes.extend_from_slice(pem.as_bytes());
        std::fs::write(&path, bytes)?;
        Ok(Self { path, previous })
    }
}
impl Drop for TrustFixture {
    fn drop(&mut self) {
        match &self.previous {
            Some(bytes) => {
                let _ = std::fs::write(&self.path, bytes);
            }
            None => {
                let _ = std::fs::remove_file(&self.path);
            }
        }
    }
}
/// Called serially by the native-main guest, sharing its actual authenticated driver session.
pub async fn verify(assembly: &MainAssembly, threads: &MainThreads) -> Result {
    let provider = rcgen::generate_simple_self_signed(vec!["localhost".into()])?;
    let _trust = TrustFixture::install(&provider.cert.pem())?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let route = ProviderRoute {
        id: RouteId([0x31; 16]),
        origin: format!("https://localhost:{port}"),
        method: "POST".into(),
        path: "/v1/chat/completions".into(),
        profile: ProviderProfile::OpenAiChatCompletions,
        session: None,
    };
    let crypto = Arc::new(rustls::crypto::ring::default_provider());
    let mut provider_config = rustls::ServerConfig::builder_with_provider(crypto)
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(
            vec![provider.cert.der().clone()],
            rustls::pki_types::PrivatePkcs8KeyDer::from(provider.key_pair.serialize_der()).into(),
        )?;
    provider_config.alpn_protocols = vec![b"http/1.1".to_vec()];
    let hits = Arc::new(AtomicUsize::new(0));
    let count = hits.clone();
    let provider_task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await?;
        let tls = TlsAcceptor::from(Arc::new(provider_config))
            .accept(stream)
            .await?;
        let service = service_fn(move |request: Request<Incoming>| {
            let count = count.clone();
            async move {
                require(
                    request
                        .headers()
                        .get("authorization")
                        .is_some_and(|v| v.as_bytes() == b"Bearer tls-fixture-real-key"),
                    "provider did not receive exact real key",
                )
                .map_err(io::Error::other)?;
                let body = request
                    .into_body()
                    .collect()
                    .await
                    .map_err(io::Error::other)?
                    .to_bytes();
                let parsed: Value = serde_json::from_slice(&body).map_err(io::Error::other)?;
                require(
                    parsed["messages"][0]["content"] == "fixture question",
                    "provider body changed",
                )
                .map_err(io::Error::other)?;
                count.fetch_add(1, Ordering::SeqCst);
                let events = [
                    "data: {\"id\":\"fixture-provider-response\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"fixture \"}}]}\n\n",
                    "data: {\"id\":\"fixture-provider-response\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"answer\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
                ];
                let chunks: Vec<_> = events
                    .into_iter()
                    .map(|s| Ok::<_, Infallible>(Frame::data(Bytes::from_static(s.as_bytes()))))
                    .collect();
                let mut response = Response::new(StreamBody::new(futures::stream::iter(chunks)));
                response.headers_mut().insert(
                    "content-type",
                    hyper::header::HeaderValue::from_static("text/event-stream"),
                );
                Ok::<_, io::Error>(response)
            }
        });
        hyper::server::conn::http1::Builder::new()
            .serve_connection(TokioIo::new(tls), service)
            .await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    });
    let mut child = Command::new("/suite/tls_pipeline_smoke")
        .arg("--client")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()?;
    let pid = child.id().ok_or_else(|| io::Error::other("child pid"))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("child input"))?;
    let mut stdout = BufReader::new(
        child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("child output"))?,
    );
    let mut line = String::new();
    stdout.read_line(&mut line).await?;
    require(
        line.trim() == "TLS_PIPELINE_CLIENT_READY",
        "child not ready",
    )?;
    let driver = threads.driver_port();
    let inspected = driver.process_facts()?.inspect(&[pid]).await?;
    let birth = match inspected.as_slice() {
        [ProcessInspection::Bound { binding }] => binding.birth_boottime_ns,
        _ => return Err(io::Error::other("child has no native binding").into()),
    };
    let boot = threads
        .process_tree()
        .ok_or_else(|| io::Error::other("tree missing"))?
        .resolve_current_pid(std::process::id())?
        .ok_or_else(|| io::Error::other("controller key"))?
        .boot_id;
    let key = ProcessKey {
        boot_id: boot,
        host_tgid: pid,
        birth_boottime_ns: birth,
    };
    let version = assembly.configuration().snapshot().version();
    let dir = tempfile::tempdir()?;
    std::fs::set_permissions(
        dir.path(),
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )?;
    let credentials = CredentialModule::initialize(CredentialInit {
        protected_root: dir.path().to_path_buf(),
        store_file: dir.path().join("credentials/store"),
        ca_public_file: dir.path().join("credentials/ca.crt"),
        bindings: Arc::new(CredentialSettings::default()),
        routes: vec![route.clone()],
        config_version: version,
    })
    .await?;
    let ca = credentials.admin_port().ensure_ca().await?;
    let secret = credentials
        .admin_port()
        .import_secret(PrivateBytes::new(b"tls-fixture-real-key".to_vec()))
        .await?;
    let binding = BindingId([0x32; 16]);
    credentials.apply_settings(
        Arc::new(CredentialSettings {
            bindings: vec![CredentialBinding {
                id: binding,
                secret: secret.secret,
                enabled: true,
                subjects: vec![CredentialSubject::Process(key)],
                routes: vec![route.id],
                slot: AuthSlot::AuthorizationBearer,
            }],
        }),
        vec![route.clone()],
        version,
    )?;
    let phantom = credentials
        .admin_port()
        .issue_phantom(binding, version)
        .await?;
    let settings = FlowSettings {
        enabled: true,
        scope: FlowScope::Processes(vec![key]),
        provider_routes: vec![route.clone()],
        egress: Some(TlsEgressSettings {
            ca: ca.id,
            max_connections: 4,
            max_streams_per_connection: 4,
        }),
        ..FlowSettings::default()
    };
    let (tx, mut admissions) = mpsc::channel::<AdmittedConnection>(4);
    let (forward, rx) = mpsc::channel(4);
    let admitted_facts = tokio::spawn(async move {
        while let Some(connection) = admissions.recv().await {
            eprintln!(
                "{}",
                json!({"case":"tls_pipeline_admitted_source","process":connection.facts.connection_initiator.process,"process_matches":connection.facts.connection_initiator.process==Some(key),"config_matches":connection.facts.config_version==version,"original_remote_ip":connection.facts.original_remote_ip,"original_remote_port":connection.facts.original_remote_port,"port_matches":connection.facts.original_remote_port==port,"socket_cookie":connection.facts.channel.object_id})
            );
            if forward.send(connection).await.is_err() {
                break;
            }
        }
    });
    let (output, mut chat) = ChatSender::channel(u32::try_from(settings.event_queue_bytes)?);
    let (flow, destinations) = FlowModule::initialize(FlowInit {
        output,
        feedback_capacity: assembly
            .configuration()
            .snapshot()
            .document()
            .queues
            .messages,
        settings: Arc::new(settings),
        config_version: version,
        ingress_rx: rx,
        credentials: credentials.use_port(),
        certificates: credentials.certificate_port(),
    })
    .await?;
    let ingress = driver
        .start_tls_ingress(
            TlsIngressInput {
                boot_id: key.boot_id,
                scope: as_types::flow::FlowScope::Processes(vec![key]),
                destinations,
                config_version: version,
                max_connections: 4,
            },
            tx,
        )
        .await?;
    let messages = assembly.messages(threads);
    let (socket, _peer) = tokio::net::UnixStream::pair()?;
    let session = messages.connect(socket)?;
    messages.content_access(&session)?.authorize()?;
    require(
        credentials.admin_port().ensure_ca().await?.id == ca.id,
        "credential owner changed CA",
    )?;
    let received = tokio::spawn(async move {
        let mut records = Vec::new();
        while let Some(envelope) = chat.recv().await {
            records.push(envelope);
        }
        Ok::<_, io::Error>(records)
    });
    let private = json!({"port":port,"ca":ca.certificate_der,"phantom":std::str::from_utf8(phantom.value.expose())?});
    let mut bytes = serde_json::to_vec(&private)?;
    bytes.push(b'\n');
    stdin.write_all(&bytes).await?;
    stdin.shutdown().await?;
    drop(stdin);
    line.clear();
    let client_output = stdout.read_line(&mut line).await;
    eprintln!(
        "{}",
        json!({"case":"tls_pipeline_actual_ingress_state","ingress":ingress.status()?,"flow":flow.status_port().status()})
    );
    client_output?;
    require(
        line.trim() == "TLS_PIPELINE_CLIENT_PASS",
        "client rejected returned SSE",
    )?;
    require(child.wait().await?.success(), "child failed")?;
    let status = ingress.stop_accepting().await?;
    require(
        status.admitted == 1 && !status.accepting,
        "ingress did not stop exactly one admission",
    )?;
    admitted_facts.await?;
    flow.stop().await?;
    let records = received.await??;

    require(hits.load(Ordering::SeqCst) == 1, "upstream request count")?;
    require(records.iter().any(|e|matches!(e.event.as_ref(),ChatEvent::Observation(o)if o.source.actor.process==Some(key)&&o.source.side==ProtocolSide::ClientRequest)),"original request missing")?;
    require(records.iter().any(|e|matches!(e.event.as_ref(),ChatEvent::Finished(m)if m.source.actor.process==Some(key)&&m.observed_parts.iter().any(|p|matches!(p,ChatPart::Text{text,..}if text=="fixture answer")))),"same-source complete chat missing")?;
    let public = serde_json::to_vec(&records)?;
    require(
        !public.windows(20).any(|w| w == b"tls-fixture-real-key"),
        "real key leaked",
    )?;
    require(
        !public
            .windows(phantom.value.expose().len())
            .any(|w| w == phantom.value.expose()),
        "phantom leaked",
    )?;
    drop(session);
    provider_task.await??;
    println!(
        "{}",
        json!({"case":"real_tls_phantom_chat_pipeline","process":key,"original_target_port":port,"ingress_admitted":status.admitted,"upstream_requests":1,"public_records":records.len(),"phantom_replaced":true,"real_key_absent_from_observations":true,"request_and_response_raw":true,"authorized_chat_stream":true,"tls_verified_both_legs":true,"scope":"disposable_guest_software_provider_not_real_openclaw"})
    );
    Ok(())
}
async fn client() -> Result {
    println!("TLS_PIPELINE_CLIENT_READY");
    let line = tokio::task::spawn_blocking(|| {
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        Ok::<_, io::Error>(input)
    })
    .await??;
    let input: Value = serde_json::from_str(&line)?;
    let port = u16::try_from(
        input["port"]
            .as_u64()
            .ok_or_else(|| io::Error::other("port"))?,
    )?;
    let ca: Vec<u8> = serde_json::from_value(input["ca"].clone())?;
    let token = input["phantom"]
        .as_str()
        .ok_or_else(|| io::Error::other("token"))?;
    eprintln!(
        "{}",
        json!({"case":"tls_pipeline_client_stage","stage":"input_decoded"})
    );
    let mut roots = rustls::RootCertStore::empty();
    roots.add(rustls::pki_types::CertificateDer::from(ca))?;
    let mut config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_root_certificates(roots)
    .with_no_client_auth();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    let tcp = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).await?;
    eprintln!(
        "{}",
        json!({"case":"tls_pipeline_client_stage","stage":"tcp_connected"})
    );
    let tls = TlsConnector::from(Arc::new(config))
        .connect(rustls::pki_types::ServerName::try_from("localhost")?, tcp)
        .await.inspect_err(|error|{eprintln!("{}",json!({"case":"tls_pipeline_client_tls_error","io_kind":format!("{:?}",error.kind()),"os_error":error.raw_os_error(),"error":error.to_string()}));})?;
    eprintln!(
        "{}",
        json!({"case":"tls_pipeline_client_stage","stage":"client_tls_verified"})
    );
    let (mut sender, driver) = hyper::client::conn::http1::handshake(TokioIo::new(tls)).await?;
    let worker = tokio::spawn(async move {
        let _ = driver.await;
    });
    let request = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("host", format!("localhost:{port}"))
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from_static(
            br#"{"messages":[{"role":"user","content":"fixture question"}],"stream":true}"#,
        )))?;
    let response = sender.send_request(request).await?;
    let status = response.status().as_u16();
    let body = response.into_body().collect().await?.to_bytes();
    let error_code = if status == 200 {
        None
    } else {
        Some(match body.as_ref() {
            b"upstream_tls" => "upstream_tls",
            b"upstream_http" => "upstream_http",
            b"upstream_send" => "upstream_send",
            b"invalid_credential" => "invalid_credential",
            b"credential_scope" => "credential_scope",
            b"credential_unavailable" => "credential_unavailable",
            b"unconfigured_route" => "unconfigured_route",
            b"origin_mismatch" => "origin_mismatch",
            _ => "unrecognized_error_body",
        })
    };
    eprintln!(
        "{}",
        json!({"case":"tls_pipeline_client_response","http_status":status,"error_code":error_code,"body_bytes":body.len(),"done_marker":body.windows(7).any(|b|b==b"[DONE]\n")})
    );
    require(status == 200, "response status")?;
    require(body.windows(7).any(|b| b == b"[DONE]\n"), "no end marker")?;
    drop(sender);
    worker.await?;
    println!("TLS_PIPELINE_CLIENT_PASS");
    Ok(())
}
async fn dnscheck() {
    match tokio::net::lookup_host(("localhost", 1)).await {
        Ok(addresses) => println!(
            "{}",
            json!({"case":"AS_MAIN_TLS_DNS_RESULT","host":"localhost","resolved":true,"addresses":addresses.collect::<Vec<_>>() })
        ),
        Err(error) => println!(
            "{}",
            json!({"case":"AS_MAIN_TLS_DNS_RESULT","host":"localhost","resolved":false,"error_kind":format!("{:?}",error.kind()),"os_error":error.raw_os_error(),"error":error.to_string()})
        ),
    }
}
#[tokio::main(flavor = "current_thread")]
#[allow(dead_code)]
async fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("--dnscheck") => dnscheck().await,
        Some("--client") => {
            if let Err(error) = client().await {
                eprintln!(
                    "{}",
                    json!({"case":"tls_pipeline_client_error","error":error.to_string()})
                );
                eprintln!("TLS_PIPELINE_CLIENT_FAILED");
                std::process::exit(1);
            }
        }
        _ => {
            eprintln!("client mode required");
            std::process::exit(2);
        }
    }
}
