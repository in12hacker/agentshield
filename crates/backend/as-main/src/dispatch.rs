//! Three-stage main scheduling over the new event/module contracts.
use as_types::{dispatch_event::ProcessInfo, main_ports::*};
use futures::{StreamExt, future::BoxFuture, stream::FuturesUnordered};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
use tokio::sync::{broadcast, mpsc, oneshot, watch};

/// Rejected input stays with its original producer; rejection is not an operation decision.
pub struct SubmitError {
    /// Admission failure.
    pub reason: ModuleError,
    /// Untouched original input and operation reference.
    pub input: Box<DispatchInput>,
}

struct Cancellation {
    operation: Option<Arc<dyn as_types::operation::OriginalOperation>>,
    facts: Option<(Arc<dyn FastFacts>, as_types::process_tree::EventKey)>,
}
impl Drop for Cancellation {
    fn drop(&mut self) {
        if let Some((facts, key)) = self.facts.take() {
            facts.finish_event(key);
        }
        if let Some(operation) = self.operation.take() {
            operation.cancel();
        }
    }
}
struct Work {
    input: DispatchInput,
    reply: Reply,
    cancellation: Cancellation,
    rules: Option<Arc<dyn RulesModule>>,
    needs_facts: bool,
    _policy: Option<as_policy_engine::PolicyLease>,
}
enum Reply {
    External(oneshot::Sender<Result<DispatchOutcome, ModuleError>>),
    Native,
}
impl Reply {
    fn is_closed(&self) -> bool {
        matches!(self, Self::External(reply) if reply.is_closed())
    }
    async fn closed(&mut self) {
        match self {
            Self::External(reply) => reply.closed().await,
            Self::Native => std::future::pending().await,
        }
    }
    fn send(self, result: Result<DispatchOutcome, ModuleError>) {
        if let Self::External(reply) = self {
            let _ = reply.send(result);
        }
    }
}
impl Work {
    fn new(
        input: DispatchInput,
        reply: Reply,
        policy: Option<as_policy_engine::PolicyLease>,
    ) -> Self {
        Self {
            cancellation: Cancellation {
                operation: input.operation.clone(),
                facts: None,
            },
            input,
            reply,
            rules: None,
            needs_facts: false,
            _policy: policy,
        }
    }
    fn reject(self, reason: ModuleError) -> SubmitError {
        let Self {
            input,
            reply: _,
            mut cancellation,
            rules: _,
            needs_facts: _,
            _policy: _,
        } = self;
        cancellation.operation = None;
        SubmitError {
            reason,
            input: Box::new(input),
        }
    }
}

/// The one typed producer port; the fast, complex and message queues have independent budgets.
#[derive(Clone)]
pub struct MainIngress {
    sender: mpsc::Sender<FastInput>,
    accepting: Arc<Mutex<bool>>,
    rejected_notices: Arc<AtomicU64>,
    rejected_logs: Arc<AtomicU64>,
    stop: watch::Sender<bool>,
    producers: std::sync::Weak<MessageProducers>,
}
/// Both queues share the actual source/operation lifetime, without a separate owner service.
pub(crate) struct MessageProducers {
    notices: mpsc::Sender<MainMessage>,
    logs: mpsc::Sender<SecurityLogEntry>,
}
impl MessageProducers {
    async fn notice(&self, notice: MainMessage) -> Result<(), ModuleError> {
        self.notices
            .send(notice)
            .await
            .map_err(|_| ModuleError::Stopped)
    }
}
fn publish_log(producers: Option<&MessageProducers>, rejected: &AtomicU64, log: SecurityLogEntry) {
    if producers.is_none_or(|sender| sender.logs.try_send(log).is_err()) {
        reject_log(rejected);
    }
}
fn reject_log(rejected: &AtomicU64) {
    rejected.fetch_add(1, Ordering::Relaxed);
    tracing::warn!("security log request not accepted; no durable write confirmed");
}
enum FastInput {
    Work(Box<Work>),
    Retire(Box<Work>, ModuleError),
    DirectHits(Vec<as_types::driver::DirectHitCount>),
    Fence(oneshot::Sender<()>),
}
impl MainIngress {
    /// Complete only after all earlier admitted inputs have run their fast fact maintenance.
    pub(crate) async fn fact_barrier(&self) -> Result<(), ModuleError> {
        let (done, waiting) = oneshot::channel();
        self.sender
            .send(FastInput::Fence(done))
            .await
            .map_err(|_| ModuleError::Stopped)?;
        waiting.await.map_err(|_| ModuleError::Stopped)
    }
    pub(crate) fn is_closed(&self) -> bool {
        *self.stop.borrow()
    }
    /// Retain one actual native result/status until the existing consumer accepts it.
    /// Closed consumption is an explicit failure, distinct from the original operation result.
    pub async fn driver_notice(&self, notice: MainMessage) -> Result<(), ModuleError> {
        let result = match self.producers.upgrade() {
            Some(sender) => sender.notice(notice).await,
            None => Err(ModuleError::Stopped),
        };
        if result.is_err() {
            self.rejected_notices.fetch_add(1, Ordering::Relaxed);
            tracing::warn!("native notice consumer unavailable; no durable effect claimed");
        }
        result
    }
    /// Publish an explicit log request without occupying completion/status capacity.
    pub fn driver_log(&self, log: RuleLogRequest) {
        publish_log(
            self.producers.upgrade().as_deref(),
            &self.rejected_logs,
            SecurityLogEntry::Rule(log),
        );
    }
    /// Retain an already-read close fact until the existing log queue accepts it.
    /// Storage closure remains a log rejection, not a fabricated driver failure.
    pub(crate) async fn driver_file_close(
        &self,
        record: as_types::security_log::FileCloseAuditRecord,
    ) {
        if let Some(producer) = self.producers.upgrade()
            && producer
                .logs
                .send(SecurityLogEntry::FileClose(record))
                .await
                .is_ok()
        {
            return;
        }
        reject_log(&self.rejected_logs);
    }
    pub(crate) fn driver_message_producer(&self) -> Option<Arc<MessageProducers>> {
        self.producers.upgrade()
    }
    /// Transfer an original and its exact policy lease to the same bounded work lifecycle.
    /// No subscriber task or second compensation budget is created for native callbacks.
    pub(crate) async fn native_work(
        &self,
        input: DispatchInput,
        policy: Result<as_policy_engine::PolicyLease, ModuleError>,
    ) -> Result<(), ModuleError> {
        if input.event.stage() != EventStage::ControlledRequest || input.operation.is_none() {
            return Err(ModuleError::InvalidInput);
        }
        let accepting = *self
            .accepting
            .lock()
            .map_err(|_| ModuleError::Unavailable)?;
        let (policy, failure) = match policy {
            Ok(policy) => (Some(policy), (!accepting).then_some(ModuleError::Stopped)),
            Err(error) => (None, Some(error)),
        };
        let work = Box::new(Work::new(input, Reply::Native, policy));
        let input = match failure {
            Some(error) => FastInput::Retire(work, error),
            None => FastInput::Work(work),
        };
        self.enqueue_native(input).await
    }
    /// Transfer a lifecycle fact without creating an external reply subscription.
    pub(crate) async fn native_observation(
        &self,
        event: as_types::dispatch_event::EventData,
    ) -> Result<(), ModuleError> {
        if event.stage() != EventStage::Observation {
            return Err(ModuleError::InvalidInput);
        }
        let work = Work::new(
            DispatchInput {
                event: Arc::new(event),
                operation: None,
                transfer_hit: None,
            },
            Reply::Native,
            None,
        );
        self.enqueue_native(FastInput::Work(Box::new(work))).await
    }
    pub(crate) async fn native_hits(
        &self,
        batch: Vec<as_types::driver::DirectHitCount>,
    ) -> Result<(), ModuleError> {
        self.enqueue_native(FastInput::DirectHits(batch)).await
    }
    async fn enqueue_native(&self, input: FastInput) -> Result<(), ModuleError> {
        if let Err(error) = self.sender.send(input).await {
            let mut work = match error.0 {
                FastInput::Work(work) | FastInput::Retire(work, _) => work,
                // Management and this consumer have retired. The native pump
                // still has to drain the wire to obtain the real Quiesce reply.
                FastInput::DirectHits(_) if self.is_closed() => return Ok(()),
                FastInput::DirectHits(_) => return Err(ModuleError::Stopped),
                FastInput::Fence(_) => return Err(ModuleError::InvalidInput),
            };
            let _policy = work._policy.take();
            let rejected = work.reject(ModuleError::Stopped);
            let notice = rejected_completion(*rejected.input, rejected.reason).await;
            self.driver_notice(MainMessage::Completed(Box::new(notice)))
                .await?;
        }
        Ok(())
    }
    /// Submit without blocking the source thread; failed admission returns the original input.
    pub fn submit(&self, input: DispatchInput) -> Result<DispatchReply, SubmitError> {
        let controlled = input.event.stage() == EventStage::ControlledRequest;
        if controlled != input.operation.is_some() {
            return Err(SubmitError {
                reason: ModuleError::InvalidInput,
                input: Box::new(input),
            });
        }
        let accepting = match self.accepting.lock() {
            Ok(accepting) => accepting,
            Err(_) => {
                return Err(SubmitError {
                    reason: ModuleError::Unavailable,
                    input: Box::new(input),
                });
            }
        };
        if !*accepting && controlled {
            return Err(SubmitError {
                reason: ModuleError::Stopped,
                input: Box::new(input),
            });
        }
        let (reply, receiving) = oneshot::channel();
        let work = Work::new(input, Reply::External(reply), None);
        match self.sender.try_reserve() {
            Ok(permit) => {
                permit.send(FastInput::Work(Box::new(work)));
                Ok(receiving)
            }
            Err(mpsc::error::TrySendError::Full(_)) => Err(work.reject(ModuleError::Busy)),
            Err(mpsc::error::TrySendError::Closed(_)) => Err(work.reject(ModuleError::Stopped)),
        }
    }
    /// Failed completion/status enqueues; this does not claim frontend or media delivery.
    pub fn rejected_notices(&self) -> u64 {
        self.rejected_notices.load(Ordering::Relaxed)
    }
    /// Failed ordinary log enqueues; independent of lost completion/status notices.
    pub fn rejected_logs(&self) -> u64 {
        self.rejected_logs.load(Ordering::Relaxed)
    }

    /// Close new admission; already accepted work keeps its original ownership and budget.
    pub fn close(&self) {
        self.close_admission();
        self.stop.send_replace(true);
    }
    pub(crate) fn close_admission(&self) {
        *self
            .accepting
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = false;
    }
}

#[derive(Clone)]
struct LoopContext {
    modules: MainModules,
    producers: Arc<MessageProducers>,
    rejected_notices: Arc<AtomicU64>,
    rejected_logs: Arc<AtomicU64>,
    limit: usize,
    stopping: watch::Receiver<bool>,
}

/// Each future is run on its one assigned thread by the host; no hidden worker pool.
pub struct MainWorkers {
    /// Callback thread: quick facts, remembered/direct rules and immediate execution.
    pub fast: BoxFuture<'static, Result<(), ModuleError>>,
    /// Main thread: serial slow matching and one active human question.
    pub main: BoxFuture<'static, Result<(), ModuleError>>,
    /// Message thread: client queries, notifications and logging inside its module.
    pub messages: BoxFuture<'static, Result<(), ModuleError>>,
}

/// Build the main module once with explicit functional modules and the runtime queue budget.
pub fn main_dispatch(
    modules: MainModules,
    security_log: Box<dyn SecurityLogSink>,
    queues: &as_types::configuration::QueueSettings,
    shutdown: broadcast::Receiver<()>,
) -> (MainIngress, MainWorkers) {
    let (sender, receiving) = mpsc::channel(queues.callbacks.get());
    let (review, reviews) = mpsc::channel(queues.review.get());
    let (notices, messages) = mpsc::channel(queues.messages.get());
    let (logs, log_requests) = mpsc::channel(queues.messages.get());
    let producers = Arc::new(MessageProducers { notices, logs });
    let (stop, stopping) = watch::channel(false);
    let ingress = MainIngress {
        sender,
        accepting: Arc::new(Mutex::new(true)),
        rejected_notices: Arc::new(AtomicU64::new(0)),
        rejected_logs: Arc::new(AtomicU64::new(0)),
        stop,
        producers: Arc::downgrade(&producers),
    };
    let context = LoopContext {
        modules: modules.clone(),
        producers,
        rejected_notices: ingress.rejected_notices.clone(),
        rejected_logs: ingress.rejected_logs.clone(),
        limit: queues.callbacks.get(),
        stopping: stopping.clone(),
    };
    let workers = MainWorkers {
        fast: Box::pin(fast_loop(
            receiving,
            review,
            context.clone(),
            ingress.accepting.clone(),
            ingress.stop.clone(),
            stopping,
            shutdown,
        )),
        main: Box::pin(main_loop(reviews, context)),
        messages: Box::pin(async move {
            let persist = async {
                security_log.run(log_requests).await.map_err(|error| {
                    tracing::error!(%error, "security log consumer failed");
                    ModuleError::Unavailable
                })
            };
            let (notices, logs) = tokio::join!(modules.messages.run(messages), persist);
            notices.and(logs)
        }),
    };
    (ingress, workers)
}

async fn fast_loop(
    mut receiving: mpsc::Receiver<FastInput>,
    review: mpsc::Sender<Work>,
    context: LoopContext,
    accepting: Arc<Mutex<bool>>,
    stop: watch::Sender<bool>,
    mut stopping: watch::Receiver<bool>,
    mut shutdown: broadcast::Receiver<()>,
) -> Result<(), ModuleError> {
    let mut pending: FuturesUnordered<BoxFuture<'static, ()>> = FuturesUnordered::new();
    let mut closing = false;
    loop {
        tokio::select! { biased;
            _ = stopping.changed(), if !closing => { receiving.close(); closing = true; }
            _ = shutdown.recv(), if !closing => { *accepting.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = false; stop.send_replace(true); receiving.close(); closing = true; }
            Some(()) = pending.next(), if !pending.is_empty() => {}
            input = receiving.recv(), if pending.len() < context.limit => match input {
                Some(FastInput::Fence(done))=>{let _=done.send(());},
                Some(FastInput::DirectHits(batch))=>context.modules.fast_facts.record_direct_hits(batch),
                Some(FastInput::Retire(work,cause))=>{pending.push(Box::pin(fail(*work,context.clone(),cause)));},
                Some(FastInput::Work(work)) => {
                    let mut work=*work;
                    work.cancellation.facts=Some((context.modules.fast_facts.clone(),as_types::process_tree::EventKey::from(work.input.event.as_ref())));
                    let mut source_finished = false;
                    let preparation = (|| {
                        if let Some(operation) = &work.input.operation { operation.validate(&work.input.event)?; }
                        let facts = context.modules.fast_facts.prepare(&work.input.event);
                        source_finished = work.input.operation.as_ref().is_some_and(|operation| operation.try_result().is_some() || operation.hold_window().deadline() <= tokio::time::Instant::now());
                        match facts {
                            Ok(facts) if facts != ProcessInfo::default() => Arc::make_mut(&mut work.input.event).process.fill_missing(facts),
                            Ok(_) => {},
                            Err(error) if source_finished => tracing::warn!(%error,"completed original facts unavailable; actual result remains authoritative"),
                            Err(error) => return Err(error),
                        }
                        if let Some(hit)=&work.input.transfer_hit { context.modules.fast_facts.record_hit(&work.input.event,hit); }
                        if work.input.event.kind() != work.input.event.payload.kind() { return Err(ModuleError::InvalidInput); }
                        if source_finished || work.reply.is_closed() || work.input.operation.is_none() { return Ok(FastMatch::NoMatch); }
                        let rules = context.modules.rules.select_version(work.input.event.kind(), work.input.operation.as_ref().and_then(|v| v.policy_version()))?;
                        let decision = rules.fast(&work.input.event)?;
                        if matches!(&decision, FastMatch::Matched(choice) if choice.hit.is_none()) { return Err(ModuleError::InvalidInput); }
                        work.needs_facts = matches!(decision, FastMatch::NeedsFacts);
                        work.rules = Some(rules);
                        Ok::<_, ModuleError>(decision)
                    })();
                    if preparation.is_ok() && source_finished {
                        pending.push(Box::pin(finish_source(work,context.clone())));
                        continue;
                    }
                    if work.reply.is_closed() && work.input.operation.is_some() { pending.push(Box::pin(fail(work,context.clone(),ModuleError::Stopped))); continue; }
                    match preparation {
                        Err(error) => pending.push(Box::pin(fail(work,context.clone(),error))),
                        Ok(FastMatch::Matched(choice)) => pending.push(Box::pin(finish(work, context.clone(), Some(choice), false))),
                        Ok(FastMatch::NeedsFacts) if !work.rules.as_ref().is_some_and(|rules| rules.can_prompt()) => pending.push(Box::pin(fail(work,context.clone(),ModuleError::Unavailable))),
                        Ok(_) if work.input.operation.is_none() || !work.rules.as_ref().is_some_and(|rules| rules.can_prompt()) => {
                            let choice = work.input.operation.as_ref().map(|_| headless_choice());
                            pending.push(Box::pin(finish(work, context.clone(), choice, false)));
                        },
                        Ok(FastMatch::NoReviewMatch) => {
                            pending.push(Box::pin(finish(work, context.clone(), Some(no_match_choice()), false)));
                        },
                        Ok(_) => match review.try_send(work) {
                            Ok(()) => {},
                            Err(mpsc::error::TrySendError::Full(work)) => pending.push(Box::pin(fail(work,context.clone(),ModuleError::Busy))),
                            Err(mpsc::error::TrySendError::Closed(work)) => pending.push(Box::pin(fail(work,context.clone(),ModuleError::Stopped))),
                        },
                    }
                }
                None => break,
            },
        }
    }
    while pending.next().await.is_some() {}
    Ok(())
}

async fn main_loop(
    mut receiving: mpsc::Receiver<Work>,
    context: LoopContext,
) -> Result<(), ModuleError> {
    while let Some(work) = receiving.recv().await {
        // The driver retains all queued originals and their own cancellation/deadline.
        // Only this loop may enter slow matching and wait for the active question.
        if work.input.operation.as_ref().is_some_and(|operation| {
            operation.try_result().is_some()
                || operation.hold_window().deadline() <= tokio::time::Instant::now()
        }) {
            finish_source(work, context.clone()).await;
            continue;
        }
        if work.rules.as_ref().is_some_and(|rules| rules.can_prompt()) {
            finish(work, context.clone(), None, true).await;
        } else if work.needs_facts {
            fail(work, context.clone(), ModuleError::Unavailable).await;
        } else {
            finish(work, context.clone(), Some(headless_choice()), false).await;
        }
    }
    Ok(())
}

async fn fail(mut work: Work, context: LoopContext, cause: ModuleError) {
    let header = work.input.event.header();
    tracing::error!(event=?as_types::process_tree::EventKey::from(work.input.event.as_ref()),
        %cause,needs_facts=work.needs_facts,actor=?header.actor_tgid,
        cgroup=?header.actor_cgroup_id,lineage=?header.actor_cgroup_lineage,
        workload=?work.input.event.process.workload,"operation preparation failed");
    let retired = retire_preparation(&work.input, cause).await;
    work.cancellation.operation = None;
    queue_transfer_log(&context, &work.input, Err(cause));
    queue_completion(&context, &work.input, None, retired.actual).await;
    work.reply.send(Err(cause));
}

async fn finish_source(mut work: Work, context: LoopContext) {
    let result = match &work.input.operation {
        Some(operation) => operation.result().await.map(DispatchOutcome::Execution),
        None => Err(ModuleError::InvalidInput),
    };
    work.cancellation.operation = None;
    queue_transfer_log(&context, &work.input, result.clone());
    queue_completion(&context, &work.input, None, result.clone()).await;
    work.reply.send(result);
}

async fn finish(work: Work, context: LoopContext, choice: Option<RuleChoice>, complex: bool) {
    let Work {
        mut input,
        mut reply,
        mut cancellation,
        rules,
        needs_facts: _,
        _policy,
    } = work;
    let fast_counted = if let Some(hit) = choice.as_ref().and_then(|v| v.hit.as_ref()) {
        context.modules.fast_facts.record_hit(&input.event, hit);
        true
    } else {
        false
    };
    let cancellation_choice = choice.clone();
    let matched = std::sync::OnceLock::new();
    let prepared = tokio::select! { biased;
        _ = stopped(context.stopping.clone()) => None,
        _ = reply.closed() => {
            None
        },
        prepared = async {
            if complex && let Some(slow) = &context.modules.slow_facts {
                tokio::select! { biased;
                    reason = source_retired(input.operation.clone()) => return Err(reason),
                    result = slow.prepare(&input.event) => {
                        let facts = result?;
                        if facts != ProcessInfo::default() {
                            Arc::make_mut(&mut input.event).process.fill_missing(facts);
                        }
                    },
                }
            }
            if input.event.kind() != input.event.payload.kind() { return Err(ModuleError::InvalidInput); }
            let Some(operation) = &input.operation else { return Ok(None); };
            let choice = match choice {
                Some(choice) => choice,
                None => {
                    let rules = rules.ok_or(ModuleError::Unavailable)?;
                    let selected = if rules.can_prompt() { rules.prompt(ReviewRequest {
                        matched: &matched,
                        event: input.event.clone(), operation: operation.clone(),
                    }).await? } else { Some(match rules.fast(&input.event)? {
                        FastMatch::Matched(choice) => choice,
                        FastMatch::NoMatch | FastMatch::NoReviewMatch => headless_choice(),
                        FastMatch::NeedsFacts => return Err(ModuleError::Unavailable),
                    }) };
                    if selected.as_ref().is_some_and(|choice| choice.hit.is_none() && choice.reason != ChoiceReason::NoInteraction) { return Err(ModuleError::InvalidInput); }
                    selected.unwrap_or_else(no_match_choice)
                }
            };
            Ok::<_, ModuleError>(Some(choice))
        } => Some(prepared),
    };
    let Some(prepared) = prepared else {
        let retired = retire_preparation(&input, ModuleError::Stopped).await;
        let actual = retired.reply;
        cancellation.operation = None;
        if let Some(choice) = &cancellation_choice {
            queue_rule_log(&context, &input, choice, actual.clone());
        } else if let Some(hit) = matched.get() {
            context.modules.fast_facts.record_hit(&input.event, hit);
            queue_log(
                &context,
                &input,
                hit,
                RuleLogDecision::Pending,
                actual.clone(),
            );
        }
        queue_transfer_log(&context, &input, actual.clone());
        queue_completion(
            &context,
            &input,
            cancellation_choice.clone(),
            retired.actual,
        )
        .await;
        reply.send(actual);
        return;
    };
    let choice = match prepared {
        Ok(choice) => choice,
        Err(error) => {
            let retired = retire_preparation(&input, error).await;
            let actual = retired.reply;
            cancellation.operation = None;
            if let Some(hit) = matched.get() {
                context.modules.fast_facts.record_hit(&input.event, hit);
                queue_log(
                    &context,
                    &input,
                    hit,
                    RuleLogDecision::Pending,
                    actual.clone(),
                );
            }
            queue_transfer_log(&context, &input, actual.clone());
            queue_completion(&context, &input, cancellation_choice, retired.actual).await;
            reply.send(actual);
            return;
        }
    };
    let outcome = match (&input.operation, &choice) {
        (Some(operation), Some(choice)) => {
            if !fast_counted && let Some(hit) = &choice.hit {
                context.modules.fast_facts.record_hit(&input.event, hit);
            }
            let executing = async {
                if reply.is_closed() {
                    return Err(ModuleError::Stopped);
                }
                operation.validate(&input.event)?;
                if operation.stop_signal().as_ref().is_some_and(|v| {
                    *v.borrow() == Some(as_types::operation::StopReason::SourceFailure)
                }) {
                    return Err(ModuleError::Unavailable);
                }
                // Execution has begun: caller cancellation no longer drops its original work.
                operation.apply(&input.event, choice).await
            }
            .await;
            match executing {
                Ok(receipt) => DispatchOutcome::Execution(receipt),
                Err(error) => {
                    let retired = retire_preparation(&input, error).await;
                    cancellation.operation = None;
                    match retired.reply {
                        Ok(outcome) => outcome,
                        Err(error) => {
                            queue_rule_log(&context, &input, choice, Err(error));
                            queue_transfer_log(&context, &input, Err(error));
                            queue_completion(
                                &context,
                                &input,
                                Some(choice.clone()),
                                retired.actual,
                            )
                            .await;
                            reply.send(Err(error));
                            return;
                        }
                    }
                }
            }
        }
        _ => DispatchOutcome::Observed,
    };
    cancellation.operation = None;
    queue_transfer_log(&context, &input, Ok(outcome.clone()));
    if let Some(choice) = &choice {
        queue_rule_log(&context, &input, choice, Ok(outcome.clone()));
    }
    queue_completion(&context, &input, choice, Ok(outcome.clone())).await;
    reply.send(Ok(outcome));
}

struct Retired {
    reply: Result<DispatchOutcome, ModuleError>,
    actual: Result<DispatchOutcome, ModuleError>,
}
async fn retire_preparation(input: &DispatchInput, cause: ModuleError) -> Retired {
    let actual = match &input.operation {
        Some(operation) => {
            if operation.try_result().is_none() {
                operation.cancel();
            }
            operation.result().await.map(DispatchOutcome::Execution)
        }
        None => Err(cause),
    };
    tracing::debug!(%cause,actual=?actual,"processing ended; original completion collected without a new decision");
    let reply = if matches!(cause, ModuleError::Stopped | ModuleError::Unavailable) {
        actual.clone()
    } else {
        Err(cause)
    };
    Retired { reply, actual }
}
fn completion(
    input: &DispatchInput,
    choice: Option<RuleChoice>,
    result: Result<DispatchOutcome, ModuleError>,
) -> MainNotice {
    MainNotice {
        decision_id: input
            .operation
            .as_ref()
            .map(|operation| operation.decision_id()),
        event: input.event.clone(),
        choice,
        result,
    }
}
async fn queue_completion(
    context: &LoopContext,
    input: &DispatchInput,
    choice: Option<RuleChoice>,
    result: Result<DispatchOutcome, ModuleError>,
) {
    // The original processing context retains this completion while the existing
    // bounded consumer is occupied. Acceptance is neither frontend delivery nor SQL commit.
    if context
        .producers
        .notice(MainMessage::Completed(Box::new(completion(
            input, choice, result,
        ))))
        .await
        .is_err()
    {
        context.rejected_notices.fetch_add(1, Ordering::Relaxed);
        tracing::warn!("Main completion consumer closed; original operation result is unchanged");
    }
}
/// Retire a real source operation rejected before main admission, without issuing another decision.
pub(crate) async fn rejected_completion(input: DispatchInput, cause: ModuleError) -> MainNotice {
    let retired = retire_preparation(&input, cause).await;
    completion(&input, None, retired.actual)
}

fn queue_transfer_log(
    context: &LoopContext,
    input: &DispatchInput,
    result: Result<DispatchOutcome, ModuleError>,
) {
    if let Some(hit) = &input.transfer_hit {
        queue_log(context, input, hit, RuleLogDecision::Transferred, result);
    }
}

fn queue_rule_log(
    context: &LoopContext,
    input: &DispatchInput,
    choice: &RuleChoice,
    result: Result<DispatchOutcome, ModuleError>,
) {
    if let Some(hit) = &choice.hit {
        queue_log(
            context,
            input,
            hit,
            RuleLogDecision::Resolved {
                decision: choice.decision,
                reason: choice.reason,
            },
            result,
        );
    }
}
fn queue_log(
    context: &LoopContext,
    input: &DispatchInput,
    hit: &RuleHit,
    decision: RuleLogDecision,
    result: Result<DispatchOutcome, ModuleError>,
) {
    if !hit.log {
        return;
    }
    publish_log(
        Some(&context.producers),
        &context.rejected_logs,
        SecurityLogEntry::Rule(RuleLogRequest {
            decision_id: input.operation.as_ref().map(|v| v.decision_id()),
            event: input.event.clone(),
            hit: hit.clone(),
            decision,
            result,
        }),
    );
}

async fn stopped(mut stopping: watch::Receiver<bool>) {
    while !*stopping.borrow_and_update() {
        if stopping.changed().await.is_err() {
            break;
        }
    }
}

fn headless_choice() -> RuleChoice {
    RuleChoice {
        decision: Decision::Allow,
        hit: None,
        reason: ChoiceReason::NoInteraction,
    }
}

fn no_match_choice() -> RuleChoice {
    RuleChoice {
        decision: Decision::Allow,
        hit: None,
        reason: ChoiceReason::NoMatch,
    }
}

async fn source_retired(
    operation: Option<Arc<dyn as_types::operation::OriginalOperation>>,
) -> ModuleError {
    let Some(operation) = operation else {
        return std::future::pending().await;
    };
    let mut stopping = operation.stop_signal();
    loop {
        if let Some(reason) = stopping.as_mut().and_then(|v| *v.borrow_and_update()) {
            return match reason {
                as_types::operation::StopReason::SourceFailure => ModuleError::Unavailable,
                as_types::operation::StopReason::Orderly => ModuleError::Stopped,
            };
        }
        tokio::select! {
            _ = tokio::time::sleep_until(operation.hold_window().deadline()) => return ModuleError::Stopped,
            changed = async { match stopping.as_mut() { Some(v) => v.changed().await, None => std::future::pending().await } } => {
                if changed.is_err() { return ModuleError::Unavailable; }
            }
        }
    }
}
