//! One body observer feeds both original protocol records and message projections.
use crate::{
    Shared, boottime,
    protocol::{Codec, Sse},
};
use as_types::flow::*;
use serde_json::Value;
use std::{
    sync::{Arc, atomic::Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

pub(crate) struct Observer {
    shared: Arc<Shared>,
    source: Arc<ProtocolSource>,
    key: ExchangeKey,
    content: Option<crate::content::Content>,
    body: Vec<u8>,
    pending_observation: Option<Arc<ProtocolObservation>>,
    sse: Option<Sse>,
    codec: Codec,
    records: u64,
    feedback_record: Option<Arc<crate::feedback::RequestTarget>>,
    disabled: bool,
    failed: bool,
    http_status: Option<u16>,
    state: BodyState,
    unmodified: Option<RuleFeedbackFailure>,
    initial_gap: Option<&'static str>,
    session: Option<SessionField>,
    header_session: Vec<SessionValue>,
}

pub(crate) struct DecodedRequest {
    pub observation: Arc<ProtocolObservation>,
    pub json: Value,
    pub target: Option<Arc<crate::feedback::RequestTarget>>,
}

enum BodyState {
    Reading,
    Eof,
    Interrupted,
    Retired,
}

impl Observer {
    pub(crate) fn transport_identity(&self) -> (ExchangeKey, ProtocolSide) {
        (self.key, self.source.side)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        shared: Arc<Shared>,
        facts: Arc<IngressFacts>,
        key: ExchangeKey,
        route: ProviderRoute,
        side: ProtocolSide,
        use_id: Option<CredentialUseId>,
        actual_auth_scope: Option<[u8; 32]>,
        headers: &http::HeaderMap,
        http_status: Option<u16>,
    ) -> Self {
        let encoding = headers
            .get(http::header::CONTENT_ENCODING)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("identity");
        let sse = headers
            .get(http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.split(';').next() == Some("text/event-stream"));
        let limit = shared.settings.decode_limits.max_message_bytes as usize;
        let content = crate::content::Content::new(encoding, limit);
        let disabled = !shared.settings.capture_content || content.is_none();
        let mut traceparents = headers.get_all("traceparent").iter();
        let request_traceparent = if side == ProtocolSide::ClientRequest {
            traceparents
                .next()
                .and_then(|header| header.to_str().ok())
                .filter(|value| traceparents.next().is_none() && traceparent_ids(value).is_some())
                .map(str::to_owned)
        } else {
            None
        };
        let session = (side == ProtocolSide::ClientRequest)
            .then_some(route.session)
            .flatten();
        let header_session = match &session {
            Some(SessionField::Header { name }) => headers
                .get_all(name)
                .iter()
                .map(|value| SessionValue::Header(value.as_bytes().to_vec()))
                .collect(),
            _ => Vec::new(),
        };
        let source = Arc::new(ProtocolSource {
            actor: facts.connection_initiator.clone(),
            channel: Some(facts.channel.clone()),
            side,
            request_traceparent,
            profile: route.profile,
            origin: route.origin,
            route_label: format!("{:02x?}", route.id.0),
            config_version: shared.version,
            observed_at_utc_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .ok()
                .and_then(|t| i64::try_from(t.as_millis()).ok()),
            credential_use: use_id,
            actual_auth_scope,
        });
        let profile = source.profile;
        let shared_capture = shared.settings.capture_content;
        let content_supported = content.is_some();
        Self {
            shared,
            source,
            key,
            content,
            body: Vec::new(),
            pending_observation: None,
            sse: if sse { Some(Sse::new(limit)) } else { None },
            codec: Codec::new(profile),
            records: if side == ProtocolSide::ClientRequest {
                0
            } else {
                1u64 << 63
            },
            feedback_record: None,
            disabled,
            failed: false,
            http_status,
            state: BodyState::Reading,
            unmodified: None,
            initial_gap: if disabled && shared_capture {
                Some(if content_supported {
                    "capture_disabled"
                } else {
                    "unsupported_content_encoding"
                })
            } else {
                None
            },
            session,
            header_session,
        }
    }
    /// This original body is handed to the HTTP sender; no later plan may chase it.
    pub(crate) fn unmodified_submission(&mut self, reason: RuleFeedbackFailure) {
        if self.source.side != ProtocolSide::ClientRequest {
            return;
        }
        let reason = *self.unmodified.get_or_insert(reason);
        if let Some(target) = &self.feedback_record {
            self.shared
                .unmodified_feedback(target, reason, self.source.actual_auth_scope);
        }
    }
    pub async fn feed(&mut self, bytes: &[u8]) -> Result<(), FlowError> {
        if let Some(reason) = self.initial_gap.take() {
            self.gap(reason).await?;
        }

        if self.disabled {
            return Ok(());
        }
        for chunk in bytes.chunks(16 * 1024) {
            let decoded = match self.content.as_mut().map(|c| c.feed(chunk)) {
                Some(Ok(v)) => v,
                _ => {
                    self.fail("content_decode").await?;
                    return Ok(());
                }
            };
            self.feed_decoded(&decoded).await?;
            if self.disabled {
                break;
            }
        }

        Ok(())
    }
    async fn feed_decoded(&mut self, bytes: &[u8]) -> Result<(), FlowError> {
        if self.disabled {
            return Ok(());
        }
        if let Some(sse) = self.sse.as_mut() {
            for unit in sse.feed(bytes) {
                match unit {
                    Ok(unit) => {
                        self.unit(unit.raw, Some(unit.data), unit.event).await?;
                    }
                    Err(reason) => self.fail(reason).await?,
                }
                if self.disabled {
                    break;
                }
            }
        } else if self.body.len().saturating_add(bytes.len())
            <= self.shared.settings.decode_limits.max_message_bytes as usize
        {
            self.body.extend_from_slice(bytes);
        } else {
            // This decoded input has already been obtained. Transfer it with
            // the retained prefix rather than discard it at the parse limit.
            self.body.extend_from_slice(bytes);
            self.fail("json_body_too_large").await?;
        }

        Ok(())
    }
    pub async fn finish(mut self, transport_complete: bool) -> Result<(), FlowError> {
        let _ = self.finish_record(transport_complete).await?;

        Ok(())
    }
    pub async fn finish_record(
        &mut self,
        transport_complete: bool,
    ) -> Result<Option<DecodedRequest>, FlowError> {
        if matches!(self.state, BodyState::Retired) {
            return Ok(None);
        }
        self.state = if transport_complete {
            BodyState::Eof
        } else {
            BodyState::Interrupted
        };
        if let Some(reason) = self.initial_gap.take() {
            self.gap(reason).await?;
        }

        if !transport_complete {
            if let Some(retirement) = self.retire(Some("transport_interrupted")) {
                retirement.await.map_err(|_| FlowError::Failed)??;
            }
            return Ok(None);
        }

        if !self.disabled {
            match self.content.as_mut().map(|c| c.finish()) {
                Some(Ok(tail)) => self.feed_decoded(&tail).await?,
                _ => self.fail("content_truncated").await?,
            }
        }
        if !self.disabled
            && let Some(unit) = self.sse.as_mut().and_then(Sse::finish)
        {
            self.unit(unit.raw, Some(unit.data), unit.event).await?;
        }
        let mut decoded = None;
        if !self.disabled && self.sse.is_none() {
            let raw = std::mem::take(&mut self.body);
            if !raw.is_empty() {
                decoded = self.unit(raw, None, None).await?;
            }
        }
        if !self.disabled && self.sse.as_ref().is_some_and(Sse::incomplete) {
            self.fail("incomplete_sse_event").await?;
        }
        if let Some(retirement) = self.retire(None) {
            retirement.await.map_err(|_| FlowError::Failed)??;
        }
        Ok(decoded)
    }

    fn next_record(&mut self) -> ProtocolRecordId {
        let id = ProtocolRecordId {
            exchange: self.key,
            sequence: self.records,
        };
        self.records = self.records.saturating_add(1);
        id
    }

    fn observation(
        &mut self,
        body: EvidenceBody,
        event_name: Option<String>,
        streaming: bool,
        native_ids: Vec<NativeProtocolId>,
        quality: Vec<CaptureIssue>,
    ) -> Arc<ProtocolObservation> {
        Arc::new(ProtocolObservation {
            id: self.next_record(),
            source: self.source.clone(),
            unit: if streaming {
                ProtocolUnit::SseEvent
            } else {
                ProtocolUnit::HttpJsonBody
            },
            event_name,
            body,
            native_ids,
            quality,
            request_session: self.session.take().map(|field| {
                let values = std::mem::take(&mut self.header_session);
                let issue = match (&field, values.len()) {
                    (SessionField::JsonPointer { .. }, _) => {
                        Some(SessionCaptureIssue::UnparsedBody)
                    }
                    (_, 0) => Some(SessionCaptureIssue::Missing),
                    (_, 1) => None,
                    _ => Some(SessionCaptureIssue::MultipleValues),
                };
                SessionObservation {
                    field,
                    values,
                    issue,
                }
            }),
        })
    }

    async fn unit(
        &mut self,
        raw: Vec<u8>,
        data: Option<Vec<u8>>,
        event: Option<String>,
    ) -> Result<Option<DecodedRequest>, FlowError> {
        let streaming = data.is_some();
        let data = data.as_deref().unwrap_or(&raw);
        if streaming
            && self.source.profile == ProviderProfile::OpenAiChatCompletions
            && data == b"[DONE]"
        {
            let observation = self.observation(
                EvidenceBody::from_bytes(raw),
                event,
                true,
                Vec::new(),
                Vec::new(),
            );
            let id = observation.id;
            self.publish_observation(observation).await?;
            for item in self.codec.items.values_mut() {
                if item.state == ChatState::Streaming {
                    item.set_state(ChatState::Complete);
                }
            }
            self.emit_items(id).await?;
            return Ok(None);
        }
        let parsed = as_types::json::parse(data).map_err(|_| "invalid_json");
        let body = EvidenceBody::from_bytes(raw);
        let parsed = match parsed {
            Ok(v) if depth(&v) <= self.shared.settings.decode_limits.max_json_depth => v,
            other => {
                let reason = other.err().unwrap_or("json_depth");
                let observation =
                    self.observation(body, event, streaming, Vec::new(), vec![CaptureIssue::Gap]);
                self.publish_observation(observation).await?;
                self.fail(reason).await?;
                return Ok(None);
            }
        };
        let mut ids = Vec::new();
        native_ids(&parsed, self.source.profile, self.source.side, &mut ids);
        let mut observation = self.observation(body, event, streaming, ids, Vec::new());
        if let Some(session) = Arc::get_mut(&mut observation)
            .ok_or(FlowError::Failed)?
            .request_session
            .as_mut()
            && let SessionField::JsonPointer { pointer } = &session.field
        {
            let original = parsed.pointer(pointer);
            let value = match original {
                Some(Value::String(value)) => Some(NativeIdValue::JsonString(value.clone())),
                Some(Value::Number(value)) => {
                    Some(NativeIdValue::JsonNumber(value.as_str().to_owned()))
                }
                _ => None,
            };
            session.issue = if value.is_some() {
                None
            } else if original.is_some() {
                Some(SessionCaptureIssue::UnsupportedValue)
            } else {
                Some(SessionCaptureIssue::Missing)
            };
            session.values.extend(value.map(SessionValue::Json));
        }
        let id = observation.id;
        self.publish_observation(observation.clone()).await?;
        if self.http_status.is_some_and(|s| s >= 400) {
            self.shared
                .publish(ChatEvent::ExchangeFailed {
                    exchange: self.key,
                    http_status: self.http_status,
                    reason: "upstream_http_error".into(),
                })
                .await?;
            return Ok(None);
        }
        if self.source.side == ProtocolSide::ClientRequest {
            self.codec.request(&parsed, &observation.native_ids);
        } else {
            self.codec.response(
                &parsed,
                observation.event_name.as_deref(),
                streaming,
                &observation.native_ids,
            );
            if self.codec.provider_failed {
                self.shared
                    .publish(ChatEvent::ExchangeFailed {
                        exchange: self.key,
                        http_status: self.http_status,
                        reason: "provider_error".into(),
                    })
                    .await?;
            }
        }
        if self.source.side == ProtocolSide::ClientRequest
            && self
                .source
                .actual_auth_scope
                .as_ref()
                .is_some_and(|auth| self.shared.has_feedback(auth))
        {
            self.feedback_record = Some(Arc::new(crate::feedback::RequestTarget::new(
                id,
                &parsed,
                self.codec
                    .items
                    .values()
                    .flat_map(|item| item.parts.values()),
            )));
            if let Some(reason) = self.unmodified {
                self.unmodified_submission(reason);
            }
        }
        if self
            .codec
            .items
            .values()
            .any(|item| !item.valid_depth(self.shared.settings.decode_limits.max_json_depth))
        {
            self.fail("decoded_json_depth_budget").await?;
            return Ok(None);
        }
        let bytes = self
            .codec
            .items
            .values()
            .fold(0usize, |n, item| n.saturating_add(item.data_bytes()))
            .saturating_add(
                self.feedback_record
                    .as_ref()
                    .map_or(0, |target| target.data_bytes()),
            );
        if bytes
            > self
                .shared
                .settings
                .decode_limits
                .max_connection_buffer_bytes as usize
        {
            self.unmodified_submission(RuleFeedbackFailure::Capacity);
            self.feedback_record = None;
            self.fail("message_budget").await?;
            return Ok(None);
        }
        self.emit_items(id).await?;
        if !streaming && self.source.side == ProtocolSide::ClientRequest {
            Ok(Some(DecodedRequest {
                observation,
                json: parsed,
                target: self.feedback_record.clone(),
            }))
        } else {
            Ok(None)
        }
    }

    async fn emit_items(&mut self, record: ProtocolRecordId) -> Result<(), FlowError> {
        let base = if self.source.side == ProtocolSide::ClientRequest {
            0
        } else {
            1u64 << 63
        };
        for (index, item) in self.codec.items.values_mut().enumerate() {
            let id = ChatMessageId {
                exchange: self.key,
                sequence: base + index as u64,
            };
            if let Some((event, revision)) = item.deliver(id, &self.source, record, boottime()) {
                self.shared.publish(event).await?;
                item.delivered(revision);
                if let Some(finish) = item.pending_finish() {
                    self.shared.publish(ChatEvent::Finished(finish)).await?;
                    item.finish_delivered();
                }
            }
        }
        Ok(())
    }
    async fn gap(&self, reason: &str) -> Result<(), FlowError> {
        self.shared
            .publish(ChatEvent::Gap(ObservationGap {
                kind: "capture".into(),
                exchange: Some(self.key),
                cursor_range: None,
                reason: reason.into(),
            }))
            .await?;

        Ok(())
    }
    async fn publish_observation(
        &mut self,
        observation: Arc<ProtocolObservation>,
    ) -> Result<(), FlowError> {
        // This is one in-flight reference, not a replay store. If the caller is
        // cancelled during backpressure, retire owns the unaccepted original.
        self.pending_observation = Some(observation.clone());
        let result = self
            .shared
            .publish(ChatEvent::Observation(observation))
            .await;
        self.pending_observation = None;
        result
    }

    async fn fail(&mut self, reason: &'static str) -> Result<(), FlowError> {
        self.failed = true;
        let retirement = self.retire(Some(reason));
        self.disabled = true;
        self.shared.parse_failures.fetch_add(1, Ordering::Relaxed);
        if let Some(retirement) = retirement {
            retirement.await.map_err(|_| FlowError::Failed)??;
        }

        Ok(())
    }

    fn retire(
        &mut self,
        gap: Option<&'static str>,
    ) -> Option<tokio::task::JoinHandle<Result<(), FlowError>>> {
        self.state = BodyState::Retired;
        let pending = self.pending_observation.take();
        let items = std::mem::take(&mut self.codec.items);
        if gap.is_none()
            && pending.is_none()
            && !items.values().any(crate::message::Item::needs_settle)
        {
            return None;
        }
        let partial =
            if gap.is_some() && !self.disabled && self.sse.is_none() && !self.body.is_empty() {
                let data = std::mem::take(&mut self.body);
                Some(self.observation(
                    EvidenceBody {
                        original_length: None,
                        segments: vec![EvidenceSegment::Bytes { offset: 0, data }],
                    },
                    None,
                    false,
                    Vec::new(),
                    vec![CaptureIssue::Gap],
                ))
            } else {
                None
            };
        let shared = self.shared.clone();
        let key = self.key;
        let failed = self.failed;
        let source = self.source.clone();
        Some(self.shared.tasks.spawn(async move {
            if let Some(pending) = pending {
                shared.publish(ChatEvent::Observation(pending)).await?;
            }
            if let Some(partial) = partial {
                shared.publish(ChatEvent::Observation(partial)).await?;
            }
            if let Some(reason) = gap {
                shared
                    .publish(ChatEvent::Gap(ObservationGap {
                        kind: "capture".into(),
                        exchange: Some(key),
                        cursor_range: None,
                        reason: reason.into(),
                    }))
                    .await?;
            }
            for mut item in items
                .into_values()
                .filter(crate::message::Item::needs_settle)
            {
                if gap.is_some() {
                    item.gap();
                }
                if let Some(event) = item.settle(source.clone(), failed, boottime()) {
                    shared.publish(event).await?;
                }
            }
            Ok(())
        }))
    }
}
impl Drop for Observer {
    fn drop(&mut self) {
        let reason = match self.state {
            BodyState::Reading => "body_dropped_before_eof",
            BodyState::Eof => "body_finalization_cancelled",
            BodyState::Interrupted => "transport_interrupted",
            BodyState::Retired => return,
        };

        let _retirement = self.retire(Some(reason));
    }
}

pub(crate) fn depth(v: &Value) -> u32 {
    match v {
        Value::Array(a) => 1 + a.iter().map(depth).max().unwrap_or(0),
        Value::Object(o) => 1 + o.values().map(depth).max().unwrap_or(0),
        _ => 1,
    }
}
pub(crate) fn native_ids(
    v: &Value,
    profile: ProviderProfile,
    side: ProtocolSide,
    out: &mut Vec<NativeProtocolId>,
) {
    fn add(v: &Value, p: &str, ns: NativeIdNamespace, out: &mut Vec<NativeProtocolId>) {
        let Some(v) = v.pointer(p) else { return };
        let value = match v {
            Value::String(s) => NativeIdValue::JsonString(s.clone()),
            Value::Number(n) => NativeIdValue::JsonNumber(n.as_str().into()),
            _ => return,
        };
        out.push(NativeProtocolId {
            namespace: ns,
            field_path: p.into(),
            value,
        });
    }
    let request = side == ProtocolSide::ClientRequest;
    match profile {
        ProviderProfile::OpenAiChatCompletions => {
            if !request {
                add(v, "/id", NativeIdNamespace::Response, out);
            }
            let messages = if request {
                v.get("messages")
            } else {
                v.get("choices")
            };
            if let Some(messages) = messages.and_then(Value::as_array) {
                for (n, m) in messages.iter().enumerate() {
                    let base = if request {
                        format!("/messages/{n}")
                    } else if m.get("delta").is_some() {
                        format!("/choices/{n}/delta")
                    } else {
                        format!("/choices/{n}/message")
                    };
                    add(
                        v,
                        &format!("{base}/tool_call_id"),
                        NativeIdNamespace::ToolResultReference,
                        out,
                    );
                    if let Some(calls) = v
                        .pointer(&format!("{base}/tool_calls"))
                        .and_then(Value::as_array)
                    {
                        for (i, _) in calls.iter().enumerate() {
                            add(
                                v,
                                &format!("{base}/tool_calls/{i}/id"),
                                NativeIdNamespace::ToolCall,
                                out,
                            );
                        }
                    }
                }
            }
        }
        ProviderProfile::OpenAiResponses => {
            add(
                v,
                "/previous_response_id",
                NativeIdNamespace::PreviousResponse,
                out,
            );
            add(v, "/conversation", NativeIdNamespace::Conversation, out);
            add(v, "/conversation/id", NativeIdNamespace::Conversation, out);
            if !request {
                add(v, "/id", NativeIdNamespace::Response, out);
                add(v, "/response/id", NativeIdNamespace::Response, out);
                add(v, "/response_id", NativeIdNamespace::Response, out);
                add(v, "/item_id", NativeIdNamespace::ProviderEvent, out);
            }
            for base in ["/input", "/output", "/response/output"] {
                if let Some(items) = v.pointer(base).and_then(Value::as_array) {
                    for (n, item) in items.iter().enumerate() {
                        let prefix = format!("{base}/{n}");
                        match item.get("type").and_then(Value::as_str) {
                            Some("function_call") => add(
                                v,
                                &format!("{prefix}/call_id"),
                                NativeIdNamespace::ToolCall,
                                out,
                            ),
                            Some("function_call_output") => add(
                                v,
                                &format!("{prefix}/call_id"),
                                NativeIdNamespace::ToolResultReference,
                                out,
                            ),
                            _ => {}
                        }
                        if !request {
                            add(
                                v,
                                &format!("{prefix}/id"),
                                NativeIdNamespace::ProviderEvent,
                                out,
                            );
                        }
                    }
                }
            }
            if !request {
                add(v, "/item/id", NativeIdNamespace::ProviderEvent, out);
                if v.pointer("/item/type").and_then(Value::as_str) == Some("function_call") {
                    add(v, "/item/call_id", NativeIdNamespace::ToolCall, out);
                }
            }
        }
        ProviderProfile::AnthropicMessages => {
            if !request {
                add(v, "/id", NativeIdNamespace::Message, out);
                add(v, "/message/id", NativeIdNamespace::Message, out);
            }
            fn blocks(v: &Value, base: &str, out: &mut Vec<NativeProtocolId>) {
                if let Some(blocks) = v.pointer(base).and_then(Value::as_array) {
                    for (n, block) in blocks.iter().enumerate() {
                        match block.get("type").and_then(Value::as_str) {
                            Some("tool_use") => add(
                                v,
                                &format!("{base}/{n}/id"),
                                NativeIdNamespace::ToolCall,
                                out,
                            ),
                            Some("tool_result") => add(
                                v,
                                &format!("{base}/{n}/tool_use_id"),
                                NativeIdNamespace::ToolResultReference,
                                out,
                            ),
                            _ => {}
                        }
                    }
                }
            }
            blocks(v, "/content", out);
            blocks(v, "/message/content", out);
            if let Some(messages) = v.get("messages").and_then(Value::as_array) {
                for (n, _) in messages.iter().enumerate() {
                    blocks(v, &format!("/messages/{n}/content"), out);
                }
            }
            match v.pointer("/content_block/type").and_then(Value::as_str) {
                Some("tool_use") => add(v, "/content_block/id", NativeIdNamespace::ToolCall, out),
                Some("tool_result") => add(
                    v,
                    "/content_block/tool_use_id",
                    NativeIdNamespace::ToolResultReference,
                    out,
                ),
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::record_reference;
    #[test]
    fn capped_record_references_keep_explicit_truncation() {
        let mut refs = Vec::new();
        let mut quality = Vec::new();
        for sequence in 0..2049 {
            record_reference(
                &mut refs,
                &mut quality,
                ProtocolRecordId {
                    exchange: ExchangeKey {
                        flow_instance: [1; 16],
                        sequence: 1,
                    },
                    sequence,
                },
            );
        }
        assert_eq!(refs.len(), 2048);
        assert_eq!(quality, vec![CaptureIssue::Truncated]);
        let mut latest = quality.clone();
        let exchange = refs[0].exchange;
        record_reference(
            &mut refs,
            &mut latest,
            ProtocolRecordId {
                exchange,
                sequence: 2049,
            },
        );
        assert_eq!(latest, quality);
    }

    #[test]
    fn multiple_tool_calls_refusal_and_obtained_null_keep_real_fields() {
        let value = serde_json::json!({"id":"response","choices":[{"index":0,"message":{"role":"assistant","content":"text","refusal":"refused","tool_calls":[{"id":"one","function":{"name":"","arguments":"null"}},{"id":"two","function":{"name":"second","arguments":"{}"}}]},"finish_reason":"tool_calls"}]});
        let mut ids = Vec::new();
        native_ids(
            &value,
            ProviderProfile::OpenAiChatCompletions,
            ProtocolSide::UpstreamResponse,
            &mut ids,
        );
        let mut codec = Codec::new(ProviderProfile::OpenAiChatCompletions);
        codec.response(&value, None, false, &ids);
        let projected = &codec.items[0].parts;
        let calls: Vec<_> = projected
            .values()
            .filter(|p| matches!(p, ChatPart::ToolCall { .. }))
            .collect();
        assert_eq!(calls.len(), 2);
        assert!(
            matches!(calls[0],ChatPart::ToolCall{name,arguments:Some(ChatContent::Json(Value::Null)),native_id:Some(id),..}if name.is_empty()&&id.field_path=="/choices/0/message/tool_calls/0/id")
        );
        assert!(
            matches!(calls[1],ChatPart::ToolCall{name,native_id:Some(id),..}if name=="second"&&id.field_path=="/choices/0/message/tool_calls/1/id")
        );
        assert!(
            projected.values().any(
                |p| matches!(p,ChatPart::Text{kind:TextKind::Refusal,text,..}if text=="refused")
            )
        );
        let mut responses = Codec::new(ProviderProfile::OpenAiResponses);
        responses.response(&serde_json::json!({"type":"response.refusal.delta","output_index":0,"content_index":0,"delta":"refused"}),None,true,&[]);
        assert!(responses.items[0].parts.values().any(|p| matches!(
            p,
            ChatPart::Text {
                kind: TextKind::Refusal,
                ..
            }
        )));
        responses.response(&serde_json::json!({"output":[{"type":"message","role":"assistant","content":[{"type":"refusal","refusal":"refused"}]}]}),None,false,&[]);
        assert!(responses.items[0].parts.values().any(|p| matches!(
            p,
            ChatPart::Text {
                kind: TextKind::Refusal,
                ..
            }
        )));
        let request = serde_json::json!({"messages":[{"role":"tool","tool_call_id":"one","content":null},{"role":"tool","tool_call_id":"two"}]});
        let mut codec = Codec::new(ProviderProfile::OpenAiChatCompletions);
        codec.request(&request, &[]);
        assert!(matches!(
            codec.items[0].parts.get(&0),
            Some(ChatPart::ToolResult {
                content: ChatContent::Json(Value::Null),
                ..
            })
        ));
        assert!(codec.items[1].parts.is_empty());
    }

    #[test]
    fn native_ids_preserve_real_scalar_types_without_scanning_business_arguments() {
        let value:Value=serde_json::from_str(r#"{"id":"literal-id","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":123456789012345678901234567890,"function":{"arguments":"{\"id\":\"business\"}"}}]}}]}"#).expect("json");
        let mut ids = Vec::new();
        native_ids(
            &value,
            ProviderProfile::OpenAiChatCompletions,
            ProtocolSide::UpstreamResponse,
            &mut ids,
        );
        assert!(
            ids.iter()
                .any(|i| i.value == NativeIdValue::JsonString("literal-id".into()))
        );
        assert!(
            ids.iter()
                .any(|i| i.field_path == "/choices/0/delta/tool_calls/0/id"
                    && i.value
                        == NativeIdValue::JsonNumber("123456789012345678901234567890".into()))
        );
        assert_eq!(ids.len(), 2);
        let value = serde_json::json!({"type":"message","id":"real-message","content":[{"type":"tool_use","id":"real-tool","input":{"call_id":"business-123","id":"invoice-42"}}]});
        let mut ids = Vec::new();
        native_ids(
            &value,
            ProviderProfile::AnthropicMessages,
            ProtocolSide::UpstreamResponse,
            &mut ids,
        );
        assert_eq!(ids.len(), 2);
        assert!(!ids.iter().any(|i| i.field_path.contains("input")));
    }
}
