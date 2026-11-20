//! Explicit software wiring smoke. The fixture never loads or claims a native driver.
use as_main::{
    assembly::{MainAssembly, MainComponents},
    messages::{MainMessages, connections::Connection, driver::DriverPort, protocol::*},
};
use as_middleware::{
    business_log::BusinessLog,
    codec::{decode_down, encode_down, encode_up},
    media::Media,
    transport,
};
use as_policy_engine::{PolicyEngine, model::*};
use as_types::{
    configuration::{Access, ConfigDocument},
    main_ports::ModuleError,
    rule_execution::{BackendStage, RuleExecution},
};
use std::{error::Error, sync::Arc};
type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;
#[path = "../../../backend/as-main/examples/support/software_driver.rs"]
mod fixture;
// Review-only software source. It never claims to execute or protect a native operation.
#[derive(Debug)]
struct ReviewSource {
    id: as_types::DecisionId,
    event: as_types::dispatch_event::EventData,
    window: as_types::hold_budget::HoldWindow,
}
#[async_trait::async_trait]
impl as_types::operation::OriginalOperation for ReviewSource {
    fn try_result(
        &self,
    ) -> Option<std::result::Result<as_types::operation::OperationReceipt, ModuleError>> {
        None
    }
    async fn result(
        &self,
    ) -> std::result::Result<as_types::operation::OperationReceipt, ModuleError> {
        Err(ModuleError::Unsupported)
    }
    fn decision_id(&self) -> as_types::DecisionId {
        self.id
    }
    async fn bind_review(
        &self,
        binding: as_types::operation::ReviewBinding,
    ) -> std::result::Result<(), ModuleError> {
        if binding.deadline <= self.window.deadline() {
            Ok(())
        } else {
            Err(ModuleError::InvalidInput)
        }
    }
    fn hold_window(&self) -> as_types::hold_budget::HoldWindow {
        self.window
    }
    fn validate(
        &self,
        event: &as_types::dispatch_event::EventData,
    ) -> std::result::Result<(), ModuleError> {
        if event == &self.event {
            Ok(())
        } else {
            Err(ModuleError::InvalidInput)
        }
    }
    async fn apply(
        &self,
        _: &as_types::dispatch_event::EventData,
        _: &as_types::main_ports::RuleChoice,
    ) -> std::result::Result<as_types::operation::OperationReceipt, ModuleError> {
        Err(ModuleError::Unsupported)
    }
    fn cancel(&self) {}
}
fn require(condition: bool, message: &str) -> Result {
    if condition {
        Ok(())
    } else {
        Err(std::io::Error::other(message).into())
    }
}
async fn request(
    messages: &Arc<MainMessages>,
    session: &Connection,
    down: Down,
) -> Result<CommandResult> {
    let reply = messages
        .submit(session, down)?
        .ok_or("missing command reply")?
        .await?;
    // This component helper has no socket write authority; only the real
    // transport may confirm a stop reply's complete local write.
    let reply = reply.message;
    println!("UP {}", String::from_utf8(encode_up(&reply)?)?);
    if let Up::Reply { result, .. } = reply {
        Ok(result)
    } else {
        Err("unexpected event".into())
    }
}
async fn host_stop_transport(
    messages: &Arc<MainMessages>,
    media: &Arc<Media>,
    business: &BusinessLog,
    listener: tokio::sync::oneshot::Receiver<as_main::runtime::MainStopRequest>,
    threads: &mut Option<as_main::MainThreads>,
) -> Result {
    use as_types::flow::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let (server, client) = tokio::net::UnixStream::pair()?;
    let connection = messages.connect(server)?;
    let (peer, work) = transport::session(messages.clone(), connection, business.clone());
    media.set_peer(Some(peer.clone()))?;
    let session = tokio::spawn(work);
    let outcome: Result = async {
        let mut client = tokio::io::BufReader::new(client);
        let before = business.chat_tail().await?;
        let subscription = CommandId([0x59; 16]);
        let subscribe = serde_json::json!({"version": VERSION, "stream": {
            "id": subscription, "command": {"type": "subscribe", "filter": {}, "after": null}
        }});
        client
            .get_mut()
            .write_all(&serde_json::to_vec(&subscribe)?)
            .await?;
        client.get_mut().write_all(b"\n").await?;
        let mut line = String::new();
        client.read_line(&mut line).await?;
        let reply: serde_json::Value = serde_json::from_str(&line)?;
        require(
            reply["stream_reply"]["result"]["status"] == "subscribed",
            "subscription was not authorized",
        )?;
        line.clear();
        let id = CommandId([0x5a; 16]);
        let mut wire = encode_down(&Down::Control {
            id,
            command: ControlCommand::RequestStop,
        })?;
        wire.push(b'\n');
        client.get_mut().write_all(&wire).await?;
        let request = listener.await?;
        peer.begin_chat_drain();
        let retired = threads
            .take()
            .ok_or("missing runtime")?
            .shutdown_draining_content();
        request.reply_written.await?;
        require(
            client.read_line(&mut line).await? != 0,
            "stop reply socket ended before the accepted frame",
        )?;
        let value: serde_json::Value = serde_json::from_str(&line)?;
        require(
            value["message"]["id"] == serde_json::to_value(id)?
                && value["message"]["result"]["status"] == "accepted"
                && value["message"]["result"]["data"]["type"] == "main_stop_requested",
            "stop reply claimed a different effect or command",
        )?;
        // Commit these explicit software tail inputs through the business owner.
        // The earlier intake case has already consumed this Media's one Flow source.
        for sequence in 1..=2 {
            let event = PreparedChatEvent::new(ChatEvent::Gap(ObservationGap {
                kind: "software-fixture".into(),
                exchange: None,
                cursor_range: None,
                reason: format!("retirement-{sequence}"),
            }))?
            .into_envelope(ChatCursor {
                service_instance: [0x5a; 16],
                sequence,
            });
            business.record_chat(event.envelope().clone()).await?;
        }
        retired.await?;
        peer.finish_chat_drain(business.chat_tail().await?);
        let mut expected = Vec::new();
        let mut cursor = before;
        while let Some(committed) = business.read_next_chat(cursor).await? {
            cursor = Some(committed.cursor);
            expected.push(serde_json::to_value(committed.as_ref())?);
        }
        require(
            expected.len() == 2,
            "fixture did not commit the original tail",
        )?;
        let mut actual = Vec::new();
        loop {
            line.clear();
            if client.read_line(&mut line).await? == 0 {
                break;
            }
            let value: serde_json::Value = serde_json::from_str(&line)?;
            actual.push(value["message"]["envelope"].clone());
        }
        require(
            actual == expected,
            "stop truncated, reordered or changed committed chat inputs",
        )?;
        println!("AS_UNIX_CHAT_STOP_TAIL_PASS software_fixture_only");
        Ok(())
    }
    .await;
    peer.close();
    let ended = session.await?;
    media.set_peer(None)?;
    outcome?;
    ended?;
    println!("AS_REQUEST_STOP_REPLY_WRITTEN_PASS software_fixture_only");
    Ok(())
}
async fn edit_rule(
    messages: &Arc<MainMessages>,
    session: &Connection,
    policy: &PolicyEngine,
    change: RuleChange,
) -> Result {
    let down = Down::Rules {
        id: CommandId([2; 16]),
        command: RuleCommand::Edit {
            expected_version: policy.snapshot().version,
            change: Box::new(change),
        },
    };
    require(
        matches!(
            request(messages, session, decode_down(&encode_down(&down)?)?).await?,
            CommandResult::Completed {
                data: ReplyData::RulesSaved {
                    applied: Some(_),
                    ..
                }
            }
        ),
        "rule edit missing saved and adopted result",
    )
}

async fn remembered_rules(
    messages: &Arc<MainMessages>,
    session: &Connection,
    policy: &PolicyEngine,
) -> Result {
    let original = policy.snapshot();
    let mut group = original
        .document
        .groups
        .iter()
        .find_map(|g| match g {
            RuleGroup::File(g) if !g.execution.is_driver() => Some(g.clone()),
            _ => None,
        })
        .ok_or("file rule example absent")?;
    let mut rule = group.rules.pop().ok_or("file rule example empty")?;
    group.id = "remembered-file".into();
    group.enabled = true;
    group.execution = RuleExecution::Backend {
        stage: BackendStage::Remembered,
    };
    group.rules.clear();
    rule.id = "remembered-file-access".into();
    rule.action = Action::Deny {
        terminate: as_types::main_ports::Termination::None,
    };
    let group_id = group.id.clone();
    edit_rule(
        messages,
        session,
        policy,
        RuleChange::CreateGroup {
            group: Box::new(RuleGroup::File(group)),
        },
    )
    .await?;
    edit_rule(
        messages,
        session,
        policy,
        RuleChange::CreateRule {
            group_id: group_id.clone(),
            rule: Box::new(RuleDefinition::File(rule.clone())),
        },
    )
    .await?;
    let get = Down::Rules {
        id: CommandId([3; 16]),
        command: RuleCommand::GetRule {
            rule_id: rule.id.clone(),
        },
    };
    match request(messages, session, get).await? {
        CommandResult::Completed {
            data: ReplyData::Rule { snapshot },
        } => {
            require(
                snapshot.version == policy.snapshot().version
                    && snapshot.group_id == group_id
                    && snapshot.rule == RuleDefinition::File(rule.clone()),
                "single-rule read differed from saved snapshot",
            )?;
        }
        _ => return Err("remembered rule query failed".into()),
    }
    let stale = original.version;
    let rule_id = rule.id.clone();
    rule.enabled = false;
    edit_rule(
        messages,
        session,
        policy,
        RuleChange::ReplaceRule {
            rule_id: rule_id.clone(),
            rule: Box::new(RuleDefinition::File(rule)),
        },
    )
    .await?;
    require(
        matches!(
            request(
                messages,
                session,
                Down::Rules {
                    id: CommandId([4; 16]),
                    command: RuleCommand::Edit {
                        expected_version: stale,
                        change: Box::new(RuleChange::DeleteRule {
                            rule_id: rule_id.clone()
                        }),
                    },
                }
            )
            .await?,
            CommandResult::Failed {
                code: FailureCode::Conflict,
                ..
            }
        ),
        "stale rule edit overwrote a newer source",
    )?;
    edit_rule(
        messages,
        session,
        policy,
        RuleChange::DeleteRule {
            rule_id: rule_id.clone(),
        },
    )
    .await?;
    require(
        matches!(
            request(
                messages,
                session,
                Down::Rules {
                    id: CommandId([5; 16]),
                    command: RuleCommand::GetRule { rule_id },
                }
            )
            .await?,
            CommandResult::Failed {
                code: FailureCode::NotFound,
                ..
            }
        ),
        "deleted remembered rule remained readable",
    )?;
    edit_rule(
        messages,
        session,
        policy,
        RuleChange::DeleteGroup { group_id },
    )
    .await?;
    require(
        policy.snapshot().document == original.document,
        "rule CRUD changed unrelated rules",
    )?;
    Ok(())
}

async fn chat_backpressure_preserves_review_until_actual_eof(
    assembly: &mut MainAssembly,
    messages: &Arc<MainMessages>,
    media: &Arc<Media>,
    business: &BusinessLog,
) -> Result {
    use as_types::{event_operation::ProcessOperation, flow::*, main_ports::*};
    use std::{
        num::{NonZeroU32, NonZeroU64},
        time::Duration,
    };
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    fn review_event(sequence: u64) -> Result<as_types::dispatch_event::EventData> {
        Ok(serde_json::from_value(
            serde_json::json!({"header":{"kind":"process","stage":"controlled_request","operation":1,"source_instance":[4,4,4,4,4,4,4,4,4,4,4,4,4,4,4,4],"sequence_domain":"1","sequence":sequence.to_string(),"source_boottime_ns":null,"actor_tgid":null,"actor_tid":null,"actor_birth_boottime_ns":null,"actor_thread_birth_boottime_ns":null,"actor_cgroup_id":null},"process":as_types::dispatch_event::ProcessInfo::default(),"payload":as_types::dispatch_event::ProcessData::default()}),
        )?)
    }
    fn wait_review(
        assembly: &MainAssembly,
        sequence: u8,
    ) -> Result<tokio::task::JoinHandle<std::result::Result<Option<RuleChoice>, ModuleError>>> {
        let event = review_event(sequence.into())?;
        let source = Arc::new(ReviewSource {
            id: as_types::DecisionId([sequence; 16]),
            event: event.clone(),
            window: as_types::hold_budget::HoldWindow::begin(Duration::from_secs(30))?,
        });
        let rules = assembly.policy().select(EventKind::Process);
        Ok(tokio::spawn(async move {
            rules
                .prompt(ReviewRequest {
                    matched: &std::sync::OnceLock::new(),
                    event: Arc::new(event),
                    operation: source,
                })
                .await
        }))
    }
    let before = assembly.configuration().snapshot();
    let original_config = (**before.document()).clone();
    let (socket, _configuration_peer) = tokio::net::UnixStream::pair()?;
    let configuration = messages.connect(socket)?;
    let mut with_chat = original_config.clone();
    with_chat.middleware.chat_content = true;
    require(
        matches!(
            request(
                messages,
                &configuration,
                Down::Configuration {
                    id: CommandId([40; 16]),
                    command: ConfigCommand::Update {
                        expected_version: before.version(),
                        document: Box::new(with_chat)
                    }
                }
            )
            .await?,
            CommandResult::Completed { .. }
        ),
        "enable actual content grant",
    )?;
    let (server, _manager_peer) = tokio::net::UnixStream::pair()?;
    let manager = messages.connect(server)?;
    edit_rule(
        messages,
        &manager,
        assembly.policy(),
        RuleChange::CreateGroup {
            group: Box::new(RuleGroup::Process(Group {
                id: "software-chat-pressure".into(),
                number: NonZeroU32::MIN,
                enabled: true,
                execution: RuleExecution::Backend {
                    stage: BackendStage::Ask,
                },
                rules: vec![Rule {
                    id: "software-chat-pressure-ask".into(),
                    number: NonZeroU32::MIN,
                    enabled: true,
                    scope: Scope::AllProtected,
                    operations: vec![ProcessOperation::Execute],
                    source: Default::default(),
                    target: Default::default(),
                    action: Action::Ask {
                        timeout_ms: NonZeroU64::new(1000).ok_or("timeout")?,
                        timeout: Decision::Deny {
                            terminate: Termination::None,
                        },
                    },
                    log: false,
                    deny_message: String::new(),
                }],
            })),
        },
    )
    .await?;
    let mut prompts = assembly
        .take_prompts()
        .ok_or("prompt consumer already taken")?;
    let (server, client) = tokio::net::UnixStream::pair()?;
    rustix::net::sockopt::set_socket_send_buffer_size(&server, 4096)?;
    let connection = messages.connect(server)?;
    let old_grant = messages.credential_admin(&connection)?;
    let (peer, work) = transport::session(messages.clone(), connection, business.clone());
    media.set_peer(Some(peer.clone()))?;
    let transport = tokio::spawn(work);
    let mut client = tokio::io::BufReader::new(client);
    let mut registration = encode_down(&Down::ReviewAvailability { available: true })?;
    registration.push(b'\n');
    client.get_mut().write_all(&registration).await?;
    let mut subscribe = serde_json::to_vec(
        &serde_json::json!({"version":2,"stream":{"id":CommandId([43;16]),"command":{"type":"subscribe","filter":ChatFilter::default(),"after":null}}}),
    )?;
    subscribe.push(b'\n');
    client.get_mut().write_all(&subscribe).await?;
    let mut line = String::new();
    loop {
        line.clear();
        let n = tokio::time::timeout(Duration::from_secs(2), client.read_line(&mut line)).await??;
        require(n != 0, "closed before subscribe")?;
        if serde_json::from_str::<serde_json::Value>(&line)?["stream_reply"]["result"]["status"]
            == "subscribed"
        {
            break;
        }
    }
    require(
        assembly.interaction().available(),
        "real review registration",
    )?;
    // This software source feeds the same sole chat consumer as the real Flow
    // runtime. It does not claim a real model, captured actor or native protection.
    let (sending, inputs) = ChatSender::channel(FlowSettings::default().event_queue_bytes as u32);
    let consumer = media.clone();
    let stored = tokio::spawn(async move { consumer.consume_chat(inputs, None).await });
    for sequence in 1..=32 {
        let exchange = ExchangeKey {
            flow_instance: [46; 16],
            sequence: 1,
        };
        let message = ChatMessage {
            id: ChatMessageId { exchange, sequence },
            revision: 1,
            source: Arc::new(ProtocolSource {
                actor: ActorFacts {
                    process: None,
                    info: as_types::dispatch_event::ProcessInfo::default(),
                    tid: None,
                    thread_birth_boottime_ns: None,
                    cgroup_id: None,
                    observed_boottime_ns: 0,
                },
                channel: None,
                side: ProtocolSide::UpstreamResponse,
                request_traceparent: None,
                profile: ProviderProfile::OpenAiResponses,
                origin: "https://software.test".into(),
                route_label: "software".into(),
                config_version: assembly.configuration().snapshot().version(),
                observed_at_utc_ms: None,
                actual_auth_scope: None,
                credential_use: None,
            }),
            record_refs: Vec::new(),
            origin: ChatOrigin::ResponseOutput,
            role: ChatRole::Assistant,
            candidate_index: None,
            model: None,
            observed_parts: vec![ChatPart::Text {
                id: 0,
                kind: TextKind::Plain,
                text: "public software chat ".repeat(4096),
            }],
            state: ChatState::Complete,
            quality: Vec::new(),
            provider_status: Some("completed".into()),
            finish_reason: None,
            usage: None,
            first_seen_boottime_ns: sequence,
            last_seen_boottime_ns: sequence,
        };
        let record = PreparedChatEvent::new(ChatEvent::Finished(Arc::new(message)))?.into_envelope(
            ChatCursor {
                service_instance: [46; 16],
                sequence,
            },
        );
        sending.send(&record).await?;
    }
    drop(sending);
    tokio::time::timeout(Duration::from_secs(5), stored).await???;
    require(
        peer.ready() && assembly.interaction().available(),
        "chat pressure retired real review identity",
    )?;
    require(
        business.counts().await?.messages == 32,
        "chat backlog was not committed independently",
    )?;
    let first = wait_review(assembly, 44)?;
    let prompt = tokio::time::timeout(Duration::from_secs(2), prompts.recv())
        .await?
        .ok_or("first original ask absent")?;
    require(
        peer.try_prompt(prompt).is_none(),
        "live reviewer rejected original ask",
    )?;
    let choice = tokio::time::timeout(Duration::from_secs(2), first)
        .await???
        .ok_or("first original choice")?;
    require(
        choice.reason == ChoiceReason::Timeout
            && choice.decision
                == Decision::Deny {
                    terminate: Termination::None,
                }
            && choice.hit.is_some(),
        "slow socket bypassed or extended the original Ask",
    )?;
    require(
        peer.ready() && assembly.interaction().available(),
        "Ask expiry retired the reviewer",
    )?;
    let mut received = 0u64;
    while received < 32 {
        line.clear();
        let n = tokio::time::timeout(Duration::from_secs(3), client.read_line(&mut line)).await??;
        require(n != 0, "chat pressure closed the management socket")?;
        let value: serde_json::Value = serde_json::from_str(&line)?;
        if value["message"]["type"] == "chat" {
            received += 1;
            require(
                value["message"]["envelope"]["cursor"]["sequence"]
                    .as_str()
                    .and_then(|n| n.parse::<u64>().ok())
                    == Some(received),
                "SQL stream reordered or skipped an original input",
            )?;
        }
    }
    for (id, command, expected) in [
        (
            CommandId([47; 16]),
            serde_json::json!({"type":"unsubscribe","subscription_id":CommandId([43;16])}),
            "unsubscribed",
        ),
        (
            CommandId([48; 16]),
            serde_json::json!({"type":"subscribe","filter":ChatFilter::default(),"after":ChatCursor{service_instance:[250;16],sequence:0}}),
            "failed",
        ),
    ] {
        let mut bytes = serde_json::to_vec(
            &serde_json::json!({"version":2,"stream":{"id":id,"command":command}}),
        )?;
        bytes.push(b'\n');
        client.get_mut().write_all(&bytes).await?;
        loop {
            line.clear();
            let n =
                tokio::time::timeout(Duration::from_secs(2), client.read_line(&mut line)).await??;
            require(n != 0, "subscription validation closed reviewer")?;
            let value: serde_json::Value = serde_json::from_str(&line)?;
            if value["stream_reply"]["id"] == serde_json::to_value(id)? {
                require(
                    value["stream_reply"]["result"]["status"] == expected,
                    "wrong stream result",
                )?;
                if expected == "failed" {
                    require(
                        value["stream_reply"]["result"]["code"] == "not_found",
                        "unknown cursor was inferred or silently reset",
                    )?;
                }
                break;
            }
        }
    }
    require(
        peer.ready(),
        "chat validation failure revoked the review channel",
    )?;
    let second = wait_review(assembly, 45)?;
    let prompt = tokio::time::timeout(Duration::from_secs(2), prompts.recv())
        .await?
        .ok_or("following ask absent")?;
    require(
        peer.try_prompt(prompt).is_none(),
        "following Ask lost real review capability",
    )?;
    loop {
        line.clear();
        let n = tokio::time::timeout(Duration::from_secs(2), client.read_line(&mut line)).await??;
        require(n != 0, "closed before following Ask")?;
        let value: serde_json::Value = serde_json::from_str(&line)?;
        if value["message"]["type"] == "ask"
            && value["message"]["decision_id"]
                == serde_json::to_value(as_types::DecisionId([45; 16]))?
        {
            require(
                value["message"]["timeout"]
                    == serde_json::json!({"type":"deny","terminate":"none"}),
                "timeout changed across transport",
            )?;
            println!("AS_ORIGINAL_ASK_FRAME {}", line.trim_end());
            break;
        }
    }
    let mut answer = encode_down(&Down::Answer {
        decision_id: as_types::DecisionId([45; 16]),
        decision: Decision::Allow,
    })?;
    answer.push(b'\n');
    client.get_mut().write_all(&answer).await?;
    let choice = tokio::time::timeout(Duration::from_secs(2), second)
        .await???
        .ok_or("following original choice")?;
    require(
        choice.reason == ChoiceReason::User && choice.decision == Decision::Allow,
        "following Ask was bypassed or blocked by SQL",
    )?;
    let question = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match business.question(as_types::DecisionId([45; 16])).await {
                Ok(q) => return Ok(q),
                Err(as_middleware::business_log::StoreError::NotProvided) => {
                    tokio::task::yield_now().await
                }
                Err(e) => return Err(e),
            }
        }
    })
    .await??;
    require(
        matches!(question.as_ref(), Up::Ask { .. }),
        "real Ask SQL disappeared with output observation",
    )?;
    require(peer.ready(), "answered Ask lost its connection")?;
    drop(client);
    tokio::time::timeout(Duration::from_secs(2), transport).await???;
    require(
        !assembly.interaction().available(),
        "true EOF did not retire review channel",
    )?;
    require(
        matches!(
            old_grant.ensure_ca().await,
            Err(CredentialError::ScopeDenied)
        ),
        "true EOF kept original authorization",
    )?;
    media.close_peer()?;
    edit_rule(
        messages,
        &manager,
        assembly.policy(),
        RuleChange::DeleteGroup {
            group_id: "software-chat-pressure".into(),
        },
    )
    .await?;
    let version = assembly.configuration().snapshot().version();
    require(
        matches!(
            request(
                messages,
                &manager,
                Down::Configuration {
                    id: CommandId([40; 16]),
                    command: ConfigCommand::Update {
                        expected_version: version,
                        document: Box::new(original_config)
                    }
                }
            )
            .await?,
            CommandResult::Completed { .. }
        ),
        "restore original configuration",
    )?;
    println!("AS_CHAT_BACKPRESSURE_ASK_AND_TRUE_EOF_PASS software_fixture_only");
    Ok(())
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result {
    let mut assembly = MainAssembly::initialize().await?;
    let paths = assembly.configuration().shared_paths();
    require(
        Arc::ptr_eq(&paths, assembly.context().paths()),
        "context copied paths",
    )?;
    let business = BusinessLog::initialize(&paths, assembly.log_storage().clone()).await?;
    let media = Arc::new(Media::new(business.clone()));
    let (_stop, stopped) = tokio::sync::broadcast::channel(1);
    let mut threads = assembly
        .start(
            Arc::new(fixture::SoftwareDriver),
            MainComponents {
                messages: media.clone(),
            },
            stopped,
        )
        .await?;
    let stop_listener = threads.bind_stop_listener()?;
    let messages = assembly.messages(&threads);
    require(
        Arc::ptr_eq(&messages, &assembly.messages(&threads)),
        "main created a second management owner",
    )?;
    chat_backpressure_preserves_review_until_actual_eof(
        &mut assembly,
        &messages,
        &media,
        &business,
    )
    .await?;
    let (stream, _peer) = tokio::net::UnixStream::pair()?;
    let session = messages.connect(stream)?;
    let id = CommandId([1; 16]);
    require(
        matches!(
            request(
                &messages,
                &session,
                Down::Configuration {
                    id,
                    command: ConfigCommand::Get
                }
            )
            .await?,
            CommandResult::Completed {
                data: ReplyData::Configuration { .. }
            }
        ),
        "configuration query failed",
    )?;
    let before = assembly.configuration().snapshot();
    let mut document = (**before.document()).clone();
    document.driver.startup_protection.process = !document.driver.startup_protection.process;
    let saved = request(
        &messages,
        &session,
        Down::Configuration {
            id,
            command: ConfigCommand::Update {
                expected_version: before.version(),
                document: Box::new(document.clone()),
            },
        },
    )
    .await?;
    require(
        matches!(
            saved,
            CommandResult::Completed {
                data: ReplyData::ConfigurationSaved {
                    applied: Some(_),
                    ..
                }
            }
        ),
        "software source did not confirm its simulated adoption",
    )?;
    let disk: ConfigDocument =
        serde_saphyr::from_str(&std::fs::read_to_string(paths.configuration_file())?)?;
    require(disk == document, "configuration was not actually saved")?;
    require(
        Arc::ptr_eq(&paths, &assembly.configuration().shared_paths()),
        "save changed path allocation",
    )?;
    require(
        matches!(
            request(
                &messages,
                &session,
                Down::Configuration {
                    id,
                    command: ConfigCommand::Update {
                        expected_version: before.version(),
                        document: Box::new(document)
                    }
                }
            )
            .await?,
            CommandResult::Failed {
                code: FailureCode::Conflict,
                ..
            }
        ),
        "stale write accepted",
    )?;
    require(
        matches!(
            request(
                &messages,
                &session,
                Down::Control {
                    id,
                    command: ControlCommand::GetStatus
                }
            )
            .await?,
            CommandResult::Completed {
                data: ReplyData::DriverStatus { .. }
            }
        ),
        "software source status query failed",
    )?;
    let rules = assembly.policy().snapshot();
    remembered_rules(&messages, &session, assembly.policy()).await?;
    require(
        matches!(
            request(
                &messages,
                &session,
                Down::Rules {
                    id,
                    command: RuleCommand::Update {
                        expected_version: rules.version,
                        document: (*rules.document).clone()
                    }
                }
            )
            .await?,
            CommandResult::Completed {
                data: ReplyData::RulesSaved {
                    applied: Some(_),
                    ..
                }
            }
        ),
        "software rule adoption missing",
    )?;
    require(
        matches!(
            request(
                &messages,
                &session,
                Down::Rules {
                    id,
                    command: RuleCommand::Reload {
                        expected_version: assembly.policy().snapshot().version
                    }
                }
            )
            .await?,
            CommandResult::Completed {
                data: ReplyData::RulesReloaded { .. }
            }
        ),
        "reload reported a save",
    )?;
    messages.submit(&session, Down::ReviewAvailability { available: true })?;
    require(
        assembly.interaction().available(),
        "review registration failed",
    )?;
    // A synthetic Prompt tests transport correlation only; it is not a kernel/rule hit claim.
    let mut frame = vec![0u8; 88];
    frame[0..2].copy_from_slice(&1u16.to_le_bytes());
    frame[2..4].copy_from_slice(&88u16.to_le_bytes());
    frame[4..8].copy_from_slice(&88u32.to_le_bytes());
    frame[8..10].copy_from_slice(&1u16.to_le_bytes());
    frame[10..12].copy_from_slice(&2u16.to_le_bytes());
    frame[12..16].copy_from_slice(&1u32.to_le_bytes());
    frame[16..32].copy_from_slice(&[1; 16]);
    frame[32..40].copy_from_slice(&1u64.to_le_bytes());
    frame[40..48].copy_from_slice(&1u64.to_le_bytes());
    let decision_id = as_types::DecisionId([2; 16]);
    let (reply, answered) = tokio::sync::oneshot::channel();
    let ask = messages.question(
        &session,
        as_policy_engine::interaction::Prompt {
            decision_id,
            event: Arc::new(as_types::dispatch_event::decode(&frame)?),
            hit: as_types::main_ports::RuleHit {
                group_id: "software-fixture".into(),
                group_number: std::num::NonZeroU32::MIN,
                rule_id: "software-fixture".into(),
                rule_number: std::num::NonZeroU32::MIN,
                source: as_types::main_ports::RuleSourceVersion::RulesDocument(
                    assembly.policy().snapshot().version,
                ),
                log: false,
                deny_message: None,
            },
            deadline: tokio::time::Instant::now() + std::time::Duration::from_secs(5),
            timeout: as_types::main_ports::Decision::Allow,
            reply,
        },
    )?;
    require(
        matches!(ask, Up::Ask { decision_id: id, .. } if id == decision_id),
        "question lost original identity",
    )?;
    require(
        matches!(
            messages.submit(
                &session,
                Down::Answer {
                    decision_id,
                    decision: as_types::main_ports::Decision::Deny {
                        terminate: as_types::main_ports::Termination::Actor,
                    },
                },
            ),
            Err(ModuleError::InvalidInput)
        ),
        "an action outside the prompt's choices was accepted",
    )?;
    require(
        matches!(
            messages.submit(
                &session,
                Down::Answer {
                    decision_id: as_types::DecisionId([3; 16]),
                    decision: as_types::main_ports::Decision::Allow
                }
            ),
            Err(ModuleError::Forbidden)
        ),
        "wrong original accepted an answer",
    )?;
    messages.submit(
        &session,
        Down::Answer {
            decision_id,
            decision: as_types::main_ports::Decision::Allow,
        },
    )?;
    require(
        answered.await? == as_types::main_ports::Decision::Allow,
        "answer did not reach original capability",
    )?;
    // A selected transport owns its reply lifetime even if another transport remains ready.
    let (a, _a_peer) = tokio::net::UnixStream::pair()?;
    let selected = messages.connect(a)?;
    let (b, _b_peer) = tokio::net::UnixStream::pair()?;
    let standby = messages.connect(b)?;
    messages.submit(&selected, Down::ReviewAvailability { available: true })?;
    messages.submit(&standby, Down::ReviewAvailability { available: true })?;
    let (reply, abandoned) = tokio::sync::oneshot::channel();
    messages.question(
        &selected,
        as_policy_engine::interaction::Prompt {
            decision_id: as_types::DecisionId([8; 16]),
            event: Arc::new(as_types::dispatch_event::decode(&frame)?),
            hit: as_types::main_ports::RuleHit {
                group_id: "software-disconnect".into(),
                group_number: std::num::NonZeroU32::MIN,
                rule_id: "software-disconnect".into(),
                rule_number: std::num::NonZeroU32::MIN,
                source: as_types::main_ports::RuleSourceVersion::RulesDocument(
                    assembly.policy().snapshot().version,
                ),
                log: false,
                deny_message: None,
            },
            deadline: tokio::time::Instant::now() + std::time::Duration::from_secs(5),
            timeout: as_types::main_ports::Decision::Allow,
            reply,
        },
    )?;
    drop(selected);
    require(
        assembly.interaction().available(),
        "standby channel should remain ready",
    )?;
    require(
        tokio::time::timeout(std::time::Duration::from_millis(200), abandoned)
            .await?
            .is_err(),
        "lost selected channel retained an unreachable question",
    )?;
    drop(standby);
    let before = assembly.configuration().snapshot();
    let mut document = (**before.document()).clone();
    // Deliberate test-only edit of the bound source exercises Reload, not a product write API.
    let mut disk_edit = document.clone();
    disk_edit.process_tree.query_page_size =
        std::num::NonZeroUsize::new(document.process_tree.query_page_size.get() + 1)
            .ok_or("page overflow")?;
    std::fs::write(
        paths.configuration_file(),
        serde_saphyr::to_string(&disk_edit)?,
    )?;
    require(
        matches!(
            request(
                &messages,
                &session,
                Down::Configuration {
                    id,
                    command: ConfigCommand::Reload {
                        expected_version: before.version()
                    }
                }
            )
            .await?,
            CommandResult::Failed {
                code: FailureCode::Unavailable,
                facts: Some(ReplyData::Configuration { .. }),
                ..
            }
        ),
        "reload reported new process-tree resources as applied",
    )?;
    let saved = assembly.configuration().snapshot();
    assembly
        .configuration()
        .update(saved.version(), document.clone())
        .await?;
    // Real Reload publishes the bound source before credential adoption. An unrelated
    // missing secret must not leave the previous middleware grant usable.
    let before_reload = assembly.configuration().snapshot();
    let mut denied_policy = document.clone();
    denied_policy.middleware.access = Access::ReadOnly;
    denied_policy.middleware.chat_content = false;
    let route = as_types::flow::ProviderRoute {
        id: as_types::flow::RouteId([0xea; 16]),
        origin: "https://api.example.test".into(),
        method: "POST".into(),
        path: "/v1/messages".into(),
        profile: as_types::flow::ProviderProfile::AnthropicMessages,
        session: None,
    };
    denied_policy.flow = Some(as_types::flow::FlowSettings {
        provider_routes: vec![route.clone()],
        ..Default::default()
    });
    denied_policy.credentials.bindings = vec![as_types::flow::CredentialBinding {
        id: as_types::flow::BindingId([0xeb; 16]),
        secret: as_types::flow::SecretRef([0xec; 16]),
        enabled: true,
        subjects: vec![as_types::flow::CredentialSubject::Process(
            as_types::process_tree::ProcessKey {
                boot_id: [1; 16],
                host_tgid: 55,
                birth_boottime_ns: 66,
            },
        )],
        routes: vec![route.id],
        slot: as_types::flow::AuthSlot::XApiKey,
    }];
    std::fs::write(
        paths.configuration_file(),
        serde_saphyr::to_string(&denied_policy)?,
    )?;
    require(
        matches!(
            request(
                &messages,
                &session,
                Down::Configuration {
                    id,
                    command: ConfigCommand::Reload {
                        expected_version: before_reload.version()
                    }
                }
            )
            .await?,
            CommandResult::Failed {
                code: FailureCode::Unavailable,
                facts: Some(ReplyData::Configuration { snapshot }),
                ..
            } if snapshot.document().middleware.access == Access::ReadOnly
        ),
        "credential failure did not preserve the published policy fact",
    )?;
    require(
        session.authorize(Access::ManageService).is_err() && !assembly.interaction().available(),
        "failed credential adoption retained the previous management grant",
    )?;
    require(
        matches!(
            messages.submit(
                &session,
                Down::Configuration {
                    id,
                    command: ConfigCommand::Get
                }
            ),
            Err(ModuleError::Unauthenticated)
        ),
        "revoked transport remained admitted after failed Reload",
    )?;
    println!("AS_CONFIGURATION_RELOAD_REVOKES_BEFORE_CREDENTIAL_FAILURE_PASS");
    // Restore this software fixture through its actual owner, then authenticate anew.
    let reloaded = assembly.configuration().snapshot();
    assembly
        .configuration()
        .update(reloaded.version(), document.clone())
        .await?;
    let (stream, _restored_peer) = tokio::net::UnixStream::pair()?;
    let session = messages.connect(stream)?;
    document.middleware.access = Access::ReadOnly;
    require(
        matches!(
            request(
                &messages,
                &session,
                Down::Configuration {
                    id,
                    command: ConfigCommand::Update {
                        expected_version: assembly.configuration().snapshot().version(),
                        document: Box::new(document)
                    }
                }
            )
            .await?,
            CommandResult::Completed { .. }
        ),
        "middleware policy not adopted",
    )?;
    require(
        session.authorize(Access::ReadOnly).is_err() && !assembly.interaction().available(),
        "old session retained permission or review availability",
    )?;
    let (stream, _new_peer) = tokio::net::UnixStream::pair()?;
    let readonly = messages.connect(stream)?;
    require(
        matches!(
            request(
                &messages,
                &readonly,
                Down::Rules {
                    id,
                    command: RuleCommand::Get
                }
            )
            .await?,
            CommandResult::Completed { .. }
        ),
        "reauthenticated query failed",
    )?;
    require(
        matches!(
            request(
                &messages,
                &readonly,
                Down::Rules {
                    id,
                    command: RuleCommand::GetRule {
                        rule_id: "as-maintenance-source-open".into(),
                    },
                }
            )
            .await?,
            CommandResult::Completed {
                data: ReplyData::Rule { .. }
            }
        ),
        "readonly single-rule query denied",
    )?;
    require(
        matches!(
            messages.submit(
                &readonly,
                Down::Rules {
                    id,
                    command: RuleCommand::Edit {
                        expected_version: assembly.policy().snapshot().version,
                        change: Box::new(RuleChange::DeleteRule {
                            rule_id: "as-maintenance-source-open".into(),
                        }),
                    },
                }
            ),
            Err(ModuleError::Forbidden)
        ),
        "readonly rule edit accepted",
    )?;
    require(
        matches!(
            messages.submit(
                &readonly,
                Down::Control {
                    id,
                    command: ControlCommand::SetRuntimeFeature {
                        feature: as_types::configuration::ProtectionKind::Process,
                        enabled: false
                    }
                }
            ),
            Err(ModuleError::Forbidden)
        ),
        "readonly control accepted",
    )?;
    require(
        matches!(
            messages.submit(
                &readonly,
                Down::Control {
                    id,
                    command: ControlCommand::RequestStop,
                }
            ),
            Err(ModuleError::Forbidden)
        ),
        "readonly host-stop request accepted",
    )?;
    require(
        decode_down(br#"{"version":1,"message":{"type":"review_availability","available":true}}"#)
            .is_err(),
        "old protocol accepted",
    )?;
    require(
        DriverPort::unavailable()
            .request(as_types::driver_control::DriverCommand::GetStatus)
            .await
            .is_err(),
        "unavailable source reported success",
    )?;
    // Restore this software fixture through its actual configuration owner;
    // the remote read-only connection never acquires management privileges.
    let mut threads = Some(threads);
    let stop_verified: Result = async {
        let current = assembly.configuration().snapshot();
        let mut document = (**current.document()).clone();
        document.middleware.access = Access::ManageService;
        document.middleware.chat_content = true;
        assembly
            .configuration()
            .update(current.version(), document)
            .await?;
        host_stop_transport(&messages, &media, &business, stop_listener, &mut threads).await
    }
    .await;
    let retired = match threads {
        Some(threads) => threads.shutdown().await,
        None => Ok(()),
    };
    let closed = business.close().await;
    stop_verified?;
    retired?;
    closed?;
    println!("AS_CONFIGURATION_MAIN_SMOKE_PASS software_fixture_only");
    Ok(())
}
