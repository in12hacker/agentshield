//! Main composes module calls; actual writes remain with the configuration and rule owners.
use super::{
    connections::{Authorization, Connection, Connections, PendingQuestion},
    driver::DriverPort,
    protocol::*,
};
use crate::{MainDispatch, ProcessingThread};
use as_policy_engine::{
    PolicyEngine,
    interaction::{Interaction, Prompt},
};
use as_runtime_config::{ConfigError, ConfigModule, ConfigSnapshot};
use as_types::{
    configuration::{Access, ConfigEffect},
    driver_control::DriverCommand,
    main_ports::ModuleError,
};
use std::sync::{Arc, Mutex as ShortMutex};
use tokio::sync::{Mutex, Semaphore, oneshot};
/// Unified main-owned management entry with one upper grant boundary and one lower driver port.
pub struct MainMessages {
    pub(super) flow: Option<super::access::FlowPorts>,
    configuration: Arc<ConfigModule>,
    rules: Arc<PolicyEngine>,
    connections: Connections,
    driver: DriverPort,
    tree: Option<Arc<as_process_tree::ProcessTree>>,
    security_log: Option<as_security_log::SecurityLogReader>,
    dispatch: MainDispatch,
    pub(super) capacity: Arc<Semaphore>,
    urgent: Arc<Semaphore>,
    persistent: Mutex<()>,
    authentication: ShortMutex<()>,
    commands: tokio_util::task::TaskTracker,
    question: PendingQuestion,
    stop_port: Option<crate::runtime::MainStopPort>,
}
impl MainMessages {
    pub(super) fn credential_task<R, F, Fut>(
        self: &Arc<Self>,
        authorization: Authorization,
        work: F,
    ) -> Result<
        oneshot::Receiver<Result<R, as_types::flow::CredentialError>>,
        as_types::flow::CredentialError,
    >
    where
        R: Send + 'static,
        F: FnOnce(Arc<dyn as_types::flow::CredentialAdminPort>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<R, as_types::flow::CredentialError>>
            + Send
            + 'static,
    {
        use as_types::flow::CredentialError;
        let _admission = self
            .authentication
            .lock()
            .map_err(|_| CredentialError::Stopped)?;
        authorization
            .check(Access::ManageService)
            .map_err(|_| CredentialError::ScopeDenied)?;
        let permit = self
            .capacity
            .clone()
            .try_acquire_owned()
            .map_err(|e| match e {
                tokio::sync::TryAcquireError::Closed => CredentialError::Stopped,
                tokio::sync::TryAcquireError::NoPermits => CredentialError::Busy,
            })?;
        let port = self
            .flow
            .as_ref()
            .ok_or(CredentialError::SecretUnavailable)?
            .credentials
            .admin_port();
        let (tx, rx) = oneshot::channel();
        let task = self.commands.track_future(async move {
            let _permit = permit;
            let result = match authorization.check(Access::ManageService) {
                Ok(()) => work(port).await,
                Err(_) => Err(CredentialError::ScopeDenied),
            };
            let _ = tx.send(result);
        });
        self.dispatch.spawn(ProcessingThread::Messages, task);
        Ok(rx)
    }
    /// Compose the real owners; no alternate files, settings cache or fallback driver is created.
    pub fn new(
        configuration: Arc<ConfigModule>,
        rules: Arc<PolicyEngine>,
        interaction: Arc<Interaction>,
        driver: DriverPort,
        threads: &crate::MainThreads,
        flow: Option<super::access::FlowPorts>,
    ) -> Arc<Self> {
        threads.management_entry(|| {
            let snapshot = configuration.snapshot();
            let capacity = snapshot.document().queues.messages.get();
            let urgent = snapshot.document().queues.driver_urgent.get();
            let question = Arc::new(ShortMutex::new(None));
            Arc::new(Self {
                flow,
                configuration,
                rules,
                connections: Connections::new(interaction, &question),
                driver,
                tree: threads.process_tree(),
                security_log: threads.security_log(),
                dispatch: threads.dispatch(),
                capacity: Arc::new(Semaphore::new(capacity)),
                urgent: Arc::new(Semaphore::new(urgent)),
                persistent: Mutex::new(()),
                authentication: ShortMutex::new(()),
                commands: tokio_util::task::TaskTracker::new(),
                question,
                stop_port: threads.stop_port(),
            })
        })
    }
    /// Authenticate against the configuration owner's current server policy.
    pub fn connect(&self, stream: tokio::net::UnixStream) -> Result<Connection, ModuleError> {
        let _admission = self
            .authentication
            .lock()
            .map_err(|_| ModuleError::Unavailable)?;
        if self.capacity.is_closed() {
            return Err(ModuleError::Stopped);
        }
        self.connections
            .connect(stream, &self.configuration.snapshot().document().middleware)
    }
    /// Verify the first WebSocket credential and bind a fresh revocable Main grant.
    pub fn connect_websocket(
        &self,
        local: std::net::SocketAddr,
        origin: Option<&str>,
        token: &[u8],
    ) -> Result<Connection, ModuleError> {
        use sha2::{Digest, Sha256};
        use subtle::ConstantTimeEq;

        let _admission = self
            .authentication
            .lock()
            .map_err(|_| ModuleError::Unavailable)?;
        if self.capacity.is_closed() {
            return Err(ModuleError::Stopped);
        }
        let snapshot = self.configuration.snapshot();
        let policy = &snapshot.document().middleware;
        let ws = policy
            .websocket
            .as_ref()
            .ok_or(ModuleError::Unauthenticated)?;
        let digest: [u8; 32] = Sha256::digest(token).into();
        if ws.listen != local
            || !local.ip().is_loopback()
            || origin.is_some_and(|origin| ws.origin.as_deref() != Some(origin))
            || token.len() != 64
            || !token.iter().all(u8::is_ascii_hexdigit)
            || !bool::from(digest.ct_eq(&ws.token_sha256))
        {
            return Err(ModuleError::Unauthenticated);
        }
        self.connections.connect_websocket(policy)
    }
    /// Admit a concrete management request on main's message runtime. Cancellation loses only the waiter.
    pub fn submit(
        self: &Arc<Self>,
        session: &Connection,
        message: Down,
    ) -> Result<Option<oneshot::Receiver<super::CommandReply>>, ModuleError> {
        let _admission = self
            .authentication
            .lock()
            .map_err(|_| ModuleError::Unavailable)?;
        if self.capacity.is_closed() {
            return Err(ModuleError::Stopped);
        }
        let required = match &message {
            Down::Flow { .. } => Access::ReadOnly,
            Down::SecurityLog { .. } => Access::ManageService,
            Down::Processes { command, .. } => {
                if matches!(command, ProcessCommand::Refresh { .. }) {
                    Access::ManageService
                } else {
                    Access::ReadOnly
                }
            }
            Down::Configuration { command, .. } => {
                if matches!(command, ConfigCommand::Get) {
                    Access::ReadOnly
                } else {
                    Access::ManageService
                }
            }
            Down::Rules { command, .. } => {
                if matches!(command, RuleCommand::Get | RuleCommand::GetRule { .. }) {
                    Access::ReadOnly
                } else {
                    Access::ManageRules
                }
            }
            Down::Control { command, .. } => {
                if matches!(command, ControlCommand::GetStatus) {
                    Access::ReadOnly
                } else {
                    Access::ManageService
                }
            }
            Down::ReviewAvailability { available } => {
                session.set_review_available(*available)?;
                return Ok(None);
            }
            Down::Answer {
                decision_id,
                decision,
            } => {
                session.review_ready()?;
                let mut pending = self.question.lock().map_err(|_| ModuleError::Unavailable)?;
                let (grant, prompt) = pending.as_ref().ok_or(ModuleError::Stopped)?;
                if !grant.same_connection(&session.authorization())
                    || prompt.decision_id != *decision_id
                {
                    return Err(ModuleError::Forbidden);
                }
                if !prompt.choices().contains(decision) {
                    return Err(ModuleError::InvalidInput);
                }
                if prompt.deadline <= tokio::time::Instant::now() || prompt.reply.is_closed() {
                    pending.take();
                    return Err(ModuleError::Stopped);
                }
                let (_, prompt) = pending.take().ok_or(ModuleError::Stopped)?;
                prompt
                    .reply
                    .send(*decision)
                    .map_err(|_| ModuleError::Stopped)?;
                return Ok(None);
            }
        };
        let id = message.command_id().ok_or(ModuleError::InvalidInput)?;
        session.authorize(required)?;
        let budget = if matches!(message, Down::Control { .. }) {
            &self.urgent
        } else {
            &self.capacity
        };
        let permit = budget
            .clone()
            .try_acquire_owned()
            .map_err(|error| match error {
                tokio::sync::TryAcquireError::Closed => ModuleError::Stopped,
                tokio::sync::TryAcquireError::NoPermits => ModuleError::Busy,
            })?;
        let (reply, receiving) = oneshot::channel();
        let owner = self.clone();
        let authorization = session.authorization();
        let task = self.commands.track_future(async move {
            let _permit = permit;
            let (result, stop_reply_write) = if matches!(
                message,
                Down::Control {
                    command: ControlCommand::RequestStop,
                    ..
                }
            ) {
                match request_stop(
                    &owner.authentication,
                    &owner.urgent,
                    &authorization,
                    owner.stop_port.as_ref(),
                ) {
                    Ok(written) => (
                        CommandResult::Accepted {
                            data: ReplyData::MainStopRequested,
                        },
                        Some(written),
                    ),
                    Err(error) => (
                        failed(FailureCode::from(error), &error.to_string(), None),
                        None,
                    ),
                }
            } else {
                (owner.execute(message, authorization, required).await, None)
            };
            let _ = reply.send(super::CommandReply {
                message: Up::Reply { id, result },
                stop_reply_write,
            });
        });
        self.dispatch.spawn(ProcessingThread::Messages, task);
        Ok(Some(receiving))
    }
    /// Expose the rule owner's one actual question to a ready, authorized channel.
    /// Project the prompt's own choices. Answers reuse submit and the same session.
    pub fn question(&self, session: &Connection, prompt: Prompt) -> Result<Up, ModuleError> {
        let _admission = self
            .authentication
            .lock()
            .map_err(|_| ModuleError::Unavailable)?;
        if self.capacity.is_closed() {
            return Err(ModuleError::Stopped);
        }
        session.review_ready()?;
        if prompt.reply.is_closed() || prompt.deadline <= tokio::time::Instant::now() {
            return Err(ModuleError::Stopped);
        }
        let mut pending = self.question.lock().map_err(|_| ModuleError::Unavailable)?;
        if pending
            .as_ref()
            .is_some_and(|(_, p)| !p.reply.is_closed() && p.deadline > tokio::time::Instant::now())
        {
            return Err(ModuleError::Busy);
        }
        let message = Up::Ask {
            decision_id: prompt.decision_id,
            hit: prompt.hit.clone(),
            event: prompt.event.clone(),
            choices: prompt.choices().to_vec(),
            timeout: prompt.timeout,
            remaining_ms: u64::try_from(
                prompt
                    .deadline
                    .saturating_duration_since(tokio::time::Instant::now())
                    .as_millis(),
            )
            .map_err(|_| ModuleError::InvalidInput)?,
        };
        *pending = Some((session.authorization(), prompt));
        Ok(message)
    }
    /// Release only this connection's same question once its original answer capability expired.
    /// Empty is idempotent; a live or replaced question is never cancelled by stale output.
    pub fn retire_expired_question(
        &self,
        session: &Connection,
        decision_id: as_types::DecisionId,
    ) -> Result<(), ModuleError> {
        let _admission = self
            .authentication
            .lock()
            .map_err(|_| ModuleError::Unavailable)?;
        session.authorize(Access::ManageRules)?;
        retire_expired(&self.question, &session.authorization(), decision_id)
    }
    async fn execute(
        &self,
        message: Down,
        authorization: Authorization,
        required: Access,
    ) -> CommandResult {
        let mutation = matches!(
            &message,
            Down::Configuration {
                command: ConfigCommand::Update { .. } | ConfigCommand::Reload { .. },
                ..
            } | Down::Rules {
                command: RuleCommand::Update { .. }
                    | RuleCommand::Edit { .. }
                    | RuleCommand::Reload { .. },
                ..
            }
        );
        let _serial = if mutation {
            Some(self.persistent.lock().await)
        } else {
            None
        };
        // Recheck after the management queue/lock: a prior policy change may have revoked this grant.
        if let Err(error) = authorization.check(required) {
            return failed(FailureCode::Unauthorized, &error.to_string(), None);
        }
        if let Down::Configuration {
            command: ConfigCommand::Update { document, .. },
            ..
        } = &message
            && let Some(flow) = &self.flow
        {
            let routes = document
                .flow
                .as_ref()
                .map(|f| f.provider_routes.as_slice())
                .unwrap_or(&[]);
            if let Err(error) = flow
                .credentials
                .validate_settings(&document.credentials, routes)
            {
                return failed(FailureCode::Invalid, &error.to_string(), None);
            }
        }
        match message {
            Down::SecurityLog { command, .. } => {
                let Some(reader) = &self.security_log else {
                    return failed(
                        FailureCode::Unavailable,
                        "security log is not initialized",
                        None,
                    );
                };
                match reader.page(command).await {
                    Ok(page) => completed(ReplyData::SecurityLog { page }),
                    Err(error) => failed(FailureCode::from(error), &error.to_string(), None),
                }
            }
            Down::Flow { command, .. } => {
                let Some(read) = self.flow.as_ref().and_then(|p| p.status.as_ref()) else {
                    return failed(
                        FailureCode::Unavailable,
                        "TLS mediation is not running",
                        None,
                    );
                };
                match command {
                    FlowCommand::Status => {
                        let ingress = match self
                            .flow
                            .as_ref()
                            .and_then(|p| p.ingress.as_ref())
                            .map(|i| i.status())
                            .transpose()
                        {
                            Ok(status) => status,
                            Err(error) => {
                                return failed(FailureCode::Unavailable, &error.to_string(), None);
                            }
                        };
                        completed(ReplyData::FlowStatus {
                            status: read.status(),
                            ingress,
                        })
                    }
                }
            }

            Down::Processes { command, .. } => {
                let Some(tree) = &self.tree else {
                    return failed(
                        FailureCode::Unavailable,
                        "process tree not initialized",
                        None,
                    );
                };
                let result = match command {
                    ProcessCommand::Query { query } => tree
                        .query(*query)
                        .await
                        .map(|result| ReplyData::Processes { result }),
                    ProcessCommand::Refresh { target } => tree
                        .refresh(target)
                        .await
                        .map(|receipt| ReplyData::ProcessesRefreshed { receipt }),
                };
                match result {
                    Ok(data) => completed(data),
                    Err(error) => {
                        let code = match &error {
                            as_process_tree::Error::NotFound => FailureCode::NotFound,
                            as_process_tree::Error::CursorExpired => FailureCode::CursorExpired,
                            as_process_tree::Error::Conflict => FailureCode::Conflict,
                            as_process_tree::Error::Invalid => FailureCode::Invalid,
                            as_process_tree::Error::ResourceLimit => FailureCode::Busy,
                            _ => FailureCode::Unavailable,
                        };
                        failed(code, &error.to_string(), None)
                    }
                }
            }
            Down::Configuration {
                command: ConfigCommand::Get,
                ..
            } => completed(ReplyData::Configuration {
                snapshot: self.configuration.snapshot(),
            }),
            Down::Configuration { command, .. } => {
                let before = self.configuration.snapshot();
                let result = match command {
                    ConfigCommand::Update {
                        expected_version,
                        document,
                    } => self
                        .configuration
                        .update(expected_version, *document)
                        .await
                        .map(|receipt| ReplyData::ConfigurationSaved {
                            receipt,
                            applied: None,
                        }),
                    ConfigCommand::Reload { expected_version } => self
                        .configuration
                        .reload(expected_version)
                        .await
                        .map(|snapshot| ReplyData::Configuration { snapshot }),
                    ConfigCommand::Get => return unreachable_result(),
                };
                self.finish_configuration(&before, result).await
            }
            Down::Rules {
                command: RuleCommand::Get,
                ..
            } => completed(ReplyData::Rules {
                snapshot: self.rules.snapshot(),
            }),
            Down::Rules {
                command: RuleCommand::GetRule { rule_id },
                ..
            } => match self.rules.get_rule(&rule_id) {
                Ok(snapshot) => completed(ReplyData::Rule {
                    snapshot: Box::new(snapshot),
                }),
                Err(error) => failed(
                    match error {
                        as_policy_engine::Error::NotFound => FailureCode::NotFound,
                        _ => FailureCode::Failed,
                    },
                    &error.to_string(),
                    None,
                ),
            },
            Down::Rules { command, .. } => {
                let result = match command {
                    RuleCommand::Update {
                        expected_version,
                        document,
                    } => self
                        .rules
                        .update(expected_version, document)
                        .await
                        .map(|receipt| ReplyData::RulesSaved {
                            receipt,
                            applied: None,
                        }),
                    RuleCommand::Edit {
                        expected_version,
                        change,
                    } => self
                        .rules
                        .edit(expected_version, *change)
                        .await
                        .map(|receipt| ReplyData::RulesSaved {
                            receipt,
                            applied: None,
                        }),
                    RuleCommand::Reload { expected_version } => self
                        .rules
                        .reload(expected_version)
                        .await
                        .map(|snapshot| ReplyData::Rules { snapshot }),
                    RuleCommand::Get | RuleCommand::GetRule { .. } => return unreachable_result(),
                };
                match result {
                    Ok(facts) => match self.adopt_current().await {
                        Ok(applied) => match facts {
                            ReplyData::RulesSaved { receipt, .. } => {
                                completed(ReplyData::RulesSaved {
                                    receipt,
                                    applied: Some(applied),
                                })
                            }
                            ReplyData::Rules { snapshot } => {
                                completed(ReplyData::RulesReloaded { snapshot, applied })
                            }
                            _ => unreachable_result(),
                        },
                        Err((code, message)) => failed(code, &message, Some(facts)),
                    },
                    Err(error) => failed(
                        match error {
                            as_policy_engine::Error::Conflict => FailureCode::Conflict,
                            as_policy_engine::Error::NotFound => FailureCode::NotFound,
                            as_policy_engine::Error::Invalid(_) => FailureCode::Invalid,
                            _ => FailureCode::Failed,
                        },
                        &error.to_string(),
                        matches!(error, as_policy_engine::Error::Durability { .. }).then(|| {
                            ReplyData::Rules {
                                snapshot: self.rules.snapshot(),
                            }
                        }),
                    ),
                }
            }
            Down::Control { command, .. } => {
                let command = match command {
                    ControlCommand::GetStatus => DriverCommand::GetStatus,
                    ControlCommand::SetRuntimeFeature { feature, enabled } => {
                        DriverCommand::SetRuntimeFeature { feature, enabled }
                    }
                    ControlCommand::ResumeRuntimeFeature { feature } => {
                        DriverCommand::ResumeRuntimeFeature { feature }
                    }
                    ControlCommand::RequestStop => return unreachable_result(),
                };
                match self.driver.request(command).await {
                    Ok(snapshot) => completed(ReplyData::DriverStatus { snapshot }),
                    Err(error) => failed(FailureCode::from(error), &error.to_string(), None),
                }
            }
            _ => unreachable_result(),
        }
    }
    async fn finish_configuration(
        &self,
        before: &ConfigSnapshot,
        result: Result<ReplyData, ConfigError>,
    ) -> CommandResult {
        let snapshot = self.configuration.snapshot();
        let (facts, save_error) = match result {
            Ok(facts) => (facts, None),
            Err(error)
                if snapshot.version() != before.version()
                    || matches!(error, ConfigError::Durability { .. }) =>
            {
                (
                    ReplyData::Configuration {
                        snapshot: snapshot.clone(),
                    },
                    Some(error),
                )
            }
            Err(error) => return config_error(error, None),
        };
        if self.adopt_authorization(before, &snapshot).is_err() {
            return configuration_failed(
                "configuration published; previous sessions could not be retired",
                save_error,
                facts,
            );
        }
        if self.adopt_credentials(&snapshot).is_err() {
            return configuration_failed(
                "configuration published; credential policy adoption failed",
                save_error,
                facts,
            );
        }
        if let Some(error) = save_error {
            return config_error(error, Some(facts));
        }
        let effects = snapshot.required_effects_from(before);
        if self.configuration.reinitialization_pending(&snapshot) {
            return failed(
                FailureCode::Unavailable,
                "configuration published; runtime resource changes require complete main reinitialization",
                Some(facts),
            );
        }
        if !effects.contains(&ConfigEffect::ApplyDriver) {
            return completed(facts);
        }
        match self.adopt_current().await {
            Ok(applied) => match facts {
                ReplyData::ConfigurationSaved { receipt, .. } => {
                    completed(ReplyData::ConfigurationSaved {
                        receipt,
                        applied: Some(applied),
                    })
                }
                ReplyData::Configuration { snapshot } => {
                    completed(ReplyData::ConfigurationReloaded { snapshot, applied })
                }
                _ => unreachable_result(),
            },
            Err((code, message)) => failed(code, &message, Some(facts)),
        }
    }

    fn adopt_credentials(&self, snapshot: &ConfigSnapshot) -> Result<(), ModuleError> {
        if let Some(flow) = &self.flow {
            flow.credentials
                .apply_settings(
                    Arc::new(snapshot.document().credentials.clone()),
                    snapshot
                        .document()
                        .flow
                        .as_ref()
                        .map(|f| f.provider_routes.clone())
                        .unwrap_or_default(),
                    snapshot.version(),
                )
                .map_err(|_| ModuleError::Unavailable)?;
        } else if !snapshot.document().credentials.bindings.is_empty() {
            return Err(ModuleError::Unavailable);
        }
        Ok(())
    }
    async fn adopt_current(
        &self,
    ) -> Result<as_types::driver_control::DriverStatus, (FailureCode, String)> {
        let status = self
            .driver
            .request(DriverCommand::GetStatus)
            .await
            .map_err(|error| (FailureCode::from(error), error.to_string()))?;
        let caps = self
            .driver
            .capabilities()
            .map_err(|error| (FailureCode::from(error), error.to_string()))?;
        let configuration = self.configuration.clone();
        let rules = self.rules.clone();
        let prepared = tokio::task::spawn_blocking(move || {
            let config = configuration
                .prepare_driver_config(&caps)
                .map_err(|error| {
                    (
                        match error {
                            ConfigError::Unsupported(_) => FailureCode::Unsupported,
                            _ => FailureCode::Failed,
                        },
                        error.to_string(),
                    )
                })?;
            let (policy, lease) = rules.prepare_driver_policy(&caps).map_err(|error| {
                (
                    match error {
                        as_policy_engine::Error::Unsupported(_) => FailureCode::Unsupported,
                        _ => FailureCode::Invalid,
                    },
                    error.to_string(),
                )
            })?;
            Ok::<_, (FailureCode, String)>((config, policy, lease))
        })
        .await
        .map_err(|_| {
            (
                FailureCode::Unavailable,
                "persistent preparation owner failed".into(),
            )
        })??;
        let (configuration, rules, lease) = prepared;
        if status.driver_config_version == Some(configuration.driver_config_version)
            && status.rule_version == Some(rules.rule_version)
        {
            return Ok(status);
        }
        self.driver
            .replace(
                as_types::driver_image::DriverInitParts {
                    expected_persistent_generation: status.persistent_generation,
                    expected_lifecycle_epoch: status.lifecycle_epoch,
                    configuration,
                    rules,
                },
                lease,
            )
            .await
            .map(|applied| applied.status)
            .map_err(|error| (FailureCode::from(error), error.to_string()))
    }
    fn adopt_authorization(
        &self,
        before: &ConfigSnapshot,
        published: &ConfigSnapshot,
    ) -> Result<(), ModuleError> {
        let _admission = self
            .authentication
            .lock()
            .map_err(|_| ModuleError::Unavailable)?;
        if before.document().middleware != published.document().middleware {
            self.connections.revoke_all()?;
        }
        Ok(())
    }
    /// Retire message producers before joining main; running owner jobs retain their actual work.
    pub fn close(&self) -> Result<(), ModuleError> {
        let _admission = self
            .authentication
            .lock()
            .map_err(|_| ModuleError::Unavailable)?;
        self.seal_admission();
        self.connections.revoke_all()
    }

    /// Seal management and release review while the host drains already-authorized content.
    pub(crate) fn seal_for_content_drain(&self) -> Result<(), ModuleError> {
        let _admission = self
            .authentication
            .lock()
            .map_err(|_| ModuleError::Unavailable)?;
        self.seal_admission();
        self.connections.stop_review()
    }

    fn seal_admission(&self) {
        self.capacity.close();
        self.urgent.close();
        self.commands.close();
    }
    /// Join accepted finite commands while the real driver and its callback reader remain available.
    pub(crate) async fn drain_commands(&self) {
        self.commands.wait().await;
    }
}

fn request_stop(
    authentication: &ShortMutex<()>,
    urgent: &Semaphore,
    authorization: &Authorization,
    port: Option<&crate::runtime::MainStopPort>,
) -> Result<crate::runtime::StopReplyWrite, ModuleError> {
    let _admission = authentication
        .lock()
        .map_err(|_| ModuleError::Unavailable)?;
    // Adoption and close hold this same lock. No await separates this recheck and handoff.
    authorization.check(Access::ManageService)?;
    if urgent.is_closed() {
        return Err(ModuleError::Stopped);
    }
    port.ok_or(ModuleError::Unavailable)?.request()
}

fn retire_expired(
    question: &PendingQuestion,
    authorization: &Authorization,
    decision_id: as_types::DecisionId,
) -> Result<(), ModuleError> {
    let mut pending = question.lock().map_err(|_| ModuleError::Unavailable)?;
    let Some((grant, prompt)) = pending.as_ref() else {
        return Ok(());
    };
    if !grant.same_connection(authorization) {
        return Err(ModuleError::Forbidden);
    }
    if prompt.decision_id != decision_id {
        return Err(ModuleError::Conflict);
    }
    if prompt.deadline > tokio::time::Instant::now() && !prompt.reply.is_closed() {
        return Err(ModuleError::Busy);
    }
    pending.take();
    Ok(())
}
fn completed(data: ReplyData) -> CommandResult {
    CommandResult::Completed { data }
}
fn failed(code: FailureCode, message: &str, facts: Option<ReplyData>) -> CommandResult {
    CommandResult::Failed {
        code,
        message: message.into(),
        facts,
    }
}
fn unreachable_result() -> CommandResult {
    failed(
        FailureCode::Invalid,
        "command did not match its typed management route",
        None,
    )
}
fn config_error(error: ConfigError, facts: Option<ReplyData>) -> CommandResult {
    failed(
        match error {
            ConfigError::Conflict => FailureCode::Conflict,
            ConfigError::Invalid(_) | ConfigError::Layout => FailureCode::Invalid,
            _ => FailureCode::Failed,
        },
        &error.to_string(),
        facts,
    )
}

fn configuration_failed(
    message: &str,
    save_error: Option<ConfigError>,
    facts: ReplyData,
) -> CommandResult {
    let message = match save_error {
        Some(error) => format!("{message} ({error})"),
        None => message.to_owned(),
    };
    failed(FailureCode::Unavailable, &message, Some(facts))
}

#[cfg(test)]
mod expiry_tests {
    use super::*;
    use as_types::{
        DecisionId, RuleVersion,
        configuration::{MiddlewareAccess, Principal},
        main_ports::{RuleHit, RuleSourceVersion},
    };
    use std::num::{NonZeroU32, NonZeroUsize};
    use tokio::net::UnixStream;

    #[tokio::test]
    async fn stop_handoff_rechecks_the_real_grant_and_closed_admission_before_consuming_the_port() {
        let (_root, context, queues) = crate::test_driver::environment();
        let mut threads = crate::MainThreads::prepare(context, queues)
            .await
            .expect("real worker threads");
        let listener = threads.bind_stop_listener().expect("real host listener");
        let port = threads.stop_port().expect("composed stop port");
        let (interaction, _) = Interaction::channel(NonZeroUsize::MIN);
        let sessions = Connections::for_testing(interaction);
        let authentication = ShortMutex::new(());
        let urgent = Semaphore::new(1);
        let mut peers = Vec::new();
        let mut connect = |access| {
            let (socket, peer) = UnixStream::pair().expect("actual socket");
            let uid = socket.peer_cred().expect("actual peer").uid();
            peers.push(peer);
            sessions
                .connect(
                    socket,
                    &MiddlewareAccess {
                        peer: Principal::UnixUid { uid },
                        access,
                        chat_content: false,
                        websocket: None,
                    },
                )
                .expect("authenticated connection")
        };
        for access in [Access::ReadOnly, Access::ManageRules] {
            let session = connect(access);
            assert!(matches!(
                request_stop(
                    &authentication,
                    &urgent,
                    &session.authorization(),
                    Some(&port)
                ),
                Err(ModuleError::Forbidden)
            ));
        }
        let revoked = connect(Access::ManageService);
        revoked
            .authorize(Access::ManageService)
            .expect("accepted initial grant");
        let old_ready = revoked.review_readiness();
        assert!(!old_ready());
        revoked
            .set_review_available(true)
            .expect("original review channel ready");
        assert!(old_ready());
        sessions
            .revoke_all()
            .expect("policy adopted before queued execution");
        assert!(
            !old_ready(),
            "a still-open revoked socket cannot route questions"
        );
        assert!(matches!(
            request_stop(
                &authentication,
                &urgent,
                &revoked.authorization(),
                Some(&port)
            ),
            Err(ModuleError::Unauthenticated)
        ));
        let manager = connect(Access::ManageService);
        manager
            .set_review_available(true)
            .expect("new review channel");
        let new_ready = manager.review_readiness();
        assert!(new_ready() && !old_ready());
        assert!(matches!(
            request_stop(&authentication, &urgent, &manager.authorization(), None),
            Err(ModuleError::Unavailable)
        ));
        let closed = Semaphore::new(1);
        closed.close();
        assert!(matches!(
            request_stop(
                &authentication,
                &closed,
                &manager.authorization(),
                Some(&port)
            ),
            Err(ModuleError::Stopped)
        ));
        let unwritten = request_stop(
            &authentication,
            &urgent,
            &manager.authorization(),
            Some(&port),
        )
        .expect("only the live manager reaches the host");
        assert!(matches!(
            request_stop(
                &authentication,
                &urgent,
                &manager.authorization(),
                Some(&port)
            ),
            Err(ModuleError::Unavailable)
        ));
        let request = listener.await.expect("exactly one actual request");
        drop(unwritten);
        assert!(request.reply_written.await.is_err());
        threads.shutdown().await.expect("actual worker joins");
    }

    #[tokio::test]
    async fn expired_answer_cleanup_preserves_live_replacement_and_control_availability() {
        let (interaction, _) = Interaction::channel(NonZeroUsize::MIN);
        let question: PendingQuestion = Arc::new(ShortMutex::new(None));
        let sessions = Connections::new(interaction, &question);
        let (socket, _peer) = UnixStream::pair().expect("first actual socket");
        let uid = socket.peer_cred().expect("actual peer").uid();
        let policy = MiddlewareAccess {
            peer: Principal::UnixUid { uid },
            access: Access::ManageRules,
            chat_content: false,
            websocket: None,
        };
        let first = sessions
            .connect(socket, &policy)
            .expect("first authenticated");
        let (socket, _other_peer) = UnixStream::pair().expect("second actual socket");
        let other = sessions
            .connect(socket, &policy)
            .expect("other authenticated");
        first
            .set_review_available(true)
            .expect("actual decision channel");
        let mut bytes = vec![0u8; 88];
        bytes[..2].copy_from_slice(&1u16.to_le_bytes());
        bytes[2..4].copy_from_slice(&88u16.to_le_bytes());
        bytes[4..8].copy_from_slice(&88u32.to_le_bytes());
        bytes[8..10].copy_from_slice(&1u16.to_le_bytes());
        bytes[10..12].copy_from_slice(&2u16.to_le_bytes());
        bytes[12..16].copy_from_slice(&1u32.to_le_bytes());
        bytes[16..32].copy_from_slice(&[1; 16]);
        bytes[32..40].copy_from_slice(&1u64.to_le_bytes());
        bytes[40..48].copy_from_slice(&1u64.to_le_bytes());
        let event = as_types::dispatch_event::decode(&bytes).expect("original fixture event");
        let id = DecisionId([1; 16]);
        let (reply, mut answered) = oneshot::channel();
        let prompt = Prompt {
            decision_id: id,
            event: Arc::new(event),
            hit: RuleHit {
                group_id: "fixture".into(),
                group_number: NonZeroU32::MIN,
                rule_id: "fixture".into(),
                rule_number: NonZeroU32::MIN,
                source: RuleSourceVersion::RulesDocument(RuleVersion::from_digest([1; 32])),
                log: false,
                deny_message: Some(String::new()),
            },
            deadline: tokio::time::Instant::now() + std::time::Duration::from_secs(60),
            timeout: as_types::main_ports::Decision::Allow,
            reply,
        };
        *question.lock().expect("pending") = Some((first.authorization(), prompt));
        assert!(matches!(
            retire_expired(&question, &first.authorization(), id),
            Err(ModuleError::Busy)
        ));
        assert!(matches!(
            retire_expired(&question, &other.authorization(), id),
            Err(ModuleError::Forbidden)
        ));
        assert!(matches!(
            retire_expired(&question, &first.authorization(), DecisionId([2; 16])),
            Err(ModuleError::Conflict)
        ));
        assert!(matches!(
            answered.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        let (event, hit) = {
            let mut pending = question.lock().expect("original deadline");
            let prompt = &mut pending.as_mut().expect("same prompt").1;
            prompt.deadline = tokio::time::Instant::now();
            (prompt.event.clone(), prompt.hit.clone())
        };
        retire_expired(&question, &first.authorization(), id).expect("actual expiry released");
        assert!(question.lock().expect("empty").is_none());
        assert!(
            answered.await.is_err(),
            "releasing the old sender does not send a decision"
        );
        retire_expired(&question, &first.authorization(), id).expect("idempotent empty");
        first
            .review_ready()
            .expect("same control channel stays available");
        let (reply, closed) = oneshot::channel();
        drop(closed);
        let next = DecisionId([2; 16]);
        *question.lock().expect("replacement") = Some((
            first.authorization(),
            Prompt {
                decision_id: next,
                event,
                hit,
                deadline: tokio::time::Instant::now() + std::time::Duration::from_secs(60),
                timeout: as_types::main_ports::Decision::Allow,
                reply,
            },
        ));
        assert!(matches!(
            retire_expired(&question, &first.authorization(), id),
            Err(ModuleError::Conflict)
        ));
        retire_expired(&question, &first.authorization(), next)
            .expect("only the same closed reply capability is released");
        first
            .review_ready()
            .expect("cleanup never revokes the channel");
    }
}
