//! Controlled local provider/client fixtures. They never call Main or emulate credential policy.
use as_types::flow::{AuthSlot, HttpsOrigin, PrivateBytes, ProviderProfile};
use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited, StreamBody, combinators::UnsyncBoxBody};
use hyper::{
    Request, Response,
    body::{Frame, Incoming},
    service::service_fn,
};
use hyper_util::rt::TokioIo;
use serde_json::{Value, json};
use std::{
    convert::Infallible,
    io::{BufRead, Read},
    net::{SocketAddr, SocketAddrV6},
    sync::{
        Arc, RwLock,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::mpsc,
    task::JoinSet,
    time::timeout,
};
use tokio_rustls::{TlsAcceptor, TlsConnector};
const MAX_INPUT: u64 = 128 * 1024;
const MAX_BODY: usize = 1024 * 1024;
type Body = UnsyncBoxBody<Bytes, Infallible>;
fn emit(value: Value) {
    println!("{value}");
}
fn failure(code: &str) {
    emit(json!({"event":"fixture_error","code":code}));
}
fn commands() -> mpsc::Receiver<Result<Value, &'static str>> {
    let (tx, rx) = mpsc::channel(4);
    tokio::task::spawn_blocking(move || {
        let mut input = std::io::stdin().lock();
        loop {
            let mut bytes = Vec::new();
            match (&mut input)
                .take(MAX_INPUT + 1)
                .read_until(b'\n', &mut bytes)
            {
                Ok(0) => break,
                Ok(_) if bytes.len() > MAX_INPUT as usize => {
                    let _ = tx.blocking_send(Err("input_too_large"));
                    break;
                }
                Err(_) => {
                    let _ = tx.blocking_send(Err("stdin_failed"));
                    break;
                }
                _ => {}
            }
            let command = serde_json::from_slice::<Value>(&bytes).map_err(|_| "invalid_json");
            let stop = command
                .as_ref()
                .ok()
                .and_then(|v| v.get("op"))
                .and_then(Value::as_str)
                == Some("stop");
            if tx.blocking_send(command).is_err() || stop {
                break;
            }
        }
    });
    rx
}
fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, &'static str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or("invalid_command")
}
fn profile(command: &Value) -> Result<ProviderProfile, &'static str> {
    let profile = command
        .get("profile")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|_| "invalid_profile")?
        .unwrap_or(ProviderProfile::OpenAiChatCompletions);
    match profile {
        ProviderProfile::OpenAiChatCompletions | ProviderProfile::OpenAiResponses => Ok(profile),
        _ => Err("unsupported_fixture_profile"),
    }
}
fn profile_path(profile: ProviderProfile) -> &'static str {
    match profile {
        ProviderProfile::OpenAiResponses => "/v1/responses",
        _ => "/v1/chat/completions",
    }
}
fn fixture_host(endpoint: SocketAddr) -> &'static str {
    if endpoint.is_ipv6() {
        "as-ipv6.test"
    } else {
        "localhost"
    }
}
fn crypto() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}
fn full(status: u16, body: Value) -> Response<Body> {
    let mut response = Response::new(Full::new(Bytes::from(body.to_string())).boxed_unsync());
    if let Ok(status) = hyper::StatusCode::from_u16(status) {
        *response.status_mut() = status;
    }
    response.headers_mut().insert(
        "content-type",
        hyper::header::HeaderValue::from_static("application/json"),
    );
    response
}
struct Provider {
    expected: RwLock<PrivateBytes>,
    slot: AuthSlot,
    total: AtomicU64,
    matched: AtomicU64,
    mismatched: AtomicU64,
    profile: ProviderProfile,
    tool_path: Option<String>,
}
impl Provider {
    fn stats(&self, event: &str) -> Value {
        json!({"event":event,"requests":self.total.load(Ordering::SeqCst),"auth_matches":self.matched.load(Ordering::SeqCst),"auth_mismatches":self.mismatched.load(Ordering::SeqCst)})
    }
    async fn respond(&self, request: Request<Incoming>) -> Response<Body> {
        let ordinal = self.total.fetch_add(1, Ordering::SeqCst) + 1;
        let header_count = request.headers().get_all("authorization").iter().count()
            + request.headers().get_all("x-api-key").iter().count();
        let actual = request
            .headers()
            .get(self.slot.header_name())
            .map(|v| v.as_bytes());
        let actual = match self.slot {
            AuthSlot::AuthorizationBearer => actual.and_then(|v| v.strip_prefix(b"Bearer ")),
            AuthSlot::XApiKey => actual,
        };
        let auth_matches = header_count == 1
            && self
                .expected
                .read()
                .is_ok_and(|expected| actual == Some(expected.expose()));
        if auth_matches {
            self.matched.fetch_add(1, Ordering::SeqCst);
        } else {
            self.mismatched.fetch_add(1, Ordering::SeqCst);
        }
        let post = request.method() == hyper::Method::POST;
        let path_matches = request.uri().path() == profile_path(self.profile);
        let body = match Limited::new(request.into_body(), MAX_BODY).collect().await {
            Ok(body) => body.to_bytes(),
            Err(_) => {
                emit(
                    json!({"event":"provider_request","ordinal":ordinal,"auth_matches":auth_matches,"credential_headers":header_count,"post":post,"path_matches":path_matches,"body_complete":false}),
                );
                return full(400, json!({"error":{"code":"fixture_body_failed"}}));
            }
        };
        let parsed = serde_json::from_slice::<Value>(&body).ok();
        let stream = parsed
            .as_ref()
            .and_then(|v| v.get("stream"))
            .and_then(Value::as_bool)
            == Some(true);
        let response_id = format!("fixture_response_{ordinal}");
        let call_id = format!("fixture_call_{ordinal}");
        let tool_results: Vec<Value> = parsed
            .as_ref()
            .and_then(|v| v.get("input"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|item| item.get("type").and_then(Value::as_str) == Some("function_call_output"))
            .cloned()
            .collect();
        let followup = self.profile == ProviderProfile::OpenAiResponses && !tool_results.is_empty();
        let prepared = auth_matches && post && path_matches && parsed.is_some();
        emit(
            json!({"event":"provider_request","ordinal":ordinal,"auth_matches":auth_matches,"credential_headers":header_count,"post":post,"path_matches":path_matches,"body_bytes":body.len(),"body_complete":true,"stream":stream,"response_prepared":prepared,"response_id":if prepared{Some(&response_id)}else{None},"tool_call_id":if prepared&&!followup{Some(&call_id)}else{None},"previous_response_id":parsed.as_ref().and_then(|v|v.get("previous_response_id")),"tool_results":tool_results}),
        );
        if !auth_matches {
            return full(401, json!({"error":{"code":"fixture_auth_mismatch"}}));
        }
        if !post || !path_matches {
            return full(404, json!({"error":{"code":"fixture_route_mismatch"}}));
        }
        if parsed.is_none() {
            return full(400, json!({"error":{"code":"fixture_invalid_json"}}));
        }
        if self.profile == ProviderProfile::OpenAiResponses {
            if stream {
                return full(
                    400,
                    json!({"error":{"code":"fixture_responses_stream_not_supported"}}),
                );
            }
            let output = if followup {
                json!([{"type":"message","id":format!("fixture_message_{ordinal}"),"role":"assistant","status":"completed","content":[{"type":"output_text","text":"fixture feedback received"}]}])
            } else {
                json!([{"type":"function_call","id":format!("fixture_item_{ordinal}"),"call_id":call_id,"name":"fixture_open_file","arguments":json!({"path":self.tool_path}).to_string(),"status":"completed"}])
            };
            return full(
                200,
                json!({"id":response_id,"object":"response","status":"completed","model":"fixture-model","output":output,"usage":{"input_tokens":3,"output_tokens":4,"total_tokens":7}}),
            );
        }
        let usage = json!({"prompt_tokens":3,"completion_tokens":4,"total_tokens":7});
        if !stream {
            return full(
                200,
                json!({"id":response_id,"object":"chat.completion","model":"fixture-model","choices":[{"index":0,"message":{"role":"assistant","content":"fixture answer","tool_calls":[{"id":call_id,"type":"function","function":{"name":"fixture_tool","arguments":"{\"value\":\"ok\"}"}}]},"finish_reason":"tool_calls"}],"usage":usage}),
            );
        }
        let records = [
            json!({"id":response_id,"object":"chat.completion.chunk","model":"fixture-model","choices":[{"index":0,"delta":{"role":"assistant","content":"fixture "}}]}),
            json!({"id":response_id,"object":"chat.completion.chunk","model":"fixture-model","choices":[{"index":0,"delta":{"content":"answer","tool_calls":[{"index":0,"id":call_id,"type":"function","function":{"name":"fixture_tool","arguments":"{\"value\":"}}]}}]}),
            json!({"id":response_id,"object":"chat.completion.chunk","model":"fixture-model","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"ok\"}"}}]},"finish_reason":"tool_calls"}]}),
            json!({"id":response_id,"object":"chat.completion.chunk","choices":[],"usage":usage}),
        ];
        let mut frames: Vec<Result<Frame<Bytes>, Infallible>> = records
            .into_iter()
            .map(|v| Ok(Frame::data(Bytes::from(format!("data: {v}\n\n")))))
            .collect();
        frames.push(Ok(Frame::data(Bytes::from_static(b"data: [DONE]\n\n"))));
        let mut response =
            Response::new(StreamBody::new(futures::stream::iter(frames)).boxed_unsync());
        response.headers_mut().insert(
            "content-type",
            hyper::header::HeaderValue::from_static("text/event-stream"),
        );
        response
    }
}
async fn initialize_provider(
    command: &Value,
) -> Result<(Arc<Provider>, TcpListener, Arc<rustls::ServerConfig>, Value), &'static str> {
    let bind = command
        .get("bind")
        .and_then(Value::as_str)
        .unwrap_or("127.0.0.1:0")
        .parse::<SocketAddr>()
        .map_err(|_| "invalid_bind")?;
    if !bind.ip().is_loopback() {
        return Err("loopback_required");
    }
    let secret = text(command, "expected_secret")?;
    if secret.is_empty() || secret.len() > 16 * 1024 {
        return Err("invalid_expected_secret");
    }
    let slot = command
        .get("auth_slot")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|_| "invalid_auth_slot")?
        .unwrap_or(AuthSlot::AuthorizationBearer);
    let profile = profile(command)?;
    let tool_path = if profile == ProviderProfile::OpenAiResponses {
        let path = text(command, "tool_path")?;
        if !std::path::Path::new(path).is_absolute() {
            return Err("absolute_tool_path_required");
        }
        Some(path.to_owned())
    } else {
        None
    };
    let host = fixture_host(bind);
    let certificate =
        rcgen::generate_simple_self_signed(vec![host.into()]).map_err(|_| "certificate_failed")?;
    let mut config = rustls::ServerConfig::builder_with_provider(crypto())
        .with_safe_default_protocol_versions()
        .map_err(|_| "tls_config_failed")?
        .with_no_client_auth()
        .with_single_cert(
            vec![certificate.cert.der().clone()],
            rustls::pki_types::PrivatePkcs8KeyDer::from(certificate.key_pair.serialize_der())
                .into(),
        )
        .map_err(|_| "tls_config_failed")?;
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    let listener = TcpListener::bind(bind).await.map_err(|_| "bind_failed")?;
    let address = listener.local_addr().map_err(|_| "address_failed")?;
    let ready = json!({"event":"provider_ready","pid":std::process::id(),"endpoint":address.to_string(),"origin":format!("https://{host}:{}",address.port()),"path":profile_path(profile),"profile":profile,"ca_der":certificate.cert.der().as_ref(),"ca_pem":certificate.cert.pem()});
    Ok((
        Arc::new(Provider {
            expected: RwLock::new(PrivateBytes::new(secret.as_bytes().to_vec())),
            slot,
            total: AtomicU64::new(0),
            matched: AtomicU64::new(0),
            mismatched: AtomicU64::new(0),
            profile,
            tool_path,
        }),
        listener,
        Arc::new(config),
        ready,
    ))
}
async fn provider(mut commands: mpsc::Receiver<Result<Value, &'static str>>) {
    let initialized = loop {
        match commands.recv().await {
            Some(Ok(c)) if c.get("op").and_then(Value::as_str) == Some("init") => {
                match initialize_provider(&c).await {
                    Ok(value) => break Some(value),
                    Err(code) => failure(code),
                }
            }
            Some(Ok(c)) if c.get("op").and_then(Value::as_str) == Some("stop") => break None,
            Some(Err(code)) => failure(code),
            Some(_) => failure("provider_init_required"),
            None => break None,
        }
    };
    let Some((provider, listener, config, ready)) = initialized else {
        return;
    };
    emit(ready);
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            command=commands.recv()=>match command{
                Some(Ok(c))=>match c.get("op").and_then(Value::as_str){
                    Some("stop")=>break,
                    Some("stats")=>emit(provider.stats("provider_stats")),
                    Some("set_secret")=>match text(&c,"expected_secret"){Ok(secret)if !secret.is_empty()&&secret.len()<=16*1024=>match provider.expected.write(){Ok(mut expected)=>{*expected=PrivateBytes::new(secret.as_bytes().to_vec());emit(json!({"event":"provider_secret_set"}));},Err(_)=>failure("provider_state_failed")},_=>failure("invalid_expected_secret")},
                    _=>failure("invalid_provider_command"),
                },Some(Err(code))=>failure(code),None=>break,
            },
            accepted=listener.accept()=>match accepted{
                Ok((stream,_))=>{let acceptor=TlsAcceptor::from(config.clone());let provider=provider.clone();connections.spawn(async move{match timeout(Duration::from_secs(5),acceptor.accept(stream)).await{
                    Ok(Ok(tls))=>{let service=service_fn(move|request|{let provider=provider.clone();async move{Ok::<_,Infallible>(provider.respond(request).await)}});let _=hyper::server::conn::http1::Builder::new().serve_connection(TokioIo::new(tls),service).await;},
                    _=>emit(json!({"event":"provider_tls_rejected"})),
                }});},Err(_)=>{failure("accept_failed");break;}
            },
            _=connections.join_next(),if !connections.is_empty()=>{},
        }
    }
    connections.abort_all();
    while connections.join_next().await.is_some() {}
    emit(provider.stats("provider_stopped"));
}
struct Client {
    endpoint: SocketAddr,
    origin: HttpsOrigin,
    config: Arc<rustls::ClientConfig>,
    phantom: PrivateBytes,
    slot: AuthSlot,
    profile: ProviderProfile,
    pending: Option<PendingCall>,
}
struct PendingCall {
    response_id: String,
    call_id: String,
    path: String,
    result: Option<Value>,
}
fn initialize_client(command: &Value) -> Result<Client, &'static str> {
    let endpoint = text(command, "endpoint")?
        .parse::<SocketAddr>()
        .map_err(|_| "invalid_endpoint")?;
    if !endpoint.ip().is_loopback() || endpoint.port() == 0 {
        return Err("loopback_endpoint_required");
    }
    let origin = HttpsOrigin::parse(text(command, "origin")?).map_err(|_| "invalid_origin")?;
    if origin.ascii_host != fixture_host(endpoint) || origin.port != endpoint.port() {
        return Err("fixture_origin_mismatch");
    }
    let ca: Vec<u8> = serde_json::from_value(command.get("ca_der").cloned().ok_or("missing_ca")?)
        .map_err(|_| "invalid_ca")?;
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(rustls::pki_types::CertificateDer::from(ca))
        .map_err(|_| "invalid_ca")?;
    let mut config = rustls::ClientConfig::builder_with_provider(crypto())
        .with_safe_default_protocol_versions()
        .map_err(|_| "tls_config_failed")?
        .with_root_certificates(roots)
        .with_no_client_auth();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    let slot = command
        .get("auth_slot")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|_| "invalid_auth_slot")?
        .unwrap_or(AuthSlot::AuthorizationBearer);
    Ok(Client {
        endpoint,
        origin,
        config: Arc::new(config),
        phantom: PrivateBytes::new(text(command, "phantom")?.as_bytes().to_vec()),
        slot,
        profile: profile(command)?,
        pending: None,
    })
}
async fn request(
    client: &Client,
    seq: u64,
    stream: bool,
    mapped: bool,
) -> Result<(Value, Option<PendingCall>), &'static str> {
    let mut value = Vec::new();
    if client.slot == AuthSlot::AuthorizationBearer {
        value.extend_from_slice(b"Bearer ");
    }
    value.extend_from_slice(client.phantom.expose());
    let mut auth =
        hyper::header::HeaderValue::from_bytes(&value).map_err(|_| "invalid_phantom_header")?;
    auth.set_sensitive(true);
    let payload = if client.profile == ProviderProfile::OpenAiResponses {
        if stream {
            return Err("fixture_responses_stream_not_supported");
        }
        if let Some(pending) = &client.pending {
            let result = pending
                .result
                .as_ref()
                .ok_or("original_tool_not_executed")?;
            json!({"model":"fixture-model","previous_response_id":pending.response_id,"input":[result],"stream":false})
        } else {
            json!({"model":"fixture-model","input":"fixture question","stream":false,"tools":[{"type":"function","name":"fixture_open_file","parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}}]})
        }
    } else {
        json!({"model":"fixture-model","messages":[{"role":"user","content":"fixture question"}],"stream":stream,"tools":[{"type":"function","function":{"name":"fixture_tool","parameters":{"type":"object","properties":{"value":{"type":"string"}},"required":["value"]}}}]})
    };
    let request = Request::builder()
        .method("POST")
        .uri(profile_path(client.profile))
        .header(
            "host",
            format!("{}:{}", client.origin.ascii_host, client.origin.port),
        )
        .header(client.slot.header_name(), auth)
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(payload.to_string())))
        .map_err(|_| "request_failed")?;
    let endpoint = match (client.endpoint, mapped) {
        (SocketAddr::V4(addr), true) => SocketAddr::V6(SocketAddrV6::new(
            addr.ip().to_ipv6_mapped(),
            addr.port(),
            0,
            0,
        )),
        (_, true) => return Err("mapped_requires_ipv4_endpoint"),
        (endpoint, false) => endpoint,
    };
    let tcp = timeout(Duration::from_secs(5), TcpStream::connect(endpoint))
        .await
        .map_err(|_| "connect_timeout")?
        .map_err(|_| "connect_failed")?;
    let peer = tcp.peer_addr().map_err(|_| "peer_unavailable")?;
    if peer != endpoint {
        return Err("original_peer_changed");
    }
    let name = rustls::pki_types::ServerName::try_from(client.origin.ascii_host.clone())
        .map_err(|_| "invalid_origin")?;
    let tls = timeout(
        Duration::from_secs(5),
        TlsConnector::from(client.config.clone()).connect(name, tcp),
    )
    .await
    .map_err(|_| "tls_timeout")?
    .map_err(|_| "tls_failed")?;
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(tls))
        .await
        .map_err(|_| "http_failed")?;
    let mut driver = tokio::spawn(async move {
        let _ = connection.await;
    });
    let result=async{
        let response=timeout(Duration::from_secs(10),sender.send_request(request)).await.map_err(|_|"response_timeout")?.map_err(|_|"http_failed")?;
        let status=response.status().as_u16();let sse=response.headers().get("content-type").and_then(|v|v.to_str().ok()).is_some_and(|v|v.starts_with("text/event-stream"));
        let body=timeout(Duration::from_secs(10),Limited::new(response.into_body(),MAX_BODY).collect()).await.map_err(|_|"body_timeout")?.map_err(|_|"body_failed")?.to_bytes();
        let done=body.windows(6).any(|w|w==b"[DONE]");
        let decoded=serde_json::from_slice::<Value>(&body).ok();
        let pending=if status==200&&client.profile==ProviderProfile::OpenAiResponses&&client.pending.is_none(){
            decoded.as_ref().and_then(|value|{
                let item=value.get("output")?.as_array()?.first()?;
                if item.get("type")?.as_str()? != "function_call" || item.get("name")?.as_str()? != "fixture_open_file" {return None;}
                let arguments:Value=serde_json::from_str(item.get("arguments")?.as_str()?).ok()?;
                Some(PendingCall{response_id:value.get("id")?.as_str()?.into(),call_id:item.get("call_id")?.as_str()?.into(),path:arguments.get("path")?.as_str()?.into(),result:None})
            })
        }else{None};
        let valid=if client.profile==ProviderProfile::OpenAiResponses {
            if client.pending.is_none(){pending.is_some()}else{decoded.as_ref().is_some_and(|v|v.pointer("/output/0/content/0/text").and_then(Value::as_str)==Some("fixture feedback received"))}
        }else if stream{sse&&done&&body.windows(12).any(|w|w==b"fixture_tool")}else{decoded.as_ref().is_some_and(|v|v.pointer("/choices/0/message/content").and_then(Value::as_str)==Some("fixture answer")&&v.pointer("/choices/0/message/tool_calls/0/function/name").and_then(Value::as_str)==Some("fixture_tool"))};
        let error_code=if status==200{None}else{Some(match body.as_ref(){b"invalid_credential"=>"invalid_credential",b"credential_scope"=>"credential_scope",b"upstream_tls"=>"upstream_tls",b"upstream_timeout"=>"upstream_timeout",_=>"http_error"})};
        let summary=json!({"event":"client_result","seq":seq,"pid":std::process::id(),"peer":peer.to_string(),"http_status":status,"verified_tls":true,"sse":sse,"done_marker":done,"payload_valid":valid,"body_bytes":body.len(),"error_code":error_code,"response_id":pending.as_ref().map(|v|&v.response_id),"tool_call_id":pending.as_ref().map(|v|&v.call_id)});
        Ok((summary,pending))
    }.await;
    drop(sender);
    if timeout(Duration::from_secs(2), &mut driver).await.is_err() {
        driver.abort();
        let _ = driver.await;
    }
    result
}
async fn client(mut commands: mpsc::Receiver<Result<Value, &'static str>>) {
    let mut state = None;
    let mut attempted = 0u64;
    let mut received = 0u64;
    while let Some(command) = commands.recv().await {
        match command {
            Err(code) => failure(code),
            Ok(c) => {
                match c.get("op").and_then(Value::as_str) {
                    Some("stop") => break,
                    Some("init") => match initialize_client(&c) {
                        Ok(client) => {
                            state = Some(client);
                            emit(json!({"event":"client_ready","pid":std::process::id()}));
                        }
                        Err(code) => failure(code),
                    },
                    Some("request") => {
                        let Some(client) = state.as_mut() else {
                            failure("client_init_required");
                            continue;
                        };
                        let Some(seq) = c.get("seq").and_then(Value::as_u64) else {
                            failure("invalid_sequence");
                            continue;
                        };
                        let stream = match c.get("stream") {
                            Some(Value::Bool(value)) => *value,
                            None => client.profile == ProviderProfile::OpenAiChatCompletions,
                            _ => {
                                failure("invalid_stream_flag");
                                continue;
                            }
                        };
                        attempted += 1;
                        match request(
                            client,
                            seq,
                            stream,
                            c.get("mapped").and_then(Value::as_bool) == Some(true),
                        )
                        .await
                        {
                            Ok((value, pending)) => {
                                if pending.is_some() {
                                    client.pending = pending;
                                }
                                received += 1;
                                emit(value);
                            }
                            Err(code) => {
                                let verified = match code {
                                    "tls_failed" => Some(false),
                                    "http_failed" | "response_timeout" | "body_timeout"
                                    | "body_failed" => Some(true),
                                    _ => None,
                                };
                                emit(
                                    json!({"event":"client_result","seq":seq,"pid":std::process::id(),"http_status":null,"verified_tls":verified,"error_code":code}),
                                );
                            }
                        }
                    }
                    Some("execute_call") => {
                        let Some(pending) = state
                            .as_mut()
                            .and_then(|client: &mut Client| client.pending.as_mut())
                        else {
                            failure("original_tool_call_required");
                            continue;
                        };
                        if pending.result.is_some() {
                            failure("original_tool_already_executed");
                            continue;
                        }
                        // This controlled application executes its parsed original call itself.
                        // The coordinator's later relation declaration is not a stock application bridge.
                        let (open_result, error) = match std::fs::File::open(&pending.path) {
                            Ok(file) => {
                                drop(file);
                                (0, Some(0))
                            }
                            Err(error) => (-1, error.raw_os_error()),
                        };
                        let output=json!({"operation":"file_open","open_result":open_result,"errno":error}).to_string();
                        let result = json!({"type":"function_call_output","call_id":pending.call_id,"output":output,"is_error":open_result==-1});
                        pending.result = Some(result.clone());
                        emit(
                            json!({"event":"tool_executed","pid":std::process::id(),"response_id":pending.response_id,"tool_call_id":pending.call_id,"path":pending.path,"open_result":open_result,"errno":error,"tool_result":result}),
                        );
                    }
                    _ => failure("invalid_client_command"),
                }
            }
        }
    }
    emit(
        json!({"event":"client_stopped","requests_attempted":attempted,"responses_received":received}),
    );
}
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mode = std::env::args().nth(1);
    if !matches!(mode.as_deref(), Some("--provider" | "--client")) {
        failure("mode_required");
        std::process::exit(2);
    }
    let provider_mode = mode.as_deref() == Some("--provider");
    emit(
        json!({"event":"fixture_ready","mode":if provider_mode{"provider"}else{"client"},"pid":std::process::id()}),
    );
    let input = commands();
    if provider_mode {
        provider(input).await;
    } else {
        client(input).await;
    }
}
