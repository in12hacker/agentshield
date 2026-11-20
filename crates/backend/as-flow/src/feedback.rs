//! Explicit unfinished fixed-text writes. No completed-event, chat or source-history cache.
use crate::Shared;
use as_types::{
    flow::*,
    main_ports::{DispatchOutcome, MainNotice},
    operation::ExecutionOutcome,
    process_tree::EventKey,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
};
use tokio_util::sync::CancellationToken;

/// Actual credential lease and standard request authentication selectors; never a provider account.
pub(crate) fn auth_scope(
    origin: &HttpsOrigin,
    profile: ProviderProfile,
    route: RouteId,
    lease: &AuthHeaderLease,
    headers: &http::HeaderMap,
) -> [u8; 32] {
    fn bytes(hash: &mut Sha256, value: &[u8]) {
        hash.update((value.len() as u64).to_be_bytes());
        hash.update(value);
    }
    let mut hash = Sha256::new();
    bytes(&mut hash, b"AgentShield controlled TLS credential scope");
    bytes(&mut hash, origin.ascii_host.as_bytes());
    hash.update(origin.port.to_be_bytes());
    hash.update([match profile {
        ProviderProfile::OpenAiResponses => 1,
        ProviderProfile::OpenAiChatCompletions => 2,
        ProviderProfile::AnthropicMessages => 3,
    }]);
    hash.update(route.0);
    hash.update(lease.binding.0);
    hash.update(lease.secret.0);
    hash.update(lease.secret_revision.to_be_bytes());
    hash.update([match lease.slot() {
        AuthSlot::AuthorizationBearer => 1,
        AuthSlot::XApiKey => 2,
    }]);
    for name in ["openai-organization", "openai-project"] {
        let values = headers.get_all(name);
        hash.update((values.iter().count() as u64).to_be_bytes());
        for value in values {
            bytes(&mut hash, value.as_bytes());
        }
    }
    hash.finalize().into()
}
struct Transaction {
    _retention_guard: Option<Box<dyn Send + Sync>>,
    request: RuleFeedbackRequest,
    call_id: NativeIdValue,
    item_id: Option<NativeIdValue>,
    signature: Option<[u8; 32]>,
    response: NativeIdValue,
    conversations: Vec<NativeIdValue>,
    auth: [u8; 32],
    text: String,
    cancelled: CancellationToken,
    abort: CancellationToken,
    done: CancellationToken,
    // 0: publishing Prepared; 1: ready; 2: writer owns it; 3: unmodified result owns it.
    phase: AtomicU8,
}
// Own the unpublished entry across its first await. Once the real watcher is
// running, it takes responsibility for the ready entry instead.
struct PrepareGuard {
    shared: Arc<Shared>,
    transaction: Option<Arc<Transaction>>,
}
impl PrepareGuard {
    fn supervise(&mut self) -> Result<(), FlowError> {
        let transaction = self
            .transaction
            .as_ref()
            .cloned()
            .ok_or(FlowError::Failed)?;
        let pending = self.shared.feedback.lock().map_err(|_| FlowError::Failed)?;
        if self.shared.intake.load(Ordering::Acquire) != 0
            || !pending
                .entries
                .get(&transaction.request.denial)
                .is_some_and(|current| Arc::ptr_eq(current, &transaction))
        {
            // The caller's `?` drops this guard and locks feedback again.
            drop(pending);
            return Err(FlowError::Unavailable);
        }
        transaction.phase.store(1, Ordering::Release);
        let shared = self.shared.clone();
        self.shared.tasks.spawn(async move {
            let stopping = tokio::select! {biased;
                _=transaction.done.cancelled()=>return,
                _=transaction.cancelled.cancelled()=>false,
                _=shared.feedback_stop.cancelled()=>true,
            };
            if stopping && transaction.phase.load(Ordering::Acquire) == 2 {
                tokio::select! {biased;
                    _=transaction.done.cancelled()=>return,
                    _=transaction.cancelled.cancelled()=>{},
                    _=shared.force.cancelled()=>{},
                }
            }
            transaction.abort.cancel();
            if transaction.phase.load(Ordering::Acquire) != 2 {
                shared.complete_feedback(&transaction, interrupted(&transaction, &shared));
            }
        });
        self.transaction = None;
        drop(pending);
        Ok(())
    }
}
impl Drop for PrepareGuard {
    fn drop(&mut self) {
        if let Some(transaction) = self.transaction.take() {
            self.shared.remove_feedback(&transaction);
            transaction.abort.cancel();
            transaction.done.cancel();
        }
    }
}
pub(crate) struct Feedback {
    capacity: usize,
    entries: HashMap<EventKey, Arc<Transaction>>,
}
impl Feedback {
    pub fn new(capacity: NonZeroUsize) -> Self {
        Self {
            capacity: capacity.get(),
            entries: HashMap::new(),
        }
    }
    #[cfg(test)]
    pub(crate) fn active_count(&self) -> usize {
        self.entries.len()
    }
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}
pub(crate) struct Port(pub Arc<Shared>);
fn denied(n: &MainNotice) -> bool {
    matches!(
        &n.result,
        Ok(DispatchOutcome::Driver {
            outcome: ExecutionOutcome::Denied,
            ..
        })
    ) || matches!(&n.result,Ok(DispatchOutcome::Execution(r))if r.outcome==ExecutionOutcome::Denied)
}
fn usable(v: &NativeIdValue) -> bool {
    match v {
        NativeIdValue::JsonString(s) => !s.is_empty(),
        NativeIdValue::JsonNumber(_) => true,
    }
}
#[async_trait::async_trait]
impl FlowFeedbackPort for Port {
    async fn prepare(
        &self,
        input: RuleFeedbackPrepareInput,
    ) -> Result<RuleFeedbackFact, FlowError> {
        match self.0.intake.load(Ordering::Acquire) {
            0 => {}
            1 => return Err(FlowError::Unavailable),
            _ => return Err(FlowError::Stopped),
        }
        let RuleFeedbackPrepareInput {
            notice,
            call,
            message,
            observations,
            cancelled,
            retention_guard,
        } = input;
        if !denied(&notice)
            || call.message != message.id
            || message.origin != ChatOrigin::ResponseOutput
        {
            return Err(FlowError::InvalidInput);
        }
        if message.source.profile != ProviderProfile::OpenAiResponses {
            return Err(FlowError::Unsupported);
        }
        let auth = message
            .source
            .actual_auth_scope
            .ok_or(FlowError::Unsupported)?;
        let text = notice
            .choice
            .as_ref()
            .and_then(|c| c.hit.as_ref())
            .and_then(|h| h.deny_message.as_ref())
            .filter(|s| !s.is_empty())
            .ok_or(FlowError::InvalidInput)?
            .clone();
        let native = message
            .observed_parts
            .iter()
            .find_map(|p| match p {
                ChatPart::ToolCall {
                    id,
                    native_id: Some(n),
                    ..
                } if *id == call.part_id
                    && n.namespace == NativeIdNamespace::ToolCall
                    && usable(&n.value) =>
                {
                    Some(n)
                }
                _ => None,
            })
            .ok_or(FlowError::Unsupported)?;
        let mut responses = Vec::new();
        let mut conversations = Vec::new();
        let signature = message.observed_parts.iter().find_map(|part| match part {
            ChatPart::ToolCall {
                id,
                name,
                arguments: Some(arguments),
                ..
            } if *id == call.part_id => call_signature(name, arguments),
            _ => None,
        });
        let mut item_ids = Vec::new();
        let mut original_call = false;
        let mut ambiguous=message.observed_parts.iter().filter(|p|matches!(p,ChatPart::ToolCall{native_id:Some(n),..}if n.namespace==NativeIdNamespace::ToolCall&&n.value==native.value)).count()>1;
        for o in &observations {
            if o.id.exchange != message.id.exchange || o.source != message.source {
                return Err(FlowError::InvalidInput);
            }
            let contains_call =
                message.record_refs.contains(&o.id) && o.native_ids.contains(native);
            original_call |= contains_call;
            if contains_call && let Some(parent) = native.field_path.strip_suffix("/call_id") {
                let item_path = format!("{parent}/id");
                for id in &o.native_ids {
                    if id.namespace == NativeIdNamespace::ProviderEvent
                        && id.field_path == item_path
                        && !item_ids.contains(&id.value)
                    {
                        item_ids.push(id.value.clone());
                    }
                }
            }
            ambiguous |= o
                .native_ids
                .iter()
                .filter(|id| {
                    id.namespace == NativeIdNamespace::ToolCall && id.value == native.value
                })
                .count()
                > 1;
            for id in &o.native_ids {
                let list = match id.namespace {
                    NativeIdNamespace::Response => &mut responses,
                    NativeIdNamespace::Conversation => &mut conversations,
                    _ => continue,
                };
                if usable(&id.value) && !list.contains(&id.value) {
                    list.push(id.value.clone());
                }
            }
        }
        if !original_call || responses.is_empty() {
            return Err(FlowError::Unsupported);
        }
        ambiguous |= responses.len() != 1 || conversations.len() > 1;
        let request = RuleFeedbackRequest {
            denial: EventKey::from(notice.event.as_ref()),
            call,
        };
        let fact = RuleFeedbackFact {
            denial: request.denial,
            call,
            state: RuleFeedbackState::Prepared,
        };
        if ambiguous || item_ids.len() > 1 {
            return Ok(RuleFeedbackFact {
                state: RuleFeedbackState::Failed {
                    original: None,
                    code: RuleFeedbackFailure::AmbiguousTarget,
                },
                ..fact
            });
        }
        if cancelled.is_cancelled() {
            return Err(FlowError::Unavailable);
        }
        if text
            .len()
            .saturating_add(native.value.data_bytes())
            .saturating_add(responses[0].data_bytes())
            .saturating_add(item_ids.first().map_or(0, NativeIdValue::data_bytes))
            .saturating_add(
                conversations
                    .iter()
                    .map(NativeIdValue::data_bytes)
                    .sum::<usize>(),
            )
            .saturating_add(std::mem::size_of::<Transaction>())
            > self.0.settings.decode_limits.max_connection_buffer_bytes as usize
        {
            return Err(FlowError::Busy);
        }
        let transaction = Arc::new(Transaction {
            _retention_guard: retention_guard,
            request,
            call_id: native.value.clone(),
            item_id: item_ids.pop(),
            signature,
            response: responses.remove(0),
            conversations,
            auth,
            text,
            cancelled,
            abort: CancellationToken::new(),
            done: CancellationToken::new(),
            phase: AtomicU8::new(0),
        });
        // Validation has ended. Original event, message bodies and records are released.
        drop(observations);
        drop(message);
        drop(notice);
        {
            let mut pending = self.0.feedback.lock().map_err(|_| FlowError::Failed)?;
            if self.0.intake.load(Ordering::Acquire) != 0 {
                return Err(FlowError::Unavailable);
            }
            if pending.entries.contains_key(&request.denial)
                || pending.entries.values().any(|t| t.request.call == call)
            {
                return Err(FlowError::Conflict);
            }
            if pending.entries.len() >= pending.capacity {
                return Err(FlowError::Busy);
            }
            pending.entries.insert(request.denial, transaction.clone());
        }
        let mut preparing = PrepareGuard {
            shared: self.0.clone(),
            transaction: Some(transaction.clone()),
        };
        tokio::select! {biased;
            _=transaction.cancelled.cancelled()=>Err(FlowError::Unavailable),
            _=self.0.feedback_stop.cancelled()=>Err(FlowError::Unavailable),
            result=self.0.publish(ChatEvent::RuleFeedback(fact.clone()))=>result,
        }?;
        preparing.supervise()?;
        Ok(fact)
    }
}
fn interrupted(t: &Transaction, shared: &Shared) -> RuleFeedbackState {
    if t.cancelled.is_cancelled() {
        RuleFeedbackState::Failed {
            original: None,
            code: RuleFeedbackFailure::Cancelled,
        }
    } else if shared.feedback_stop.is_cancelled() {
        RuleFeedbackState::Failed {
            original: None,
            code: RuleFeedbackFailure::SourceUnavailable,
        }
    } else {
        RuleFeedbackState::Failed {
            original: None,
            code: RuleFeedbackFailure::TransportError,
        }
    }
}
fn native_value(v: &Value) -> Option<NativeIdValue> {
    match v {
        Value::String(s) => Some(NativeIdValue::JsonString(s.clone())),
        Value::Number(n) => Some(NativeIdValue::JsonNumber(n.to_string())),
        _ => None,
    }
}
// Only a projection of this unfinished request's explicit protocol references.
// It owns no body or argument copy, and is shared with the original HTTP observer.
pub(crate) struct RequestTarget {
    pub original: ProtocolRecordId,
    previous: Option<NativeIdValue>,
    conversation: Option<NativeIdValue>,
    references_valid: bool,
    calls: Vec<ReplayedCall>,
    outputs: Vec<NativeIdValue>,
}
struct ReplayedCall {
    input_index: usize,
    id: NativeIdValue,
    item: Option<NativeIdValue>,
    signature: Option<[u8; 32]>,
    valid: bool,
}
fn optional_id(value: Option<&Value>) -> Result<Option<NativeIdValue>, ()> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => native_value(value).map(Some).ok_or(()),
    }
}
fn call_signature(name: &str, arguments: &ChatContent) -> Option<[u8; 32]> {
    let mut hash = Sha256::new();
    hash.update(b"AS original function call fields");
    hash.update((name.len() as u64).to_be_bytes());
    hash.update(name.as_bytes());
    match arguments {
        ChatContent::Text(value) => {
            hash.update([0]);
            hash.update(value.as_bytes());
        }
        ChatContent::Json(value) => {
            hash.update([1]);
            serde_json::to_writer(&mut hash, value).ok()?;
        }
    }
    Some(hash.finalize().into())
}

impl RequestTarget {
    pub(crate) fn new<'a>(
        original: ProtocolRecordId,
        json: &Value,
        parts: impl Iterator<Item = &'a ChatPart>,
    ) -> Self {
        let previous = optional_id(json.get("previous_response_id"));
        let conversation = match json.get("conversation") {
            Some(Value::Object(value)) => match value.get("id") {
                Some(value) => optional_id(Some(value)),
                None => Err(()),
            },
            value => optional_id(value),
        };
        let mut target = Self {
            original,
            references_valid: previous.is_ok() && conversation.is_ok(),
            previous: previous.ok().flatten(),
            conversation: conversation.ok().flatten(),
            calls: Vec::new(),
            outputs: Vec::new(),
        };
        for (input_index, item) in json
            .get("input")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            let Some(id) = item.get("call_id").and_then(native_value) else {
                continue;
            };
            match item.get("type").and_then(Value::as_str) {
                Some("function_call") => {
                    let item_id = optional_id(item.get("id"));
                    let valid = item_id.is_ok();
                    target.calls.push(ReplayedCall {
                        input_index,
                        id,
                        item: item_id.ok().flatten(),
                        signature: None,
                        valid,
                    });
                }
                Some("function_call_output") => target.outputs.push(id),
                _ => {}
            }
        }
        for part in parts {
            if let ChatPart::ToolCall {
                native_id: Some(native),
                name,
                arguments: Some(arguments),
                ..
            } = part
            {
                let Some(index) = native
                    .field_path
                    .strip_prefix("/input/")
                    .and_then(|path| path.strip_suffix("/call_id"))
                    .and_then(|index| index.parse::<usize>().ok())
                else {
                    continue;
                };
                if let Ok(position) = target
                    .calls
                    .binary_search_by_key(&index, |call| call.input_index)
                {
                    let call = &mut target.calls[position];
                    if call.id != native.value || call.signature.is_some() {
                        call.valid = false;
                    } else {
                        call.signature = call_signature(name, arguments);
                    }
                }
            }
        }
        target
    }
    pub(crate) fn data_bytes(&self) -> usize {
        fn heap(id: &NativeIdValue) -> usize {
            id.data_bytes()
                .saturating_sub(std::mem::size_of::<NativeIdValue>())
        }
        let fixed = std::mem::size_of::<Self>()
            .saturating_add(
                self.calls
                    .capacity()
                    .saturating_mul(std::mem::size_of::<ReplayedCall>()),
            )
            .saturating_add(
                self.outputs
                    .capacity()
                    .saturating_mul(std::mem::size_of::<NativeIdValue>()),
            );
        self.previous
            .iter()
            .chain(self.conversation.iter())
            .chain(self.outputs.iter())
            .chain(
                self.calls
                    .iter()
                    .flat_map(|call| std::iter::once(&call.id).chain(call.item.iter())),
            )
            .fold(fixed, |bytes, id| bytes.saturating_add(heap(id)))
    }
    fn matches(&self, t: &Transaction) -> bool {
        if !self.references_valid
            || self.previous.as_ref().is_some_and(|id| id != &t.response)
            || self
                .conversation
                .as_ref()
                .is_some_and(|id| !t.conversations.contains(id))
        {
            return false;
        }
        let mut calls = self.calls.iter().filter(|call| call.id == t.call_id);
        let replayed = if let Some(call) = calls.next() {
            if calls.next().is_some()
                || !call.valid
                || call
                    .item
                    .as_ref()
                    .zip(t.item_id.as_ref())
                    .is_some_and(|(actual, original)| actual != original)
                || call.signature.is_none()
                || call.signature != t.signature
            {
                return false;
            }
            true
        } else {
            false
        };
        self.previous.is_some() || replayed
    }
}

impl Shared {
    fn remove_feedback(&self, transaction: &Arc<Transaction>) -> Option<Arc<Transaction>> {
        let mut pending = self.feedback.lock().ok()?;
        if !pending
            .entries
            .get(&transaction.request.denial)
            .is_some_and(|current| Arc::ptr_eq(current, transaction))
        {
            return None;
        }
        pending.entries.remove(&transaction.request.denial)
    }
    // A terminal fact retains no original event, rule text or source record bodies.
    fn complete_feedback(
        self: &Arc<Self>,
        transaction: &Arc<Transaction>,
        state: RuleFeedbackState,
    ) {
        let Some(t) = self.remove_feedback(transaction) else {
            return;
        };
        t.done.cancel();
        let fact = RuleFeedbackFact {
            denial: t.request.denial,
            call: t.request.call,
            state,
        };
        drop(t);
        let shared = self.clone();
        self.tasks.spawn(async move {
            let _ = shared.publish(ChatEvent::RuleFeedback(fact)).await;
        });
    }
    pub(crate) fn has_feedback(&self, auth: &[u8; 32]) -> bool {
        self.feedback.lock().is_ok_and(|s| {
            s.entries
                .values()
                .any(|t| t.auth == *auth && t.phase.load(Ordering::Acquire) == 1)
        })
    }
    pub(crate) fn unmodified_feedback(
        self: &Arc<Self>,
        target: &RequestTarget,
        reason: RuleFeedbackFailure,
        auth: Option<[u8; 32]>,
    ) {
        let Some(auth) = auth else { return };
        let entries: Vec<_> = match self.feedback.lock() {
            Ok(s) => s
                .entries
                .values()
                .filter(|t| t.auth == auth && t.phase.load(Ordering::Acquire) == 1)
                .cloned()
                .collect(),
            Err(_) => return,
        };
        for t in entries
            .into_iter()
            .filter(|t| target.matches(t) && target.outputs.contains(&t.call_id))
        {
            if t.phase
                .compare_exchange(1, 3, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                continue;
            }
            self.complete_feedback(
                &t,
                RuleFeedbackState::Failed {
                    original: Some(target.original),
                    code: reason,
                },
            );
        }
    }
    pub(crate) async fn modify_feedback(
        self: &Arc<Self>,
        decoded: crate::observe::DecodedRequest,
    ) -> Result<Option<Modified>, FlowError> {
        let crate::observe::DecodedRequest {
            observation,
            mut json,
            target,
        } = decoded;
        let Some(target) = target else {
            return Ok(None);
        };
        let Some(auth) = observation.source.actual_auth_scope else {
            return Ok(None);
        };
        if observation.source.profile != ProviderProfile::OpenAiResponses {
            return Ok(None);
        }
        let original = observation.id;
        let original_len = observation
            .body
            .original_length
            .and_then(|value| usize::try_from(value).ok())
            .ok_or(FlowError::InvalidInput)?;
        let candidates: Vec<_> = {
            let s = self.feedback.lock().map_err(|_| FlowError::Failed)?;
            s.entries
                .values()
                .filter(|t| t.auth == auth && t.phase.load(Ordering::Acquire) == 1)
                .cloned()
                .collect()
        };
        let candidates = candidates
            .into_iter()
            .filter(|t| target.matches(t))
            .collect::<Vec<_>>();
        drop(observation);
        let mut plans = Vec::new();
        for t in &candidates {
            let positions: Vec<_> = json
                .get("input")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
                .filter_map(|(i, v)| {
                    (v.get("type").and_then(Value::as_str) == Some("function_call_output")
                        && v.get("call_id").and_then(native_value).as_ref() == Some(&t.call_id))
                    .then_some(i)
                })
                .collect();
            if positions.is_empty() {
                continue;
            }
            if t.phase
                .compare_exchange(1, 2, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                continue;
            }
            let code = if t.cancelled.is_cancelled() {
                Some(RuleFeedbackFailure::Cancelled)
            } else if positions.len() != 1
                || candidates.iter().filter(|c| c.call_id == t.call_id).count() != 1
            {
                Some(RuleFeedbackFailure::AmbiguousTarget)
            } else {
                None
            };
            if let Some(code) = code {
                self.complete_feedback(
                    t,
                    RuleFeedbackState::Failed {
                        original: Some(original),
                        code,
                    },
                );
                continue;
            }
            let output = json
                .get_mut("input")
                .and_then(Value::as_array_mut)
                .and_then(|a| a.get_mut(positions[0]))
                .and_then(|v| v.get_mut("output"));
            if let Some(output) = output
                && append(output, &t.text)
            {
                plans.push(t.clone());
            } else {
                self.complete_feedback(
                    t,
                    RuleFeedbackState::Failed {
                        original: Some(original),
                        code: RuleFeedbackFailure::UnsupportedBody,
                    },
                );
            }
        }
        if plans.is_empty() {
            return Ok(None);
        }
        let mut guard = WriteGuard {
            shared: self.clone(),
            plans,
            original,
            done: false,
            finished: CancellationToken::new(),
        };
        let bytes = match serde_json::to_vec(&json) {
            Ok(v) => v,
            Err(_) => {
                guard.failed(RuleFeedbackFailure::UnsupportedBody);
                return Err(FlowError::Failed);
            }
        };
        if bytes.len() > self.settings.decode_limits.max_message_bytes as usize
            || bytes.len().saturating_add(original_len)
                > self.settings.decode_limits.max_connection_buffer_bytes as usize
            || crate::observe::depth(&json) > self.settings.decode_limits.max_json_depth
        {
            guard.failed(RuleFeedbackFailure::Capacity);
            return Ok(None);
        }
        let evidence = Arc::new(EvidenceBody::from_bytes(bytes.clone()));
        for t in &guard.plans {
            self.publish(ChatEvent::RuleFeedback(RuleFeedbackFact {
                denial: t.request.denial,
                call: t.request.call,
                state: RuleFeedbackState::Applied {
                    original,
                    modified_body: evidence.clone(),
                },
            }))
            .await?;
        }
        Ok(Some(Modified { bytes, guard }))
    }
}
fn append(v: &mut Value, text: &str) -> bool {
    match v {
        Value::String(s) => {
            if !s.is_empty() {
                s.push('\n')
            }
            s.push_str(text);
            true
        }
        Value::Array(a)
            if a.iter().all(|v| {
                v.get("type").and_then(Value::as_str) == Some("input_text")
                    && v.get("text").is_some_and(Value::is_string)
            }) =>
        {
            a.push(serde_json::json!({"type":"input_text","text":text}));
            true
        }
        _ => false,
    }
}
pub(crate) struct Modified {
    pub bytes: Vec<u8>,
    pub guard: WriteGuard,
}
pub(crate) struct WriteGuard {
    shared: Arc<Shared>,
    plans: Vec<Arc<Transaction>>,
    original: ProtocolRecordId,
    done: bool,
    finished: CancellationToken,
}
impl WriteGuard {
    pub fn cancelled(&self) -> bool {
        !self.done
            && self
                .plans
                .iter()
                .any(|t| t.cancelled.is_cancelled() || t.abort.is_cancelled())
    }
    pub fn cancellations(&self) -> (Vec<CancellationToken>, CancellationToken) {
        (
            self.plans
                .iter()
                .flat_map(|t| [t.cancelled.clone(), t.abort.clone()])
                .collect(),
            self.finished.clone(),
        )
    }
    pub fn submitted(&mut self) {
        self.finish(None);
    }
    pub fn failed(&mut self, code: RuleFeedbackFailure) {
        self.finish(Some(code));
    }
    fn finish(&mut self, failure: Option<RuleFeedbackFailure>) {
        if self.done {
            return;
        }
        self.done = true;
        self.finished.cancel();
        for t in std::mem::take(&mut self.plans) {
            let state = if t.cancelled.is_cancelled() || t.abort.is_cancelled() {
                let mut state = interrupted(&t, &self.shared);
                if let RuleFeedbackState::Failed { original, .. } = &mut state {
                    *original = Some(self.original)
                }
                state
            } else {
                match failure {
                    None => RuleFeedbackState::Submitted {
                        original: self.original,
                    },
                    Some(code) => RuleFeedbackState::Failed {
                        original: Some(self.original),
                        code,
                    },
                }
            };
            self.shared.complete_feedback(&t, state);
        }
    }
}
impl Drop for WriteGuard {
    fn drop(&mut self) {
        if !self.done {
            self.finish(Some(RuleFeedbackFailure::TransportError));
        }
    }
}

#[cfg(test)]
mod admission_tests {
    use super::*;

    #[tokio::test]
    async fn feedback_stop_seals_registration_before_a_published_prepare_can_resume() {
        let (mut shared, mut events) = crate::tests::protocol_fixture();
        let settings = &mut Arc::get_mut(&mut shared).expect("fixture owner").settings;
        Arc::make_mut(settings).egress = Some(TlsEgressSettings {
            ca: CaRef([1; 16]),
            max_connections: 1,
            max_streams_per_connection: 1,
        });
        let request = RuleFeedbackRequest {
            denial: EventKey {
                source_instance: [1; 16],
                sequence_domain: 1,
                sequence: 1,
            },
            call: ChatPartRef {
                message: ChatMessageId {
                    exchange: ExchangeKey {
                        flow_instance: shared.instance,
                        sequence: 1,
                    },
                    sequence: 1,
                },
                part_id: 0,
            },
        };
        let transaction = Arc::new(Transaction {
            _retention_guard: None,
            request,
            call_id: NativeIdValue::JsonString("call".into()),
            item_id: None,
            signature: None,
            response: NativeIdValue::JsonString("response".into()),
            conversations: Vec::new(),
            auth: [1; 32],
            text: "fixed".into(),
            cancelled: CancellationToken::new(),
            abort: CancellationToken::new(),
            done: CancellationToken::new(),
            phase: AtomicU8::new(0),
        });
        shared
            .feedback
            .lock()
            .expect("pending")
            .entries
            .insert(request.denial, transaction.clone());
        let mut preparing = PrepareGuard {
            shared: shared.clone(),
            transaction: Some(transaction.clone()),
        };
        shared
            .publish(ChatEvent::RuleFeedback(RuleFeedbackFact {
                denial: request.denial,
                call: request.call,
                state: RuleFeedbackState::Prepared,
            }))
            .await
            .expect("actual Prepared publication");
        // A different runtime can complete stop after publication and before this
        // synchronous handoff. Exercise those exact production boundaries in order.
        let module = crate::FlowModule {
            shared: shared.clone(),
            cancel: CancellationToken::new(),
            worker: None,
        };
        module.stop().await.expect("actual stop");
        assert!(shared.tasks.is_closed() && shared.tasks.is_empty());
        assert!(
            matches!(preparing.supervise(), Err(FlowError::Unavailable)),
            "stop receipt prohibits a late supervisor registration"
        );
        assert!(shared.tasks.is_empty());
        drop(preparing);
        assert!(transaction.abort.is_cancelled() && transaction.done.is_cancelled());
        assert_eq!(
            shared
                .feedback
                .lock()
                .expect("sealed entries")
                .active_count(),
            0
        );
        assert!(matches!(
            events
                .try_recv()
                .expect("previous real publication")
                .event
                .as_ref(),
            ChatEvent::RuleFeedback(RuleFeedbackFact {
                state: RuleFeedbackState::Prepared,
                ..
            })
        ));
        assert!(events.try_recv().is_err());
    }
}
