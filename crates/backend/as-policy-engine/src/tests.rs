use super::*;
use as_types::event_operation::ProcessOperation;
use as_types::rule_execution::{BackendStage, RuleExecution};
use as_types::{
    DecisionId,
    hold_budget::HoldWindow,
    workload::{ContainerInstance, Workload},
};
use std::{num::NonZeroUsize, time::Duration};

fn document() -> PolicyDocument {
    serde_saphyr::from_str(
        r#"
groups:
- kind: process
  id: process-fast
  number: 1
  enabled: true
  execution: { location: backend, stage: fast }
  rules:
  - id: deny-process
    number: 1
    enabled: true
    scope: {kind: all_protected}
    operations: [execute]
    source: {pid: 345, ppid: 123, executable: {operator: glob, value: '/usr/bin/*'}}
    target: {}
    action: {type: deny, terminate: actor_and_parent}
    log: true
- kind: file
  id: file-ask
  number: 1
  enabled: true
  execution: { location: backend, stage: ask }
  rules:
  - id: ask-file
    number: 1
    enabled: true
    scope: {kind: all_protected}
    operations: [open]
    target: {path: {operator: regex, value: '^/private/'}}
    action: {type: ask, timeout_ms: 100, timeout: {type: deny, terminate: none}}
    log: true
- kind: network
  id: network-fast
  number: 1
  enabled: true
  execution: { location: backend, stage: fast }
  rules:
  - id: allow-net
    number: 1
    enabled: true
    scope: {kind: all_protected}
    operations: [connect]
    target: {remote_cidr: '10.0.0.0/8', remote_port: {first: 443, last: 443}}
    action: {type: allow}
    log: false
"#,
    )
    .expect("current typed policy")
}
fn source(document: &PolicyDocument) -> (tempfile::TempDir, as_types::context::MainContext) {
    use as_types::context::*;
    let root = tempfile::tempdir().expect("test layout");
    let base = root.path();
    std::fs::create_dir(base.join("config")).expect("config directory");
    std::fs::write(
        base.join("config/rules.yaml"),
        serde_saphyr::to_string(document).expect("policy"),
    )
    .expect("source");
    let paths = Arc::new(ConfigPaths::from_configuration(ResolvedPaths {
        backend_executable: base.join("bin/as-service"),
        self_protection_root: base.into(),
        configuration_file: base.join("config/as.yaml"),
        rules_file: base.join("config/rules.yaml"),
        driver_assets_directory: base.join("driver"),
        middleware_endpoint: base.join("run/middleware.sock"),
        data_directory: base.join("data"),
        middleware_database_file: base.join("data/middleware/business.sqlite"),
        credential_store_file: base.join("data/credentials/store"),
        ca_public_file: base.join("data/credentials/ca.crt"),
        security_log_file: base.join("log/security.sqlite"),
    }));
    (root, MainContext::new(paths, TargetPlatform::current()))
}
async fn load(
    document: PolicyDocument,
    interaction: Arc<Interaction>,
) -> (
    tempfile::TempDir,
    as_types::context::MainContext,
    PolicyEngine,
) {
    let (root, context) = source(&document);
    let engine = PolicyEngine::initialize(&context, interaction)
        .await
        .expect("fixed source policy");
    (root, context, engine)
}
fn event(kind: u16) -> EventData {
    event_operation(kind, 1)
}
fn event_operation(kind: u16, operation: u32) -> EventData {
    let mut bytes = vec![0u8; 88];
    bytes[0..2].copy_from_slice(&1u16.to_le_bytes());
    bytes[2..4].copy_from_slice(&88u16.to_le_bytes());
    bytes[4..8].copy_from_slice(&88u32.to_le_bytes());
    bytes[8..10].copy_from_slice(&kind.to_le_bytes());
    bytes[10..12].copy_from_slice(&2u16.to_le_bytes());
    bytes[12..16].copy_from_slice(&operation.to_le_bytes());
    bytes[16..32].copy_from_slice(&[1; 16]);
    bytes[32..40].copy_from_slice(&1u64.to_le_bytes());
    bytes[40..48].copy_from_slice(&1u64.to_le_bytes());
    bytes[56..60].copy_from_slice(&345u32.to_le_bytes());
    bytes[80..84].copy_from_slice(&2u32.to_le_bytes());
    let mut v = as_types::dispatch_event::decode(&bytes).expect("original event");
    v.process.executable = Some(b"/usr/bin/worker".to_vec());
    v.process.parent_pid = Some(123);
    match &mut v.payload {
        EventPayload::File(v) => v.path = Some(b"/private/key".to_vec()),
        EventPayload::Network(v) => {
            v.remote_ip = Some("10.2.3.4".parse().expect("IP"));
            v.remote_port = Some(443);
        }
        _ => {}
    }
    v
}
fn interaction() -> (
    Arc<Interaction>,
    tokio::sync::mpsc::Receiver<interaction::Prompt>,
) {
    Interaction::channel(NonZeroUsize::new(8).expect("capacity"))
}
fn request<'a>(
    event: &'a EventData,
    budget: Duration,
    matched: &'a std::sync::OnceLock<RuleHit>,
) -> ReviewRequest<'a> {
    ReviewRequest {
        matched,
        event: Arc::new(event.clone()),
        operation: Arc::new(ReviewSource(
            HoldWindow::begin(budget).expect("window"),
            false,
        )),
    }
}

#[tokio::test]
async fn typed_families_patterns_and_offline_skips_ask() {
    let (port, mut prompts) = interaction();
    let (_workspace, _config, engine) = load(document(), port).await;
    let process = expect_direct(
        engine
            .select(EventKind::Process)
            .fast(&event(1))
            .expect("process"),
    );
    assert_eq!(
        process.decision,
        Decision::Deny {
            terminate: Termination::ActorAndParent
        }
    );
    assert!(process.hit.expect("rule").log);
    let net = engine.select(EventKind::Network);
    assert_eq!(
        expect_direct(net.fast(&event(3)).expect("network")).decision,
        Decision::Allow
    );
    let mut outside = event(3);
    if let EventPayload::Network(v) = &mut outside.payload {
        v.remote_ip = Some("192.0.2.1".parse().expect("IP"));
    }
    assert!(matches!(
        net.fast(&outside).expect("definite miss"),
        FastMatch::NoMatch
    ));
    let file = event(2);
    let matched = std::sync::OnceLock::new();
    let choice = engine
        .select(EventKind::File)
        .prompt(request(&file, Duration::from_secs(1), &matched))
        .await
        .expect("headless");
    let choice = choice.expect("explicit headless quick miss");
    assert_eq!(choice.decision, Decision::Allow);
    assert_eq!(choice.reason, ChoiceReason::NoInteraction);
    assert!(
        choice.hit.is_none(),
        "Ask pool is not matched without a channel"
    );
    assert!(matched.get().is_none(), "no invented Ask hit or rule log");
    assert!(prompts.try_recv().is_err());
}

#[tokio::test]
async fn definite_send_miss_can_finish_outside_an_active_review() {
    let (port, mut prompts) = interaction();
    let _connection = port.connect().expect("review channel");
    let mut doc = document();
    let RuleGroup::Network(group) = &mut doc.groups[2] else {
        panic!("network group");
    };
    group.execution = RuleExecution::Backend {
        stage: BackendStage::Ask,
    };
    group.rules[0].operations = vec![as_types::event_operation::NetworkOperation::Send];
    group.rules[0].action = Action::Ask {
        timeout_ms: 1_000.try_into().expect("positive timeout"),
        timeout: Decision::Deny {
            terminate: Termination::None,
        },
    };
    let (_workspace, _config, engine) = load(doc, port).await;
    let net = engine.select(EventKind::Network);
    let send = event_operation(3, 2);
    let mut reviewer_send = send.clone();
    let EventPayload::Network(target) = &mut reviewer_send.payload else {
        panic!("network event");
    };
    target.remote_port = Some(21854);
    assert!(matches!(
        net.fast(&reviewer_send),
        Ok(FastMatch::NoReviewMatch)
    ));
    let mut missing_port = send.clone();
    let EventPayload::Network(target) = &mut missing_port.payload else {
        panic!("network event");
    };
    target.remote_port = None;
    assert!(matches!(net.fast(&missing_port), Ok(FastMatch::NoMatch)));
    assert!(matches!(net.fast(&send), Ok(FastMatch::NoMatch)));
    assert!(prompts.try_recv().is_err());
}

#[tokio::test]
async fn interaction_answer_disconnect_and_original_deadline() {
    let (port, mut prompts) = interaction();
    let connection = port.connect().expect("real connection");
    let mut doc = document();
    let RuleGroup::File(group) = &mut doc.groups[1] else {
        panic!("file")
    };
    if let Action::Ask { timeout_ms, .. } = &mut group.rules[0].action {
        *timeout_ms = 10000.try_into().expect("positive");
    }
    let (_workspace, _config, engine) = load(doc, port.clone()).await;
    let file = event(2);
    let rules = engine.select(EventKind::File);
    let matched = std::sync::OnceLock::new();
    let wait = rules.prompt(request(&file, Duration::from_secs(2), &matched));
    tokio::pin!(wait);
    tokio::select! {result=&mut wait=>panic!("returned before answer: {result:?}"),prompt=prompts.recv()=>{prompt.expect("request").reply.send(Decision::Allow).expect("live");}}
    assert_eq!(
        wait.await.expect("result").expect("hit").reason,
        ChoiceReason::User
    );
    let matched = std::sync::OnceLock::new();
    let wait = rules.prompt(request(&file, Duration::from_secs(2), &matched));
    tokio::pin!(wait);
    let (pending, reconnected) = tokio::select! {_=&mut wait=>panic!("no early result"),prompt=prompts.recv()=>{let pending=prompt.expect("pending");drop(connection);let reconnected=port.connect().expect("reconnected");(pending,reconnected)}};
    let result = tokio::time::timeout(Duration::from_millis(100), wait)
        .await
        .expect("disconnect must retire before configured/source deadline")
        .expect("result")
        .expect("hit");
    assert!(pending.reply.is_closed());
    drop(reconnected);
    assert_eq!(result.reason, ChoiceReason::Timeout);
    assert_eq!(
        result.decision,
        Decision::Deny {
            terminate: Termination::None
        }
    );
    let _connection = port.connect().expect("connection");
    let result = rules
        .prompt(request(
            &file,
            Duration::from_millis(1),
            &std::sync::OnceLock::new(),
        ))
        .await
        .expect("deadline")
        .expect("hit");
    assert_eq!(result.reason, ChoiceReason::Timeout);
}

#[tokio::test]
async fn ordered_container_scope_missing_facts_and_no_name_rebinding() {
    let (port, _) = interaction();
    let mut doc = document();
    let RuleGroup::Process(group) = &mut doc.groups[0] else {
        panic!("process")
    };
    group.rules[0].source = ProcessMatch::default();
    let instance = ContainerInstance {
        runtime: "docker-local".into(),
        namespace: None,
        id: "full-object-A".into(),
    };
    group.rules[0].scope = Scope::Containers {
        instances: vec![instance.clone()],
    };
    group.rules[0].action = Action::Allow;
    let mut deny = group.rules[0].clone();
    deny.id = "deny-rest".into();
    deny.number = 2.try_into().expect("positive");
    deny.scope = Scope::AllProtected;
    deny.action = Action::Deny {
        terminate: Termination::None,
    };
    group.rules.push(deny);
    let (_workspace, _config, engine) = load(doc, port).await;
    let rules = engine.select(EventKind::Process);
    let mut e = event(1);
    assert!(
        matches!(rules.fast(&e), Ok(FastMatch::NeedsFacts)),
        "headless missing scope is not an allow or mismatch"
    );
    assert!(matches!(
        rules
            .prompt(request(
                &e,
                Duration::from_secs(1),
                &std::sync::OnceLock::new()
            ))
            .await,
        Err(ModuleError::Unavailable)
    ));
    struct ScopeEvidence(ContainerInstance);
    #[async_trait::async_trait]
    impl as_types::process_source::ProcessFactsPort for ScopeEvidence {
        async fn inspect(
            &self,
            _: &[u32],
        ) -> Result<Vec<as_types::process_source::ProcessInspection>, ModuleError> {
            Err(ModuleError::Unsupported)
        }
        async fn facts_barrier(
            &self,
        ) -> Result<as_types::process_source::ProcessFactsBarrier, ModuleError> {
            Err(ModuleError::Unsupported)
        }
        fn outside_containers(
            &self,
            instances: &[ContainerInstance],
            lineage: &[u64],
            _: u64,
        ) -> bool {
            instances == std::slice::from_ref(&self.0) && !lineage.contains(&132)
        }
    }
    engine
        .bind_scope_source(Arc::new(ScopeEvidence(instance.clone())))
        .expect("source");
    let proof_event = |group: &str, lineage: serde_json::Value| {
        let mut v = serde_json::to_value(event(1)).expect("event");
        v["header"]["source_boottime_ns"] = "100".into();
        v["header"]["actor_cgroup_id"] = group.into();
        v["header"]["actor_cgroup_lineage"] = lineage;
        serde_json::from_value::<EventData>(v).expect("original lineage")
    };
    let outside = proof_event("1", serde_json::json!(["1"]));
    assert_eq!(
        expect_direct(rules.fast(&outside).expect("outside proof"))
            .hit
            .expect("rule")
            .rule_id
            .as_str(),
        "deny-rest"
    );
    let descendant = proof_event("133", serde_json::json!(["1", "132", "133"]));
    assert!(matches!(rules.fast(&descendant), Ok(FastMatch::NeedsFacts)));
    assert!(matches!(rules.fast(&e), Ok(FastMatch::NeedsFacts)));
    e.process.workload = Some(Workload::Container(instance));
    assert_eq!(
        expect_direct(rules.fast(&e).expect("known A")).decision,
        Decision::Allow
    );
    e.process.workload = Some(Workload::Container(ContainerInstance {
        runtime: "docker-local".into(),
        namespace: None,
        id: "new-object-A".into(),
    }));
    assert_eq!(
        expect_direct(rules.fast(&e).expect("new instance"))
            .hit
            .expect("rule")
            .rule_id
            .as_str(),
        "deny-rest"
    );
    e.process.workload = Some(Workload::Host);
    assert_eq!(
        expect_direct(rules.fast(&e).expect("host"))
            .hit
            .expect("rule")
            .rule_id
            .as_str(),
        "deny-rest"
    );
}

#[tokio::test]
async fn source_update_keeps_old_inflight_snapshot_and_failed_reload_keeps_active_rules() {
    let (port, _) = interaction();
    let (workspace, config, engine) = load(document(), port).await;
    let old = engine.select(EventKind::Process);
    let old_version = engine.snapshot().version;
    let generation = engine.pin();
    let mut updated = (*engine.snapshot().document).clone();
    let g = updated
        .groups
        .iter_mut()
        .find_map(|g| match g {
            RuleGroup::Process(g) if g.id == "process-fast" => Some(g),
            _ => None,
        })
        .expect("original process group");
    g.execution = RuleExecution::Backend {
        stage: BackendStage::Remembered,
    };
    g.rules[0].action = Action::Allow;
    engine
        .update(old_version, updated)
        .await
        .expect("validated atomic update");
    assert_ne!(old_version, engine.snapshot().version);
    assert_eq!(generation.version(), old_version);
    let exact = engine
        .select_version(EventKind::Process, Some(old_version))
        .expect("retained driver generation");
    assert!(matches!(
        expect_direct(exact.fast(&event(1)).expect("original version")).decision,
        Decision::Deny { .. }
    ));
    drop(generation);
    assert!(matches!(
        engine.select_version(EventKind::Process, Some(old_version)),
        Err(ModuleError::Unavailable)
    ));
    assert!(matches!(
        expect_direct(old.fast(&event(1)).expect("old")).decision,
        Decision::Deny { .. }
    ));
    assert_eq!(
        expect_direct(
            engine
                .select(EventKind::Process)
                .fast(&event(1))
                .expect("new")
        )
        .decision,
        Decision::Allow
    );
    let (port, _) = interaction();
    let loaded = PolicyEngine::initialize(&config, port)
        .await
        .expect("reload persisted remembered");
    assert_eq!(loaded.snapshot().version, engine.snapshot().version);
    let version = loaded.snapshot().version;
    std::fs::write(workspace.path().join("config/rules.yaml"), "groups: [bad]")
        .expect("bad candidate");
    assert!(loaded.reload(version).await.is_err());
    assert_eq!(loaded.snapshot().version, version);
}

#[tokio::test]
async fn driver_order_validation_and_no_user_mode_install_claim() {
    let mut doc = document();
    let RuleGroup::Process(group) = &mut doc.groups[0] else {
        panic!("process")
    };
    group.execution = RuleExecution::Driver {};
    group.number = 2.try_into().expect("ordinary driver group");
    group.rules[0].source = ProcessMatch::default();
    group.rules[0].action = Action::ToUser;
    let mut later = group.rules[0].clone();
    later.id = "later-deny".into();
    later.number = 2.try_into().expect("number");
    later.action = Action::Deny {
        terminate: Termination::None,
    };
    group.rules.push(later);
    let policy = DriverPolicy::compile(&doc).expect("typed driver plan");
    let hit = policy.evaluate(&event(1)).expect("first match");
    assert!(matches!(hit.action, Action::ToUser));
    assert_eq!(
        hit.hit.expect("original hit").source,
        RuleSourceVersion::RulesDocument(super::document_version(&doc).expect("document digest"))
    );
    let (port, _) = interaction();
    let (_workspace, config) = source(&doc);
    let prepared = PolicyEngine::initialize(&config, port)
        .await
        .expect("source preparation");
    assert!(
        matches!(
            prepared.select(EventKind::Process).fast(&event(1)),
            Ok(FastMatch::NoMatch)
        ),
        "driver rules never run as backend fast rules"
    );
    let RuleGroup::Process(group) = &mut doc.groups[0] else {
        panic!("process")
    };
    group.rules[1].number = group.rules[0].number;
    assert!(DriverPolicy::compile(&doc).is_err());
}

#[tokio::test]
async fn remembered_precedes_manual_but_actions_within_a_stage_follow_numbers() {
    let (port, _) = interaction();
    let mut doc = document();
    let RuleGroup::Process(group) = &mut doc.groups[0] else {
        panic!("process")
    };
    let mut first = group.rules[0].clone();
    first.id = "allow-first".into();
    first.action = Action::Allow;
    first.number = 1.try_into().expect("number");
    group.rules[0].number = 2.try_into().expect("number");
    group.rules.push(first);
    let (_workspace, _config, engine) = load(doc.clone(), port).await;
    assert_eq!(
        expect_direct(
            engine
                .select(EventKind::Process)
                .fast(&event(1))
                .expect("match")
        )
        .decision,
        Decision::Allow
    );
    let RuleGroup::Process(group) = &mut doc.groups[0] else {
        panic!("process")
    };
    let mut remembered = group.clone();
    remembered.id = "remembered".into();
    remembered.number = 99.try_into().expect("number");
    remembered.execution = RuleExecution::Backend {
        stage: BackendStage::Remembered,
    };
    remembered.rules.truncate(1);
    remembered.rules[0].id = "remembered-deny".into();
    doc.groups.push(RuleGroup::Process(remembered));
    let (port, _) = interaction();
    let (_workspace, _config, engine) = load(doc, port).await;
    assert!(matches!(
        expect_direct(
            engine
                .select(EventKind::Process)
                .fast(&event(1))
                .expect("match")
        )
        .decision,
        Decision::Deny { .. }
    ));
    let mut wrong_parent = event(1);
    wrong_parent.process.parent_pid = Some(999);
    assert!(matches!(
        engine
            .select(EventKind::Process)
            .fast(&wrong_parent)
            .expect("different event facts"),
        FastMatch::NoMatch
    ));
}

#[derive(Debug)]
struct ReviewSource(HoldWindow, bool);
#[async_trait::async_trait]
impl as_types::operation::OriginalOperation for ReviewSource {
    fn try_result(&self) -> Option<Result<as_types::operation::OperationReceipt, ModuleError>> {
        None
    }
    async fn result(&self) -> Result<as_types::operation::OperationReceipt, ModuleError> {
        Err(ModuleError::Unavailable)
    }

    fn decision_id(&self) -> DecisionId {
        DecisionId([1; 16])
    }
    fn hold_window(&self) -> HoldWindow {
        self.0
    }
    async fn bind_review(&self, _: as_types::operation::ReviewBinding) -> Result<(), ModuleError> {
        if self.1 {
            Err(ModuleError::Unavailable)
        } else {
            Ok(())
        }
    }
    fn validate(&self, _: &EventData) -> Result<(), ModuleError> {
        Ok(())
    }
    async fn apply(
        &self,
        _: &EventData,
        _: &RuleChoice,
    ) -> Result<as_types::operation::OperationReceipt, ModuleError> {
        Err(ModuleError::Unavailable)
    }
    fn cancel(&self) {}
}

#[tokio::test]
async fn failed_original_binding_does_not_publish_a_question() {
    let (port, mut prompts) = interaction();
    let _channel = port.connect().expect("available channel");
    let (_workspace, _config, engine) = load(document(), port).await;
    let event = event(2);
    let matched = std::sync::OnceLock::new();
    let request = ReviewRequest {
        event: Arc::new(event.clone()),
        matched: &matched,
        operation: Arc::new(ReviewSource(
            HoldWindow::begin(Duration::from_secs(1)).expect("window"),
            true,
        )),
    };
    assert!(matches!(
        engine.select(EventKind::File).prompt(request).await,
        Err(ModuleError::Unavailable)
    ));
    assert!(
        matched.get().is_some(),
        "retain the real rule for failure logging"
    );
    assert!(
        prompts.try_recv().is_err(),
        "no unbound original is exposed to the user"
    );
}

fn expect_direct(value: FastMatch) -> RuleChoice {
    match value {
        FastMatch::Matched(choice) => choice,
        other => panic!("expected direct match, received {other:?}"),
    }
}

#[tokio::test]
async fn unresolved_prior_condition_never_falls_through_to_a_broad_allow() {
    let (port, mut prompts) = interaction();
    let _ready = port.connect().expect("real channel registration");
    let mut doc = document();
    let RuleGroup::Process(group) = &mut doc.groups[0] else {
        panic!("process")
    };
    group.rules[0].source.parent_executable = Some(TextMatch::Exact("/trusted/parent".into()));
    let mut fallback = group.rules[0].clone();
    fallback.id = "broad-allow".into();
    fallback.number = std::num::NonZeroU32::new(2).expect("order");
    fallback.source = ProcessMatch::default();
    fallback.action = Action::Allow;
    group.rules.push(fallback);
    let (_root, _context, engine) = load(doc, port).await;
    let mut input = event(1);
    let rules = engine.select(EventKind::Process);
    assert!(matches!(rules.fast(&input), Ok(FastMatch::NeedsFacts)));
    let matched = std::sync::OnceLock::new();
    assert!(matches!(
        rules
            .prompt(request(&input, Duration::from_secs(1), &matched))
            .await,
        Err(ModuleError::Unavailable)
    ));
    assert!(matched.get().is_none());
    assert!(prompts.try_recv().is_err());
    input.process.parent_executable = Some(b"/different/parent".to_vec());
    assert!(matches!(
        expect_direct(rules.fast(&input).expect("definite mismatch")).decision,
        Decision::Allow
    ));
}

#[test]
fn explicit_execution_rejects_legacy_or_contradictory_placement_and_observation_rules() {
    let doc = document();
    let text = serde_saphyr::to_string(&doc).expect("current encoding");
    let value: serde_json::Value = serde_json::to_value(&doc).expect("current document");
    let mut old = value.clone();
    old["groups"][0]
        .as_object_mut()
        .expect("group")
        .remove("execution");
    old["groups"][0]["stage"] = "fast".into();
    assert!(serde_json::from_value::<PolicyDocument>(old).is_err());
    let mut invalid = value;
    invalid["groups"][0]["execution"] = serde_json::json!({"location":"driver","stage":"ask"});
    assert!(serde_json::from_value::<PolicyDocument>(invalid).is_err());
    assert!(serde_saphyr::from_str::<PolicyDocument>(&text).is_ok());
    let mut observation = doc;
    let RuleGroup::Process(group) = &mut observation.groups[0] else {
        panic!("process")
    };
    group.rules[0].operations = vec![ProcessOperation::Create];
    let (port, _) = interaction();
    assert!(build(observation, port, ScopeSource::default()).is_err());
}

#[tokio::test]
async fn native_export_keeps_three_family_order_and_rejects_unexecutable_backend_actions() {
    use as_types::driver_control::{DriverCapabilities, DriverFeature};
    let (interaction, _) = interaction();
    let (_root, _context, engine) = load(document(), interaction).await;
    let mut caps = DriverCapabilities {
        max_frame_bytes: 16384,
        max_pending: 64,
        max_transfer_bytes: 4 * 1024 * 1024,
        max_hold_ms: 300000,
        operation_mask: DriverCapabilities::OPERATION_MASK,
        condition_mask: (1 << 25) - 1,
        features: 0,
    };
    assert!(
        matches!(
            engine.prepare_driver_policy(&caps),
            Err(Error::Unsupported(_))
        ),
        "backend actor+parent termination cannot silently become block-only"
    );
    caps.features = DriverFeature::TerminateActor as u32 | DriverFeature::TerminateParent as u32;
    let (image, lease) = engine
        .prepare_driver_policy(&caps)
        .expect("actual declared actions");
    assert_eq!(image.rule_version, lease.version());
    assert!(image.immutable_buffer.len() > 32);
    assert_eq!(
        u32::from_le_bytes(image.immutable_buffer[0..4].try_into().expect("header")),
        32,
    );
    assert_eq!(
        u32::from_le_bytes(image.immutable_buffer[4..8].try_into().expect("count")),
        5,
        "only the five persisted maintenance rules occupy the driver pools"
    );
    assert_eq!(&image.immutable_buffer[28..32], &1u32.to_le_bytes());
    for at in [16, 20, 24] {
        assert_eq!(
            u32::from_le_bytes(
                image.immutable_buffer[at..at + 4]
                    .try_into()
                    .expect("backend operations")
            ),
            if at == 24 { 0 } else { 1 }
        );
    }
}
#[tokio::test]
async fn native_routing_only_includes_enabled_backend_operations() {
    use as_types::driver_control::DriverCapabilities;
    use as_types::event_operation::NetworkOperation;
    let mut doc = document();
    let RuleGroup::Process(process) = &mut doc.groups[0] else {
        panic!("process")
    };
    process.enabled = false;
    let RuleGroup::File(file) = &mut doc.groups[1] else {
        panic!("file")
    };
    file.rules[0].enabled = false;
    let RuleGroup::Network(network) = &mut doc.groups[2] else {
        panic!("network")
    };
    network.execution = RuleExecution::Backend {
        stage: BackendStage::Remembered,
    };
    network.rules[0].operations = vec![NetworkOperation::Send];
    let caps = DriverCapabilities {
        max_frame_bytes: 16384,
        max_pending: 64,
        max_transfer_bytes: 4194304,
        max_hold_ms: 300000,
        operation_mask: DriverCapabilities::OPERATION_MASK,
        condition_mask: (1 << 25) - 1,
        features: 0,
    };
    for (document, expected_routes, expected_driver_count) in [
        (PolicyDocument::default(), [0, 0, 0], 5),
        (doc.clone(), [0, 0, 0], 5),
        (
            {
                let RuleGroup::Network(network) = &mut doc.groups[2] else {
                    panic!("network")
                };
                network.execution = RuleExecution::Driver {};
                network.number = 2.try_into().expect("ordinary driver group");
                doc
            },
            [0, 0, 0],
            6,
        ),
    ] {
        let (interaction, _) = interaction();
        let (_root, _context, engine) = load(document, interaction).await;
        let (image, _lease) = engine
            .prepare_driver_policy(&caps)
            .expect("current projection");
        assert_eq!(
            u32::from_le_bytes(image.immutable_buffer[4..8].try_into().expect("rule count")),
            expected_driver_count
        );
        for (index, expected) in expected_routes.into_iter().enumerate() {
            let at = 16 + index * 4;
            assert_eq!(
                u32::from_le_bytes(
                    image.immutable_buffer[at..at + 4]
                        .try_into()
                        .expect("route")
                ),
                expected
            );
        }
    }
}

#[tokio::test]
async fn native_file_routes_preserve_exact_path_without_exporting_a_rule_hit() {
    use as_types::driver_control::{DriverCapabilities, DriverFeature};
    let mut doc = document();
    doc.groups.retain(|g| matches!(g, RuleGroup::File(_)));
    let RuleGroup::File(file) = &mut doc.groups[0] else {
        panic!("file")
    };
    file.execution = RuleExecution::Backend {
        stage: BackendStage::Fast,
    };
    file.rules[0].target.path = Some(TextMatch::Exact("/private/key".into()));
    file.rules[0].action = Action::Deny {
        terminate: Termination::None,
    };
    file.rules[0].scope = Scope::Containers {
        instances: vec![ContainerInstance {
            runtime: "docker".into(),
            namespace: None,
            id: "fixture-container".into(),
        }],
    };
    let (interaction, _) = interaction();
    let (_root, _context, engine) = load(doc, interaction).await;
    let caps = DriverCapabilities {
        max_frame_bytes: 16384,
        max_pending: 64,
        max_transfer_bytes: 4194304,
        max_hold_ms: 300000,
        operation_mask: DriverCapabilities::OPERATION_MASK,
        condition_mask: (1 << 25) - 1,
        features: DriverFeature::ContainerBinding as u32
            | DriverFeature::TerminateActor as u32
            | DriverFeature::TerminateParent as u32,
    };
    let (image, _) = engine.prepare_driver_policy(&caps).expect("file route");
    let bytes = &image.immutable_buffer;
    let word = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().expect("image word"));
    assert_eq!(
        word(20),
        0,
        "only projected file rule must not set the unfiltered mask"
    );
    assert_eq!(word(28), 1);
    let mut at = 32;
    for _ in 0..word(4) {
        at += word(at) as usize;
    }
    assert_eq!(word(at), 2);
    assert_eq!(word(at + 4), 1);
    assert_eq!(&bytes[at + 8..at + 12], &[15, 0, 1, 0]);
    assert_eq!(&bytes[at + 24..], b"/private/key");
}
#[tokio::test]
async fn native_file_routes_prioritize_path_then_mask_and_preserve_missing_facts() {
    use as_types::driver_control::{DriverCapabilities, DriverCondition};
    for path in [
        None,
        Some(TextMatch::Exact("/private/key".into())),
        Some(TextMatch::Prefix("/private/".into())),
        Some(TextMatch::Regex("^/private/".into())),
    ] {
        let mut doc = document();
        doc.groups
            .retain(|group| matches!(group, RuleGroup::File(_)));
        let RuleGroup::File(file) = &mut doc.groups[0] else {
            panic!("file")
        };
        file.rules[0].target.path = path.clone();
        file.rules[0].target.access_mask = Some(2);
        let (port, _) = interaction();
        let (_root, _context, engine) = load(doc, port).await;
        let mut caps = DriverCapabilities {
            max_frame_bytes: 16384,
            max_pending: 64,
            max_transfer_bytes: 4194304,
            max_hold_ms: 300000,
            operation_mask: DriverCapabilities::OPERATION_MASK,
            condition_mask: (1 << 25) - 1,
            features: 0,
        };
        let (image, _) = engine
            .prepare_driver_policy(&caps)
            .expect("necessary route");
        let b = &image.immutable_buffer;
        let word = |at| u32::from_le_bytes(b[at..at + 4].try_into().expect("word"));
        let mut at = 32;
        for _ in 0..word(4) {
            at += word(at) as usize;
        }
        assert_eq!(&b[20..24], &0u32.to_le_bytes());
        assert_eq!(&b[28..32], &1u32.to_le_bytes());
        assert_eq!(&b[at..at + 8], &[2, 0, 0, 0, 1, 0, 0, 0]);
        let projected_path = match &path {
            Some(TextMatch::Exact(value)) => Some((1, value.as_bytes())),
            Some(TextMatch::Prefix(value)) => Some((2, value.as_bytes())),
            _ => None,
        };
        if let Some((operator, value)) = projected_path {
            assert_eq!(&b[at + 8..at + 12], &[15, 0, operator, 0]);
            assert_eq!(word(at + 12), value.len() as u32);
            assert_eq!(&b[at + 24..], value);
        } else {
            assert_eq!(&b[at + 8..at + 12], &[19, 0, 5, 0]);
            assert_eq!(&b[at + 12..at + 16], &4u32.to_le_bytes());
            assert_eq!(&b[at + 24..], &2u32.to_le_bytes());
        }

        let snapshot = engine.active.load();
        let rule = &snapshot.file.0.rules[0];
        for (actual, expected) in [(Some(1), false), (Some(2), true), (Some(3), true)] {
            let mut event = event(2);
            let EventPayload::File(file) = &mut event.payload else {
                panic!("file")
            };
            file.access_mask = actual;
            assert!(
                matches!(rule.matches(&event, false, None), Ok(compile::Match::Yes)) == expected
            );
        }
        let missing = event(2);
        assert!(matches!(
            rule.matches(&missing, false, None),
            Ok(compile::Match::Missing)
        ));
        assert!(matches!(
            rule.matches(&missing, true, None),
            Err(ModuleError::Unavailable)
        ));

        caps.condition_mask &= !(1 << (DriverCondition::AccessMask as u16 - 1));
        let (image, _) = engine
            .prepare_driver_policy(&caps)
            .expect("candidate fallback");
        if let Some((operator, _)) = projected_path {
            assert_eq!(&image.immutable_buffer[20..24], &0u32.to_le_bytes());
            assert_eq!(
                &image.immutable_buffer[at + 8..at + 12],
                &[15, 0, operator, 0]
            );
        } else {
            assert_eq!(&image.immutable_buffer[20..24], &1u32.to_le_bytes());
            assert_eq!(&image.immutable_buffer[28..32], &0u32.to_le_bytes());
        }
    }
}

#[tokio::test]
async fn backend_target_conditions_require_a_real_target_field_source() {
    use as_types::driver_control::DriverCapabilities;
    let mut doc = PolicyDocument::default();
    doc.groups.push(
        serde_saphyr::from_str(
            r#"kind: network
id: no-domain-source
number: 1
enabled: true
execution: {location: backend, stage: fast}
rules:
- id: domain
  number: 1
  enabled: true
  scope: {kind: all_protected}
  operations: [send]
  target: {domain: {operator: exact, value: example.test}}
  action: {type: allow}
  log: false
"#,
        )
        .expect("typed target"),
    );
    let (interaction, _) = interaction();
    let (_root, _context, engine) = load(doc, interaction).await;
    let mut caps = DriverCapabilities {
        max_frame_bytes: 16384,
        max_pending: 64,
        max_transfer_bytes: 4194304,
        max_hold_ms: 300000,
        operation_mask: DriverCapabilities::OPERATION_MASK,
        condition_mask: [
            as_types::driver_control::DriverCondition::Executable,
            as_types::driver_control::DriverCondition::TargetExecutable,
            as_types::driver_control::DriverCondition::FilePath,
            as_types::driver_control::DriverCondition::RemotePort,
            as_types::driver_control::DriverCondition::Protocol,
            as_types::driver_control::DriverCondition::MountEffect,
        ]
        .into_iter()
        .fold(0, |mask, field| mask | (1 << (field as u16 - 1))),
        features: as_types::driver_control::DriverFeature::SelfMountRules as u32,
    };
    assert!(
        matches!(
            engine.prepare_driver_policy(&caps),
            Err(Error::Unsupported(_))
        ),
        "backend field cannot be accepted just because matcher supports it"
    );
    caps.features |= as_types::driver_control::DriverFeature::DnsQuerySnapshot as u32;
    engine
        .prepare_driver_policy(&caps)
        .expect("the declared DNS snapshot source supplies the backend condition without native domain matching");
}

#[tokio::test]
async fn dns_snapshot_domain_uses_the_existing_network_fast_and_review_paths() {
    use as_types::dispatch_event::{DnsQueryAnalysis, DnsSendSnapshot};
    use as_types::driver_control::{DriverCapabilities, DriverFeature};
    use as_types::event_operation::NetworkOperation;
    let mut doc = document();
    doc.groups
        .retain(|group| matches!(group, RuleGroup::Network(_)));
    let RuleGroup::Network(group) = &mut doc.groups[0] else {
        panic!("network")
    };
    group.rules[0].operations = vec![NetworkOperation::Send];
    group.rules[0].target = NetworkTarget {
        domain: Some(TextMatch::Exact("mixed.example".into())),
        ..Default::default()
    };
    group.rules[0].action = Action::Deny {
        terminate: Termination::None,
    };
    let (port, mut prompts) = interaction();
    let _channel = port.connect().expect("same review channel");
    let (_root, _context, engine) = load(doc.clone(), port).await;
    let mut input = event_operation(3, 2);
    let EventPayload::Network(network) = &mut input.payload else {
        panic!("network")
    };
    network.protocol = Some(17);
    network.remote_port = Some(53);
    network.target_domain = Some(b"MiXeD.Example".to_vec());
    let data =
        b"\x12\x34\x01\x00\x00\x01\x00\x00\x00\x00\x00\x00\x05MiXeD\x07Example\x00\x00\x01\x00\x01"
            .to_vec();
    network.dns_snapshot = Some(DnsSendSnapshot {
        requested_bytes: data.len() as u64,
        attempted_bytes: data.len() as u32,
        send_flags: 0,
        data,
        analysis: Some(DnsQueryAnalysis::Question {
            qtype: 1,
            qclass: 1,
        }),
    });
    assert_eq!(
        expect_direct(
            engine
                .select(EventKind::Network)
                .fast(&input)
                .expect("DNS fast")
        )
        .decision,
        Decision::Deny {
            terminate: Termination::None
        }
    );
    let mut unrelated = input.clone();
    let EventPayload::Network(network) = &mut unrelated.payload else {
        panic!("network")
    };
    network.dns_snapshot = None;
    network.protocol = None;
    network.remote_port = None;
    assert!(
        matches!(
            engine.select(EventKind::Network).fast(&unrelated),
            Ok(FastMatch::NoReviewMatch)
        ),
        "a reserved name without a DNS source does not turn ordinary traffic into Missing"
    );
    let mut response = input.clone();
    let EventPayload::Network(network) = &mut response.payload else {
        panic!("network")
    };
    network.dns_snapshot.as_mut().expect("snapshot").analysis = Some(DnsQueryAnalysis::Response);
    assert!(matches!(
        engine.select(EventKind::Network).fast(&response),
        Ok(FastMatch::NoReviewMatch)
    ));
    let caps = DriverCapabilities {
        max_frame_bytes: 16384,
        max_pending: 64,
        max_transfer_bytes: 4194304,
        max_hold_ms: 300000,
        operation_mask: DriverCapabilities::OPERATION_MASK,
        condition_mask: ((1 << 26) - 1) & !(1 << 24),
        features: DriverFeature::DnsQuerySnapshot as u32 | DriverFeature::SelfMountRules as u32,
    };
    let (image, _) = engine
        .prepare_driver_policy(&caps)
        .expect("DNS source, no native field25");
    assert_eq!(&image.immutable_buffer[24..28], &0u32.to_le_bytes());
    let mut at = 32;
    let word =
        |at| u32::from_le_bytes(image.immutable_buffer[at..at + 4].try_into().expect("word"));
    for _ in 0..word(4) {
        at += word(at) as usize;
    }
    assert_eq!(&image.immutable_buffer[at + 8..at + 12], &[23, 0, 7, 0]);
    assert_eq!(&image.immutable_buffer[at + 24..at + 28], &[53, 0, 53, 0]);
    let RuleGroup::Network(group) = &mut doc.groups[0] else {
        panic!("network")
    };
    group.rules[0].action = Action::Allow;
    engine
        .update(engine.snapshot().version, doc.clone())
        .await
        .expect("same rule owner");
    assert_eq!(
        expect_direct(
            engine
                .select(EventKind::Network)
                .fast(&input)
                .expect("DNS allow")
        )
        .decision,
        Decision::Allow
    );
    let RuleGroup::Network(group) = &mut doc.groups[0] else {
        panic!("network")
    };
    group.execution = RuleExecution::Backend {
        stage: BackendStage::Ask,
    };
    group.rules[0].action = Action::Ask {
        timeout_ms: 1000.try_into().expect("timeout"),
        timeout: Decision::Deny {
            terminate: Termination::None,
        },
    };
    engine
        .update(engine.snapshot().version, doc.clone())
        .await
        .expect("same Ask pool");
    let rules = engine.select(EventKind::Network);
    let matched = std::sync::OnceLock::new();
    let wait = rules.prompt(request(&input, Duration::from_secs(2), &matched));
    tokio::pin!(wait);
    tokio::select! {result=&mut wait=>panic!("returned before original Ask: {result:?}"),prompt=prompts.recv()=>{
        let prompt=prompt.expect("same Prompt");assert_eq!(prompt.event.as_ref(),&input);
        prompt.reply.send(Decision::Allow).expect("original waiter");
    }}
    assert_eq!(
        wait.await.expect("answer").expect("choice").reason,
        ChoiceReason::User
    );
    let RuleGroup::Network(mut driver) = doc.groups[0].clone() else {
        panic!("network")
    };
    driver.id = "dns-driver-source".into();
    driver.number = 20.try_into().expect("group");
    driver.execution = RuleExecution::Driver {};
    driver.rules[0].id = "dns-driver-action".into();
    driver.rules[0].action = Action::Allow;
    driver.rules[0].target = NetworkTarget {
        protocol: Some(17),
        remote_port: Some(PortRange {
            first: 53,
            last: 53,
        }),
        ..Default::default()
    };
    let mut priority = doc.clone();
    priority.groups.push(RuleGroup::Network(driver.clone()));
    assert!(matches!(
        DriverPolicy::compile(&priority)
            .expect("formal Driver rules")
            .evaluate(&input)
            .expect("Driver match")
            .action,
        Action::Allow
    ));
    driver.rules[0].action = Action::ToUser;
    priority.groups.pop();
    priority.groups.push(RuleGroup::Network(driver));
    assert!(matches!(
        DriverPolicy::compile(&priority)
            .expect("explicit transfer rule")
            .evaluate(&input)
            .expect("Driver transfer")
            .action,
        Action::ToUser
    ));
    let RuleGroup::Network(group) = &mut doc.groups[0] else {
        panic!("network")
    };
    group.rules[0].operations = vec![NetworkOperation::Connect];
    assert!(matches!(
        DriverPolicy::compile(&doc),
        Err(Error::Unsupported(_))
    ));
}

#[test]
fn actual_rule_yaml_rejects_misspelled_text_condition_fields() {
    let mut policy = document();
    let RuleGroup::Process(group) = &mut policy.groups[0] else {
        panic!("process group")
    };
    group.rules[0].source.executable = Some(TextMatch::Exact("/bin/worker".into()));
    // Serialize the actual YAML owner type. JSON arbitrary-precision numbers have
    // a private serde representation that is not a YAML rule-field encoding.
    let known = serde_saphyr::to_string(&policy).expect("valid YAML");
    assert!(serde_saphyr::from_str::<PolicyDocument>(&known).is_ok());
    let marker = "value: /bin/worker";
    let at = known.find(marker).expect("actual condition value");
    let indentation = known[..at].rsplit('\n').next().expect("value line").len();
    let unknown = known.replacen(
        marker,
        &format!("{marker}\n{}typo: true", " ".repeat(indentation)),
        1,
    );
    assert!(
        serde_saphyr::from_str::<PolicyDocument>(&unknown).is_err(),
        "unknown condition fields must never be silently ignored"
    );
}

#[tokio::test]
async fn active_backend_container_scope_requires_a_real_binding_source() {
    use as_types::driver_control::{DriverCapabilities, DriverFeature};
    let (interaction, _) = interaction();
    let mut doc = document();
    let RuleGroup::Process(group) = &mut doc.groups[0] else {
        panic!("process group")
    };
    group.rules[0].source = ProcessMatch::default();
    group.rules[0].action = Action::Allow;
    group.rules[0].scope = Scope::Containers {
        instances: vec![ContainerInstance {
            runtime: "docker".into(),
            namespace: None,
            id: "specific-instance".into(),
        }],
    };
    let (_root, _context, engine) = load(doc.clone(), interaction).await;
    let mut caps = DriverCapabilities {
        max_frame_bytes: 16384,
        max_pending: 64,
        max_transfer_bytes: 4 * 1024 * 1024,
        max_hold_ms: 300000,
        operation_mask: DriverCapabilities::OPERATION_MASK,
        condition_mask: (1 << 25) - 1,
        features: 0,
    };
    assert!(matches!(
        engine.prepare_driver_policy(&caps),
        Err(Error::Unsupported(
            "exact container-instance scope has no active source"
        ))
    ));
    // This verifies the compiler's capability input, not native container enforcement.
    caps.features = DriverFeature::ContainerBinding as u32;
    assert!(engine.prepare_driver_policy(&caps).is_ok());
    caps.features = 0;
    let RuleGroup::Process(group) = &mut doc.groups[0] else {
        panic!("process group")
    };
    group.rules[0].enabled = false;
    engine
        .update(engine.snapshot().version, doc.clone())
        .await
        .expect("disabled rule saved");
    assert!(engine.prepare_driver_policy(&caps).is_ok());
    let RuleGroup::Process(group) = &mut doc.groups[0] else {
        panic!("process group")
    };
    group.rules[0].enabled = true;
    group.enabled = false;
    engine
        .update(engine.snapshot().version, doc)
        .await
        .expect("disabled group saved");
    assert!(engine.prepare_driver_policy(&caps).is_ok());
}

#[tokio::test]
async fn fixed_denial_text_preserves_empty_whitespace_and_original_lease_after_update() {
    let mut doc = document();
    let RuleGroup::Process(process) = &mut doc.groups[0] else {
        panic!("process")
    };
    assert_eq!(
        process.rules[0].deny_message, "",
        "omitted text is a retrieved empty string"
    );
    process.rules[0].deny_message = "  固定拒绝\n原文  ".into();
    let RuleGroup::File(file) = &mut doc.groups[1] else {
        panic!("file")
    };
    file.rules[0].deny_message = "Ask 原文".into();
    let (port, _) = interaction();
    let (_workspace, _context, engine) = load(doc.clone(), port).await;
    let old = engine.pin();
    let choice = expect_direct(
        engine
            .select(EventKind::Process)
            .fast(&event(1))
            .expect("fast"),
    );
    let original = choice.hit.expect("actual hit");
    assert_eq!(original.deny_message.as_deref(), Some("  固定拒绝\n原文  "));
    assert_eq!(
        old.0.file.0.rules[0].hit.deny_message.as_deref(),
        Some("Ask 原文")
    );
    assert_eq!(
        old.0.network.0.rules[0].hit.deny_message.as_deref(),
        Some("")
    );
    let mut native_hit = original.clone();
    native_hit.deny_message = None;
    assert_eq!(
        old.resolve_hit_feedback(&native_hit)
            .expect("original snapshot")
            .deny_message,
        original.deny_message
    );
    let RuleGroup::Process(process) = &mut doc.groups[0] else {
        panic!("process")
    };
    process.rules[0].deny_message = "新的同名规则".into();
    let updated = engine
        .update(old.version(), doc)
        .await
        .expect("save new text");
    assert_ne!(
        updated.saved_version,
        old.version(),
        "text participates in the complete rule version"
    );
    assert_eq!(
        old.resolve_hit_feedback(&native_hit)
            .expect("retained old version")
            .deny_message
            .as_deref(),
        Some("  固定拒绝\n原文  ")
    );
    assert!(
        engine.pin().resolve_hit_feedback(&native_hit).is_err(),
        "current same-name rule cannot resolve an old hit"
    );
    let current = expect_direct(
        engine
            .select(EventKind::Process)
            .fast(&event(1))
            .expect("new fast"),
    );
    assert_eq!(
        current.hit.expect("new hit").deny_message.as_deref(),
        Some("新的同名规则")
    );
    for (index, mut forged) in [
        native_hit.clone(),
        native_hit.clone(),
        native_hit.clone(),
        native_hit.clone(),
        native_hit.clone(),
        native_hit.clone(),
        native_hit.clone(),
    ]
    .into_iter()
    .enumerate()
    {
        match index {
            0 => forged.group_id = "wrong-group".into(),
            1 => forged.rule_id = "wrong-rule".into(),
            2 => forged.group_number = std::num::NonZeroU32::new(2).expect("order"),
            3 => forged.rule_number = std::num::NonZeroU32::new(2).expect("order"),
            4 => forged.log = !forged.log,
            5 => {
                forged.source = RuleSourceVersion::Configuration(
                    as_types::configuration::DriverConfigVersion([0; 32]),
                )
            }
            _ => forged.deny_message = Some("forged text".into()),
        }
        assert!(old.resolve_hit_feedback(&forged).is_err());
    }
}

#[test]
fn denial_text_rejects_allow_and_transfer_even_when_disabled() {
    for (execution, action) in [
        (
            RuleExecution::Backend {
                stage: BackendStage::Fast,
            },
            Action::Allow,
        ),
        (RuleExecution::Driver {}, Action::ToUser),
    ] {
        let mut doc = document();
        let RuleGroup::Process(group) = &mut doc.groups[0] else {
            panic!("process")
        };
        group.execution = execution;
        group.rules[0].source.executable = Some(TextMatch::Exact("/usr/bin/worker".into()));
        group.enabled = false;
        group.rules[0].action = action;
        group.rules[0].deny_message = " ".into();
        let (port, _) = interaction();
        assert!(
            build(doc.clone(), port, ScopeSource::default()).is_err(),
            "nonempty whitespace is still configured text"
        );
        let RuleGroup::Process(group) = &mut doc.groups[0] else {
            panic!("process")
        };
        group.rules[0].deny_message.clear();
        let (port, _) = interaction();
        assert!(build(doc, port, ScopeSource::default()).is_ok());
    }
}

#[tokio::test]
async fn native_denial_feedback_flag_is_independent_of_log_and_does_not_export_text() {
    use as_types::driver_control::DriverCapabilities;
    let mut doc = document();
    let RuleGroup::Process(group) = &mut doc.groups[0] else {
        panic!("process")
    };
    group.execution = RuleExecution::Driver {};
    group.number = 2.try_into().expect("ordinary driver group");
    group.rules[0].source.executable = Some(TextMatch::Exact("/usr/bin/worker".into()));
    group.rules[0].action = Action::Deny {
        terminate: Termination::None,
    };
    group.rules[0].log = false;
    group.rules[0].deny_message = "not-in-native-image".into();
    let (port, _) = interaction();
    let (_workspace, _context, engine) = load(doc.clone(), port).await;
    let caps = DriverCapabilities {
        max_frame_bytes: 16384,
        max_pending: 64,
        max_transfer_bytes: 4194304,
        max_hold_ms: 300000,
        operation_mask: DriverCapabilities::OPERATION_MASK,
        condition_mask: (1 << 25) - 1,
        features: 0,
    };
    let (image, lease) = engine
        .prepare_driver_policy(&caps)
        .expect("native projection");
    let body = native_rule(&image, 1, 2);
    assert_eq!(&body[24..28], &0u32.to_le_bytes(), "log stays false");
    assert_eq!(
        &body[36..40],
        &1u32.to_le_bytes(),
        "actual denial feedback requested"
    );
    assert!(
        !image
            .immutable_buffer
            .windows(b"not-in-native-image".len())
            .any(|v| v == b"not-in-native-image"),
        "Native receives no message bytes"
    );
    let hit = lease
        .0
        .driver
        .process
        .iter()
        .find(|r| r.hit.rule_id.as_str() == "deny-process")
        .expect("original driver rule")
        .hit
        .clone();
    assert_eq!(
        lease
            .resolve_hit_feedback(&hit)
            .expect("driver original feedback")
            .deny_message
            .as_deref(),
        Some("not-in-native-image")
    );
    for (action, text) in [
        (
            Action::Deny {
                terminate: Termination::None,
            },
            "",
        ),
        (Action::Allow, ""),
        (Action::ToUser, ""),
    ] {
        let RuleGroup::Process(group) = &mut doc.groups[0] else {
            panic!("process")
        };
        group.rules[0].action = action;
        group.rules[0].deny_message = text.into();
        let (port, _) = interaction();
        let (_workspace, _context, engine) = load(doc.clone(), port).await;
        let (image, _) = engine.prepare_driver_policy(&caps).expect("plain action");
        assert_eq!(&native_rule(&image, 1, 2)[36..40], &0u32.to_le_bytes());
    }
}

fn native_rule(image: &as_types::driver_image::DriverPolicyImage, kind: u16, group: u32) -> &[u8] {
    let mut rules = &image.immutable_buffer[32..];
    while !rules.is_empty() {
        let length = u32::from_le_bytes(rules[..4].try_into().expect("rule length")) as usize;
        if u16::from_le_bytes(rules[4..6].try_into().expect("kind")) == kind
            && u32::from_le_bytes(rules[16..20].try_into().expect("group")) == group
        {
            return &rules[..length];
        }
        rules = &rules[length..];
    }
    panic!("requested native rule not exported")
}

#[test]
fn impossible_file_target_conditions_are_rejected_for_driver_and_backend_rules() {
    use as_types::event_operation::FileOperation as Op;
    for execution in [
        RuleExecution::Driver {},
        RuleExecution::Backend {
            stage: BackendStage::Remembered,
        },
    ] {
        let mut doc = document();
        doc.groups.retain(|g| matches!(g, RuleGroup::File(_)));
        let RuleGroup::File(group) = &mut doc.groups[0] else {
            panic!("file")
        };
        group.execution = execution;
        group.number = 3.try_into().expect("ordinary group");
        group.rules[0].source = ProcessMatch::default();
        group.rules[0].action = Action::Allow;
        for (operations, target) in [
            (
                vec![Op::Open, Op::Rename],
                FileTarget {
                    new_path: Some(TextMatch::Prefix("/protected/".into())),
                    ..Default::default()
                },
            ),
            (
                vec![Op::Rename],
                FileTarget {
                    access_mask: Some(1),
                    ..Default::default()
                },
            ),
            (
                vec![Op::Unlink, Op::CreateDirectory],
                FileTarget {
                    inode: Some(0),
                    ..Default::default()
                },
            ),
        ] {
            let RuleGroup::File(group) = &mut doc.groups[0] else {
                panic!("file")
            };
            group.rules[0].operations = operations;
            group.rules[0].target = target;
            let (port, _) = interaction();
            assert!(
                matches!(
                    build(doc.clone(), port, ScopeSource::default()),
                    Err(Error::Invalid(_))
                ),
                "no operation in an OR list may lack the target condition's source"
            );
        }
        let RuleGroup::File(group) = &mut doc.groups[0] else {
            panic!("file")
        };
        group.rules[0].target = FileTarget::default();
        group.rules[0].operations = vec![Op::Write];
        let (port, _) = interaction();
        assert!(
            matches!(
                build(doc, port, ScopeSource::default()),
                Err(Error::Unsupported(_))
            ),
            "an unimplemented write is not an ordinary controlled rule"
        );
    }
}

#[tokio::test]
async fn rename_source_capability_and_conditions_survive_real_publication_without_erasing_missing_facts()
 {
    use as_types::{
        driver_control::{DriverCapabilities, DriverCondition},
        event_operation::FileOperation,
    };
    let mut doc = document();
    doc.groups.retain(|g| matches!(g, RuleGroup::File(_)));
    let RuleGroup::File(group) = &mut doc.groups[0] else {
        panic!("file")
    };
    group.execution = RuleExecution::Backend {
        stage: BackendStage::Remembered,
    };
    group.rules[0].source = ProcessMatch::default();
    group.rules[0].action = Action::Allow;
    group.rules[0].operations = vec![FileOperation::Rename];
    group.rules[0].target = FileTarget {
        new_path: Some(TextMatch::Prefix("/protected/".into())),
        ..Default::default()
    };
    let (port, _) = interaction();
    let (_root, _context, engine) = load(doc.clone(), port).await;
    let mut caps = DriverCapabilities {
        max_frame_bytes: 16384,
        max_pending: 64,
        max_transfer_bytes: 4194304,
        max_hold_ms: 300000,
        operation_mask: DriverCapabilities::OPERATION_MASK,
        condition_mask: (1 << 25) - 1,
        features: 0,
    };
    caps.condition_mask &= !(1 << (DriverCondition::NewPath as u16 - 1));
    assert!(matches!(
        engine.prepare_driver_policy(&caps),
        Err(Error::Unsupported(_))
    ));
    caps.condition_mask |= 1 << (DriverCondition::NewPath as u16 - 1);
    let (image, _) = engine
        .prepare_driver_policy(&caps)
        .expect("actual rename source declared");
    assert_eq!(
        &image.immutable_buffer[20..24],
        &0x100u32.to_le_bytes(),
        "one Rename route, no invented Open route"
    );
    caps.operation_mask &= !(1 << 10);
    assert!(matches!(
        engine.prepare_driver_policy(&caps),
        Err(Error::Unsupported(_))
    ));
    caps.operation_mask |= 1 << 10;
    let mut original = event_operation(2, 9);
    assert!(matches!(
        engine.select(EventKind::File).fast(&original),
        Ok(FastMatch::NeedsFacts)
    ));
    if let EventPayload::File(data) = &mut original.payload {
        data.new_path = Some(b"/protected/item".to_vec());
    }
    assert!(matches!(
        engine.select(EventKind::File).fast(&original),
        Ok(FastMatch::Matched(_))
    ));
    let version = engine.snapshot().version;
    let mut invalid = doc.clone();
    let RuleGroup::File(group) = &mut invalid.groups[0] else {
        panic!("file")
    };
    group.rules[0].operations = vec![FileOperation::Open];
    assert!(matches!(
        engine.update(version, invalid).await,
        Err(Error::Invalid(_))
    ));
    assert_eq!(
        engine.snapshot().version,
        version,
        "invalid Open/new_path cannot publish or erase the condition"
    );
    let RuleGroup::File(group) = &mut doc.groups[0] else {
        panic!("file")
    };
    group.execution = RuleExecution::Driver {};
    group.number = 3.try_into().expect("ordinary driver group");
    engine
        .update(version, doc)
        .await
        .expect("same rule moved explicitly to Driver");
    let (image, _) = engine
        .prepare_driver_policy(&caps)
        .expect("actual Driver rename projection");
    let body = native_rule(&image, 2, 3);
    assert_eq!(&body[8..12], &0x100u32.to_le_bytes());
    let group_length = usize::from(u16::from_le_bytes(
        body[32..34].try_into().expect("group length"),
    ));
    let id_length = usize::from(u16::from_le_bytes(
        body[34..36].try_into().expect("rule length"),
    ));
    let condition = 40 + group_length + id_length;
    assert_eq!(
        &body[condition..condition + 2],
        &(DriverCondition::NewPath as u16).to_le_bytes(),
        "the condition is encoded, not dropped"
    );
}

#[tokio::test]
async fn maintenance_is_persisted_and_matches_original_source_or_target_in_all_driver_pools() {
    let (port, _) = interaction();
    let (_root, context, engine) = load(PolicyDocument::default(), port).await;
    assert!(Arc::ptr_eq(context.paths(), &engine.paths));
    let snapshot = engine.snapshot();
    let persisted: PolicyDocument = serde_saphyr::from_str(
        &std::fs::read_to_string(context.paths().rules_file()).expect("saved rules"),
    )
    .expect("canonical typed document");
    assert_eq!(&persisted, snapshot.document.as_ref());
    assert_eq!(
        document_version(&persisted).expect("saved digest"),
        snapshot.version
    );
    assert_eq!(persisted.groups.len(), 3);
    let original_version = snapshot.version;
    let driver = &engine.pin().0.driver;
    let root = context
        .paths()
        .self_protection_root()
        .to_str()
        .expect("UTF-8 fixture");
    for (kind, operation) in [
        (1, 1),
        (2, 1),
        (2, 5),
        (2, 6),
        (2, 7),
        (2, 9),
        (3, 1),
        (3, 2),
    ] {
        let mut original = event_operation(kind, operation);
        if let EventPayload::Process(p) = &mut original.payload {
            p.target_executable = Some(b"/usr/bin/tool".to_vec());
        }
        original.process.executable = Some(format!("{root}/bin/service").into_bytes());
        let matched = driver.evaluate(&original).expect("maintenance match");
        assert!(matches!(matched.action, Action::Allow));
        let hit = matched.hit.expect("visible maintenance hit");
        assert!(!hit.log);
        assert_eq!(hit.deny_message.as_deref(), Some(""));
        assert_eq!(
            hit.source,
            RuleSourceVersion::RulesDocument(original_version)
        );
        original.process.executable = Some(format!("{root}-other/bin/service").into_bytes());
        assert!(
            driver
                .evaluate(&original)
                .expect("directory boundary")
                .hit
                .is_none()
        );
        original.process.executable = Some(b"/usr/bin/external".to_vec());
        assert!(
            driver
                .evaluate(&original)
                .expect("external source")
                .hit
                .is_none()
        );
        match &mut original.payload {
            EventPayload::Process(p) => {
                p.target_executable = Some(format!("{root}/bin/service").into_bytes())
            }
            EventPayload::File(p) => p.path = Some(format!("{root}/config/as.yaml").into_bytes()),
            EventPayload::Network(_) => continue,
        }
        let target = driver
            .evaluate(&original)
            .expect("independent target match");
        if kind == 2 && operation != 1 {
            assert!(
                target.hit.is_none(),
                "external root mutations gain no target maintenance Allow"
            );
            continue;
        }
        assert!(matches!(target.action, Action::Allow));
        assert!(
            target
                .hit
                .expect("target hit")
                .rule_id
                .as_str()
                .starts_with("as-maintenance-target-")
        );
    }
    let caps = as_types::driver_control::DriverCapabilities {
        max_frame_bytes: 16384,
        max_pending: 64,
        max_transfer_bytes: 4194304,
        max_hold_ms: 300000,
        operation_mask: as_types::driver_control::DriverCapabilities::OPERATION_MASK,
        condition_mask: (1 << 25) - 1,
        features: 0,
    };
    let (image, lease) = engine
        .prepare_driver_policy(&caps)
        .expect("same real exporter");
    assert_eq!(image.rule_version, original_version);
    assert_eq!(lease.version(), original_version);
    assert_eq!(
        u32::from_le_bytes(image.immutable_buffer[4..8].try_into().expect("count")),
        5
    );
    for (kind, group) in [(1, 1), (2, 2), (3, 1)] {
        assert_eq!(
            &native_rule(&image, kind, group)[24..28],
            &0u32.to_le_bytes()
        );
    }
    assert_eq!(
        &native_rule(&image, 2, 2)[8..12],
        &0x60371u32.to_le_bytes(),
        "source maintenance exports the eight real operations in the same image"
    );
}

#[tokio::test]
async fn maintenance_survives_user_clear_reload_and_rejects_edits_or_reserved_orders() {
    let (port, _) = interaction();
    let (_root, context, engine) = load(document(), port).await;
    let original = engine.snapshot();
    let mut edited = (*original.document).clone();
    let RuleGroup::File(group) = &mut edited.groups[1] else {
        panic!("maintenance file")
    };
    group.rules[0].log = true;
    assert!(matches!(
        engine.update(original.version, edited).await,
        Err(Error::Invalid(_))
    ));
    assert_eq!(engine.snapshot().version, original.version);
    let mut occupied = document();
    let RuleGroup::Process(group) = &mut occupied.groups[0] else {
        panic!("process")
    };
    group.execution = RuleExecution::Driver {};
    assert!(matches!(
        engine.update(original.version, occupied).await,
        Err(Error::Invalid(_))
    ));
    let cleared = engine
        .update(original.version, PolicyDocument::default())
        .await
        .expect("clear user rules");
    assert_ne!(cleared.saved_version, original.version);
    assert_eq!(engine.snapshot().document.groups.len(), 3);
    assert!(matches!(
        engine
            .update(original.version, PolicyDocument::default())
            .await,
        Err(Error::Conflict)
    ));
    std::fs::write(context.paths().rules_file(), "groups: []\n").expect("reload supplied source");
    let reloaded = engine
        .reload(cleared.saved_version)
        .await
        .expect("normalize reload");
    assert_eq!(reloaded.version, cleared.saved_version);
    let persisted: PolicyDocument = serde_saphyr::from_str(
        &std::fs::read_to_string(context.paths().rules_file()).expect("saved reload"),
    )
    .expect("canonical reloaded source");
    assert_eq!(&persisted, reloaded.document.as_ref());
    let (port, _) = interaction();
    let reopened = PolicyEngine::initialize(&context, port)
        .await
        .expect("canonical reopen");
    assert_eq!(reopened.snapshot().version, reloaded.version);
}

#[tokio::test]
async fn remembered_item_edits_use_the_fixed_document_writer_and_survive_reopen() {
    let (port, _) = interaction();
    let (_workspace, context, engine) = load(PolicyDocument::default(), port).await;
    let RuleGroup::Process(mut group) = document().groups.remove(0) else {
        panic!("process fixture")
    };
    group.id = "remembered-process".into();
    group.execution = RuleExecution::Backend {
        stage: BackendStage::Remembered,
    };
    let mut rule = group.rules.remove(0);
    rule.id = "remembered-execute".into();
    rule.deny_message = "  用户配置的固定拒绝提示\n保留原文  ".into();
    let definition = RuleDefinition::Process(rule.clone());
    engine
        .edit(
            engine.snapshot().version,
            RuleChange::CreateGroup {
                group: Box::new(RuleGroup::Process(group.clone())),
            },
        )
        .await
        .expect("persist explicit remembered group");
    let before_rule = engine.snapshot().version;
    let change = RuleChange::CreateRule {
        group_id: group.id.clone(),
        rule: Box::new(definition.clone()),
    };
    let encoded = serde_json::to_vec(&change).expect("typed command");
    let decoded = serde_json::from_slice(&encoded).expect("same current command");
    let receipt = engine
        .edit(before_rule, decoded)
        .await
        .expect("persist original rule fields");
    let readback = engine.get_rule(&rule.id).expect("single readback");
    assert_eq!(
        readback,
        RuleSnapshot {
            version: receipt.saved_version,
            group_id: group.id.clone(),
            rule: definition.clone()
        }
    );
    assert_ne!(readback.version, before_rule);
    assert!(matches!(
        expect_direct(
            engine
                .select(EventKind::Process)
                .fast(&event(1))
                .expect("remembered match")
        )
        .decision,
        Decision::Deny { .. }
    ));
    rule.action = Action::Allow;
    rule.deny_message.clear();
    let replaced = engine
        .edit(
            readback.version,
            RuleChange::ReplaceRule {
                rule_id: rule.id.clone(),
                rule: Box::new(RuleDefinition::Process(rule.clone())),
            },
        )
        .await
        .expect("replace one remembered rule");
    assert_ne!(replaced.saved_version, readback.version);
    assert_eq!(
        engine.get_rule(&rule.id).expect("new rule").rule,
        RuleDefinition::Process(rule.clone())
    );
    assert_eq!(
        readback.rule, definition,
        "the old read remains an immutable value"
    );
    group.rules.push(rule.clone());
    group.execution = RuleExecution::Backend {
        stage: BackendStage::Fast,
    };
    engine
        .edit(
            replaced.saved_version,
            RuleChange::ReplaceGroup {
                group_id: group.id.clone(),
                group: Box::new(RuleGroup::Process(group.clone())),
            },
        )
        .await
        .expect("explicit placement replacement");
    assert!(
        engine
            .snapshot()
            .document
            .groups
            .contains(&RuleGroup::Process(group.clone()))
    );
    let stored = read(&engine.directory).expect("real fixed source");
    assert_eq!(&stored, engine.snapshot().document.as_ref());
    let before_unchanged = engine.snapshot().version;
    let unchanged = engine
        .edit(
            before_unchanged,
            RuleChange::ReplaceRule {
                rule_id: rule.id.clone(),
                rule: Box::new(RuleDefinition::Process(rule.clone())),
            },
        )
        .await
        .expect("same content");
    assert_eq!(unchanged.saved_version, before_unchanged);
    let original_version = engine.snapshot().version;
    let (port, _) = interaction();
    let reopened = PolicyEngine::initialize(&context, port)
        .await
        .expect("restart reads the same source");
    assert_eq!(
        reopened.get_rule(&rule.id).expect("persisted single rule"),
        engine.get_rule(&rule.id).expect("original owner read")
    );
    reopened
        .edit(
            original_version,
            RuleChange::DeleteRule {
                rule_id: rule.id.clone(),
            },
        )
        .await
        .expect("delete one rule");
    assert!(matches!(reopened.get_rule(&rule.id), Err(Error::NotFound)));
    assert!(
        matches!(
            engine
                .edit(
                    original_version,
                    RuleChange::ReplaceRule {
                        rule_id: rule.id.clone(),
                        rule: Box::new(RuleDefinition::Process(rule))
                    }
                )
                .await,
            Err(Error::Conflict)
        ),
        "the sole writer rechecks actual source version, beyond the first in-memory check"
    );
    reopened
        .edit(
            reopened.snapshot().version,
            RuleChange::DeleteGroup { group_id: group.id },
        )
        .await
        .expect("delete the ordinary empty group");
    assert_eq!(
        read(&reopened.directory)
            .expect("saved deletion")
            .groups
            .len(),
        3
    );
}

fn rename_definition(rule: &mut RuleDefinition, id: &str) {
    match rule {
        RuleDefinition::Process(r) => r.id = id.into(),
        RuleDefinition::File(r) => r.id = id.into(),
        RuleDefinition::Network(r) => r.id = id.into(),
    }
}

#[tokio::test]
async fn item_edits_reject_absent_duplicates_identity_changes_and_all_three_family_mismatches() {
    let (port, _) = interaction();
    let (_workspace, _context, engine) = load(document(), port).await;
    let original = engine.snapshot();
    let saved = read(&engine.directory).expect("before rejected edits");
    for (group, id, other) in [
        ("process-fast", "deny-process", "ask-file"),
        ("file-ask", "ask-file", "allow-net"),
        ("network-fast", "allow-net", "deny-process"),
    ] {
        let mut wrong = engine.get_rule(other).expect("other family").rule;
        rename_definition(&mut wrong, "new-wrong-family");
        assert!(matches!(
            engine
                .edit(
                    original.version,
                    RuleChange::CreateRule {
                        group_id: group.into(),
                        rule: Box::new(wrong.clone())
                    }
                )
                .await,
            Err(Error::Invalid(_))
        ));
        rename_definition(&mut wrong, id);
        assert!(matches!(
            engine
                .edit(
                    original.version,
                    RuleChange::ReplaceRule {
                        rule_id: id.into(),
                        rule: Box::new(wrong)
                    }
                )
                .await,
            Err(Error::Invalid(_))
        ));
    }
    let rule = engine.get_rule("deny-process").expect("existing rule").rule;
    assert!(matches!(
        engine
            .edit(
                original.version,
                RuleChange::CreateRule {
                    group_id: "process-fast".into(),
                    rule: Box::new(rule.clone())
                }
            )
            .await,
        Err(Error::Invalid(_))
    ));
    let mut renamed = rule;
    rename_definition(&mut renamed, "renamed-rule");
    assert!(matches!(
        engine
            .edit(
                original.version,
                RuleChange::ReplaceRule {
                    rule_id: "deny-process".into(),
                    rule: Box::new(renamed)
                }
            )
            .await,
        Err(Error::Invalid(_))
    ));
    let user_group = saved
        .groups
        .iter()
        .find(|g| matches!(g,RuleGroup::Process(g)if g.id=="process-fast"))
        .expect("user group")
        .clone();
    assert!(matches!(
        engine
            .edit(
                original.version,
                RuleChange::CreateGroup {
                    group: Box::new(user_group.clone())
                }
            )
            .await,
        Err(Error::Invalid(_))
    ));
    let RuleGroup::Process(mut renamed_group) = user_group else {
        panic!("process")
    };
    renamed_group.id = "renamed-group".into();
    assert!(matches!(
        engine
            .edit(
                original.version,
                RuleChange::ReplaceGroup {
                    group_id: "process-fast".into(),
                    group: Box::new(RuleGroup::Process(renamed_group))
                }
            )
            .await,
        Err(Error::Invalid(_))
    ));
    let RuleGroup::File(mut wrong_group) = document().groups.remove(1) else {
        panic!("file")
    };
    wrong_group.id = "process-fast".into();
    assert!(matches!(
        engine
            .edit(
                original.version,
                RuleChange::ReplaceGroup {
                    group_id: "process-fast".into(),
                    group: Box::new(RuleGroup::File(wrong_group))
                }
            )
            .await,
        Err(Error::Invalid(_))
    ));
    assert!(matches!(engine.get_rule("absent"), Err(Error::NotFound)));
    for change in [
        RuleChange::DeleteRule {
            rule_id: "absent".into(),
        },
        RuleChange::DeleteGroup {
            group_id: "absent".into(),
        },
    ] {
        assert!(matches!(
            engine.edit(original.version, change).await,
            Err(Error::NotFound)
        ));
    }
    assert_eq!(engine.snapshot().version, original.version);
    assert_eq!(read(&engine.directory).expect("unchanged source"), saved);
}

#[tokio::test]
async fn every_targeted_maintenance_edit_is_invalid_instead_of_silently_normalized_back() {
    let (port, _) = interaction();
    let (_workspace, _context, engine) = load(PolicyDocument::default(), port).await;
    let original = engine.snapshot();
    let managed = engine
        .get_rule("as-maintenance-source-execute")
        .expect("managed rule is readable");
    let group = original.document.groups[0].clone();
    for change in [
        RuleChange::CreateGroup {
            group: Box::new(group.clone()),
        },
        RuleChange::ReplaceGroup {
            group_id: managed.group_id.clone(),
            group: Box::new(group),
        },
        RuleChange::DeleteGroup {
            group_id: managed.group_id.clone(),
        },
        RuleChange::CreateRule {
            group_id: managed.group_id.clone(),
            rule: Box::new(managed.rule.clone()),
        },
        RuleChange::ReplaceRule {
            rule_id: "as-maintenance-source-execute".into(),
            rule: Box::new(managed.rule.clone()),
        },
        RuleChange::DeleteRule {
            rule_id: "as-maintenance-source-execute".into(),
        },
    ] {
        assert!(matches!(
            engine.edit(original.version, change).await,
            Err(Error::Invalid(_))
        ));
    }
    assert_eq!(engine.snapshot().version, original.version);
    assert_eq!(
        read(&engine.directory).expect("maintenance unchanged"),
        *original.document
    );
}

#[tokio::test]
async fn fast_allow_preset_uses_fixed_source_publication_and_native_export_without_widening_paths()
{
    use as_types::{
        driver_control::{DriverCapabilities, DriverFeature},
        event_operation::FileOperation,
    };
    let preset_text = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../config/rules.fast-allow.yaml"
    ));
    let preset: PolicyDocument = serde_saphyr::from_str(preset_text).expect("actual preset YAML");
    let (_root, context) = source(&PolicyDocument::default());
    std::fs::write(context.paths().rules_file(), preset_text)
        .expect("install original preset at fixed source");
    let (port, _) = interaction();
    let engine = PolicyEngine::initialize(&context, port)
        .await
        .expect("actual fixed-source load and normalization");
    let mut candidate = document();
    candidate.groups.extend(preset.groups.clone());
    let receipt = engine
        .update(engine.snapshot().version, candidate)
        .await
        .expect("real save and publication");
    let reloaded = engine
        .reload(receipt.saved_version)
        .await
        .expect("actual fixed-source reload");
    assert_eq!(reloaded.version, receipt.saved_version);
    let saved: PolicyDocument = serde_saphyr::from_str(
        &std::fs::read_to_string(context.paths().rules_file()).expect("saved original rules"),
    )
    .expect("saved typed document");
    assert_eq!(saved, *reloaded.document);
    let (port, _) = interaction();
    let _ready = port.connect().expect("available original review channel");
    let reopened = PolicyEngine::initialize(&context, port)
        .await
        .expect("reopen the published source");
    assert_eq!(reopened.snapshot().version, reloaded.version);
    let caps = DriverCapabilities {
        max_frame_bytes: 16384,
        max_pending: 64,
        max_transfer_bytes: 4194304,
        max_hold_ms: 300000,
        operation_mask: DriverCapabilities::OPERATION_MASK,
        condition_mask: (1 << 26) - 1,
        features: DriverFeature::TerminateActor as u32
            | DriverFeature::TerminateParent as u32
            | DriverFeature::SelfMountRules as u32,
    };
    let (image, lease) = reopened
        .prepare_driver_policy(&caps)
        .expect("real capability validation and native image");
    assert_eq!(image.rule_version, receipt.saved_version);
    assert_eq!(lease.version(), receipt.saved_version);
    assert_eq!(
        u32::from_le_bytes(
            image.immutable_buffer[4..8]
                .try_into()
                .expect("native count")
        ),
        20
    );
    assert_eq!(
        &image.immutable_buffer[20..24],
        &1_u32.to_le_bytes(),
        "ordinary private-file backend routing remains enabled"
    );
    for group in [3, 4] {
        let encoded = native_rule(&image, 2, group);
        assert_eq!(&encoded[6..8], &0_u16.to_le_bytes(), "real Allow encoding");
        assert_eq!(
            &encoded[24..28],
            &0_u32.to_le_bytes(),
            "no per-hit log request"
        );
    }
    let driver = DriverPolicy::compile(&saved).expect("same ordered matcher");
    let mut unsupported = saved.clone();
    let RuleGroup::File(mount_group) = unsupported.groups.iter_mut().find(|group|
        matches!(group, RuleGroup::File(group) if group.id == "linux-self-protection-independent-mounts")
    ).expect("configured maintenance") else { panic!("file"); };
    mount_group.rules[0].target.mount_effect = Some(MountEffect::ProtectedTree);
    assert!(
        DriverPolicy::compile(&unsupported).is_err(),
        "ordinary Allow cannot target the actual self-protection tree"
    );
    let RuleGroup::File(mount_group) = unsupported.groups.iter_mut().find(|group|
        matches!(group, RuleGroup::File(group) if group.id == "linux-self-protection-independent-mounts")
    ).expect("configured maintenance") else { panic!("file"); };
    mount_group.rules[0].target.mount_effect = Some(MountEffect::IndependentView);
    mount_group.rules[0].action = Action::ToUser;
    assert!(
        DriverPolicy::compile(&unsupported).is_err(),
        "direct mount support does not manufacture a waitable operation"
    );
    for (path, masks) in [
        ("/dev/null", &[1, 2, 3][..]),
        ("/dev/zero", &[1, 2, 3][..]),
        ("/dev/random", &[1, 2, 3][..]),
        ("/dev/urandom", &[1, 2, 3][..]),
        ("/proc/cpuinfo", &[1, 3][..]),
        ("/proc/meminfo", &[1, 3][..]),
        ("/proc/uptime", &[1, 3][..]),
        ("/proc/version", &[1, 3][..]),
        ("/proc/loadavg", &[1, 3][..]),
        ("/proc/stat", &[1, 3][..]),
    ] {
        assert!(
            image
                .immutable_buffer
                .windows(path.len())
                .any(|bytes| bytes == path.as_bytes()),
            "original exact target retained in image"
        );
        for &mask in masks {
            let mut input = event(2);
            let EventPayload::File(file) = &mut input.payload else {
                panic!("file fixture");
            };
            file.path = Some(path.as_bytes().to_vec());
            file.access_mask = Some(mask);
            let matched = driver.evaluate(&input).expect("real supplied file fields");
            assert!(matches!(matched.action, Action::Allow));
            let hit = matched.hit.expect("explicit preset RuleHit");
            assert!(hit.rule_id.as_str().starts_with("linux-fast-allow-"));
            assert!(!hit.log);
        }
    }
    for (path, mask, operation) in [
        ("/proc/meminfo", 2, FileOperation::Open),
        ("/dev/null-extra", 1, FileOperation::Open),
        ("/dev/mem", 3, FileOperation::Open),
        ("/dev/kmsg", 2, FileOperation::Open),
        ("/proc/sys/kernel/core_pattern", 3, FileOperation::Open),
        ("/usr/bin/worker", 1, FileOperation::Open),
        ("/private/key", 1, FileOperation::Open),
        ("/dev/null", 1, FileOperation::Unlink),
    ] {
        let mut input = event_operation(2, operation as u32);
        let EventPayload::File(file) = &mut input.payload else {
            panic!("file fixture");
        };
        file.path = Some(path.as_bytes().to_vec());
        file.access_mask = if operation == FileOperation::Open {
            Some(mask)
        } else {
            None
        };
        assert!(
            driver
                .evaluate(&input)
                .expect("exact boundaries")
                .hit
                .is_none(),
            "outside preset must retain original routing"
        );
    }
    for (effect, matched) in [
        (MountEffect::IndependentView, true),
        (MountEffect::ProtectedTree, false),
        (MountEffect::Unknown, false),
    ] {
        let mut input = event_operation(2, FileOperation::Mount as u32);
        let EventPayload::File(file) = &mut input.payload else {
            panic!("file");
        };
        file.mount_effect = Some(effect);
        let result = driver.evaluate(&input).expect("same driver matcher");
        assert_eq!(result.hit.is_some(), matched);
        if matched {
            assert!(matches!(result.action, Action::Allow));
            assert_eq!(
                result.hit.expect("real configured hit").group_id,
                "linux-self-protection-independent-mounts"
            );
        }
    }
    let mut modified = (*reopened.snapshot().document).clone();
    let group = modified.groups.iter_mut().find(|g| matches!(g, RuleGroup::File(g) if g.id == "linux-self-protection-independent-mounts")).expect("editable group");
    if let RuleGroup::File(group) = group {
        group.enabled = false;
    }
    reopened
        .update(reopened.snapshot().version, modified)
        .await
        .expect("ordinary management may disable maintenance policy");
    let disabled = DriverPolicy::compile(&reopened.snapshot().document).expect("disabled source");
    let mut input = event_operation(2, FileOperation::Mount as u32);
    if let EventPayload::File(file) = &mut input.payload {
        file.mount_effect = Some(MountEffect::IndependentView);
    }
    assert!(
        disabled
            .evaluate(&input)
            .expect("no policy hit")
            .hit
            .is_none()
    );
    assert!(matches!(
        reopened
            .select(EventKind::File)
            .fast(&event(2))
            .expect("existing private-file rule"),
        FastMatch::NoMatch
    ));
}

#[tokio::test]
async fn typed_backend_routes_project_process_target_and_network_port_conservatively() {
    use as_types::driver_control::{DriverCapabilities, DriverCondition, DriverFeature};
    let mut doc = document();
    let RuleGroup::Process(process) = &mut doc.groups[0] else {
        panic!("process")
    };
    process.rules[0].target.executable = Some(TextMatch::Prefix("/usr/bin/".into()));
    let RuleGroup::Network(network) = &mut doc.groups[2] else {
        panic!("network")
    };
    network.rules[0].operations = vec![as_types::event_operation::NetworkOperation::Send];
    network.rules[0].target.remote_port = Some(PortRange {
        first: 24671,
        last: 24674,
    });
    let (port, _) = interaction();
    let (_root, _context, engine) = load(doc, port).await;
    let mut caps = DriverCapabilities {
        max_frame_bytes: 16384,
        max_pending: 64,
        max_transfer_bytes: 4194304,
        max_hold_ms: 300000,
        operation_mask: DriverCapabilities::OPERATION_MASK,
        condition_mask: (1 << 25) - 1,
        features: DriverFeature::TerminateActor as u32 | DriverFeature::TerminateParent as u32,
    };
    let (image, _) = engine.prepare_driver_policy(&caps).expect("typed routes");
    let b = &image.immutable_buffer;
    let word = |at| u32::from_le_bytes(b[at..at + 4].try_into().expect("word"));
    assert_eq!([word(16), word(20), word(24)], [0, 1, 0]);
    assert_eq!(word(28), 2);
    let mut at = 32;
    for _ in 0..word(4) {
        at += word(at) as usize;
    }
    assert_eq!(&b[at..at + 4], &[1, 0, 0, 0]);
    assert_eq!(word(at + 4), 1);
    assert_eq!(&b[at + 8..at + 12], &[13, 0, 2, 0]);
    at += 24 + word(at + 12) as usize;
    assert_eq!(&b[at..at + 4], &[3, 0, 0, 0]);
    assert_eq!(word(at + 4), 2);
    assert_eq!(&b[at + 8..at + 12], &[23, 0, 7, 0]);
    assert_eq!(
        &b[at + 24..at + 28],
        &[24671u16.to_le_bytes(), 24674u16.to_le_bytes()].concat()
    );
    caps.condition_mask &= !(1 << (DriverCondition::RemotePort as u16 - 1));
    let (image, _) = engine
        .prepare_driver_policy(&caps)
        .expect("unfiltered fallback preserves candidate");
    assert_eq!(&image.immutable_buffer[24..28], &2u32.to_le_bytes());
    assert_eq!(&image.immutable_buffer[28..32], &1u32.to_le_bytes());
}

#[tokio::test]
async fn conference_policy_is_scoped_before_publication_and_exported_by_rule_owner() {
    use as_types::driver_control::{DriverCapabilities, DriverFeature};
    let text = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../config/rules.conference.yaml"
    ));
    let mut policy: PolicyDocument = serde_saphyr::from_str(text).expect("ordinary preset data");
    let instance = ContainerInstance {
        runtime: "docker".into(),
        namespace: None,
        id: "a".repeat(64),
    };
    fn configure<T, O>(group: &mut Group<T, O>, instance: &ContainerInstance) {
        assert!(
            !group.enabled,
            "template cannot protect an invented instance"
        );
        group.enabled = true;
        if !group.execution.is_driver() {
            for rule in &mut group.rules {
                rule.scope = Scope::Containers {
                    instances: vec![instance.clone()],
                };
            }
        }
    }
    for group in &mut policy.groups {
        match group {
            RuleGroup::Process(group) => configure(group, &instance),
            RuleGroup::File(group) => configure(group, &instance),
            RuleGroup::Network(group) => {
                for rule in &mut group.rules {
                    if rule.target.remote_cidr.as_deref() == Some("@docker_gateway_ipv4@/32") {
                        rule.target.remote_cidr = Some("192.0.2.1/32".into());
                    }
                }
                configure(group, &instance);
            }
        }
    }
    let (_root, context) = source(&PolicyDocument::default());
    let (port, _requests) = interaction();
    let _channel = port.connect().expect("authenticated review source");
    let engine = PolicyEngine::initialize(&context, port)
        .await
        .expect("fixed source");
    let receipt = engine
        .update(engine.snapshot().version, policy)
        .await
        .expect("ordinary publication");
    let caps = DriverCapabilities {
        max_frame_bytes: 16384,
        max_pending: 64,
        max_transfer_bytes: 4194304,
        max_hold_ms: 300000,
        operation_mask: DriverCapabilities::OPERATION_MASK,
        condition_mask: (1 << 25) - 1,
        features: DriverFeature::ContainerBinding as u32 | DriverFeature::DnsQuerySnapshot as u32,
    };
    let (_image, lease) = engine
        .prepare_driver_policy(&caps)
        .expect("supported backend scope, no native container-rule claim");
    assert_eq!(lease.version(), receipt.saved_version);
    for kind in [EventKind::Process, EventKind::File, EventKind::Network] {
        let rules = engine.select(kind);
        let mut input = event(kind as u16);
        input.process.workload = Some(Workload::Container(instance.clone()));
        match &mut input.payload {
            EventPayload::Process(target) => {
                target.target_executable = Some(b"/usr/bin/printf".to_vec())
            }
            EventPayload::File(target) => {
                target.path = Some(b"/home/node/.openclaw/workspace/as-demo/ask-file.txt".to_vec());
                target.access_mask = Some(1);
            }
            EventPayload::Network(target) => {
                target.remote_ip = Some("192.0.2.1".parse().expect("test gateway"));
                target.remote_port = Some(18088);
                target.protocol = Some(6);
            }
        }
        assert!(
            matches!(rules.fast(&input), Ok(FastMatch::NoMatch)),
            "selected target reaches Ask"
        );
        input.process.workload = Some(Workload::Host);
        assert!(matches!(rules.fast(&input), Ok(FastMatch::NoReviewMatch)));
        input.process.workload = Some(Workload::Container(ContainerInstance {
            id: "b".repeat(64),
            ..instance.clone()
        }));
        assert!(matches!(rules.fast(&input), Ok(FastMatch::NoReviewMatch)));
        input.process.workload = Some(Workload::Container(instance.clone()));
        match &mut input.payload {
            EventPayload::Process(target) => {
                target.target_executable = Some(b"/usr/bin/true".to_vec())
            }
            EventPayload::File(target) => {
                target.path = Some(b"/tmp/openclaw/openclaw-dynamic-date.log".to_vec())
            }
            EventPayload::Network(target) => target.remote_port = Some(443),
        }
        assert!(
            matches!(rules.fast(&input), Ok(FastMatch::NoReviewMatch)),
            "unrelated target does not enter review"
        );
    }
}
