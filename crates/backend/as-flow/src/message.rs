//! The only mutable structured projection of one in-flight provider message.
use as_types::flow::*;
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};

#[derive(Default)]
struct ToolDraft {
    fragment: String,
    arguments: Option<ChatContent>,
    native_id: Option<NativeProtocolId>,
}

enum PartChange {
    Append { from: usize, text: String },
    Put(Option<ChatPart>),
    Native(Option<NativeProtocolId>),
}

pub(crate) struct Item {
    pub role: ChatRole,
    pub model: Option<String>,
    pub candidate: Option<u32>,
    pub state: ChatState,
    pub provider_status: Option<String>,
    pub finish: Option<String>,
    pub usage: Option<Vec<UsageCounter>>,
    pub parts: BTreeMap<u32, ChatPart>,
    tools: BTreeMap<u32, ToolDraft>,
    changes: BTreeMap<u32, PartChange>,
    dirty: bool,
    touched: bool,
    delivery: Option<Delivery>,
    pending_finish: Option<Arc<ChatMessage>>,
}

struct Delivery {
    id: ChatMessageId,
    revision: u64,
    first: u64,
    state: ChatState,
    refs: Vec<ProtocolRecordId>,
    quality: Vec<CaptureIssue>,
}

impl Item {
    pub fn new(role: ChatRole, model: Option<String>) -> Self {
        Self {
            role,
            model,
            candidate: None,
            state: ChatState::Streaming,
            provider_status: None,
            finish: None,
            usage: None,
            parts: BTreeMap::new(),
            tools: BTreeMap::new(),
            changes: BTreeMap::new(),
            dirty: false,
            touched: true,
            delivery: None,
            pending_finish: None,
        }
    }

    fn update<T: PartialEq>(slot: &mut T, value: T, dirty: &mut bool) {
        if *slot != value {
            *slot = value;
            *dirty = true;
        }
    }

    pub fn set_role(&mut self, role: ChatRole) {
        Self::update(&mut self.role, role, &mut self.dirty);
    }

    pub fn set_candidate(&mut self, index: u32) {
        Self::update(&mut self.candidate, Some(index), &mut self.dirty);
    }

    pub fn set_model(&mut self, model: Option<String>) {
        Self::update(&mut self.model, model, &mut self.dirty);
    }

    pub fn set_state(&mut self, state: ChatState) {
        Self::update(&mut self.state, state, &mut self.dirty);
    }

    pub fn set_provider_status(&mut self, status: Option<String>) {
        Self::update(&mut self.provider_status, status, &mut self.dirty);
    }

    pub fn set_finish(&mut self, finish: Option<String>) {
        Self::update(&mut self.finish, finish, &mut self.dirty);
    }

    pub fn set_usage(&mut self, usage: Option<Vec<UsageCounter>>) {
        Self::update(&mut self.usage, usage, &mut self.dirty);
    }

    pub fn touch(&mut self) {
        self.touched = true;
    }

    pub fn clear_parts(&mut self) {
        self.parts.clear();
        self.tools.clear();
        self.changes.clear();
    }

    pub fn put(&mut self, id: u32, part: ChatPart) {
        self.touch();
        if self.parts.get(&id) != Some(&part) {
            let previous = self.parts.insert(id, part);
            self.remember_replacement(id, previous);
        }
    }

    fn remember_replacement(&mut self, id: u32, previous: Option<ChatPart>) {
        let original = match self.changes.remove(&id) {
            Some(change) => change.restore(previous),
            None => previous,
        };
        self.changes.insert(id, PartChange::Put(original));
    }

    pub fn text(&mut self, id: u32, kind: TextKind, value: &str, append: bool) {
        self.touch();
        if append
            && let Some(ChatPart::Text {
                text,
                kind: old_kind,
                ..
            }) = self.parts.get_mut(&id)
            && *old_kind == kind
        {
            if !value.is_empty() {
                let from = text.len();
                text.push_str(value);
                if let PartChange::Append { text: delta, .. } = self
                    .changes
                    .entry(id)
                    .or_insert_with(|| PartChange::Append {
                        from,
                        text: String::new(),
                    })
                {
                    delta.push_str(value);
                }
            }
        } else {
            self.put(
                id,
                ChatPart::Text {
                    id,
                    kind,
                    text: value.into(),
                },
            );
        }
    }

    pub fn tool(&mut self, id: u32, name: Option<&str>, arguments: Option<&Value>, append: bool) {
        self.touch();
        let draft = self.tools.entry(id).or_default();
        let previous = self.parts.get(&id).and_then(|part| match part {
            ChatPart::ToolCall {
                name, native_id, ..
            } => Some((name.as_str(), native_id.as_ref())),
            _ => None,
        });
        let arguments = if append {
            if let Some(fragment) = arguments.and_then(Value::as_str) {
                draft.fragment.push_str(fragment);
                as_types::json::parse(draft.fragment.as_bytes())
                    .ok()
                    .map(ChatContent::Json)
            } else if let Some(ChatPart::ToolCall {
                name: current,
                arguments,
                ..
            }) = self.parts.get(&id)
            {
                if name.is_none_or(|name| name == current) {
                    return;
                }
                arguments.clone()
            } else {
                draft.arguments.take()
            }
        } else if let Some(Value::String(raw)) = arguments {
            draft.fragment.clear();
            let value = as_types::json::parse(raw.as_bytes())
                .ok()
                .map(ChatContent::Json);
            if self.state == ChatState::Streaming {
                draft.fragment.push_str(raw);
            }
            value
        } else {
            draft.fragment.clear();
            arguments.cloned().map(ChatContent::Json)
        };
        if let Some(name) = name.or(previous.map(|(name, _)| name)) {
            let native_id = previous
                .and_then(|(_, native)| native.cloned())
                .or_else(|| draft.native_id.take());
            self.put(
                id,
                ChatPart::ToolCall {
                    id,
                    native_id,
                    name: name.into(),
                    arguments,
                },
            );
        } else {
            draft.arguments = arguments;
        }
    }

    pub fn tool_result(&mut self, id: u32, value: &Value) {
        self.put(
            id,
            ChatPart::ToolResult {
                id,
                native_id: None,
                content: ChatContent::Json(value.clone()),
            },
        );
    }

    pub fn bind(&mut self, id: u32, native: &NativeProtocolId) {
        if let Some(slot) = self.parts.get_mut(&id).and_then(part_native) {
            if slot.as_ref() != Some(native) {
                let prior = slot.replace(native.clone());
                self.changes.entry(id).or_insert(PartChange::Native(prior));
                self.touch();
            }
        } else {
            self.tools.entry(id).or_default().native_id = Some(native.clone());
        }
    }

    pub fn opaque(&mut self, id: u32, value: Option<&Value>) {
        if let Some(ty) = value.and_then(Value::as_str) {
            self.put(
                id,
                ChatPart::Opaque {
                    id,
                    provider_type: ty.into(),
                    reason: "unsupported_content_type".into(),
                },
            );
        }
    }

    pub fn replace(&mut self, mut incoming: Self) {
        self.touch();
        self.set_role(incoming.role);
        self.set_model(incoming.model.take());
        // A provider snapshot is authoritative for the item's actual parts.
        let removed: Vec<_> = self
            .parts
            .keys()
            .filter(|id| !incoming.parts.contains_key(id))
            .copied()
            .collect();
        for id in removed {
            let previous = self.parts.remove(&id);
            self.remember_replacement(id, previous);
        }
        for (id, mut part) in incoming.parts {
            if let Some(slot) = part_native(&mut part)
                && slot.is_none()
                && let Some(
                    ChatPart::ToolCall { native_id, .. } | ChatPart::ToolResult { native_id, .. },
                ) = self.parts.get(&id)
            {
                *slot = native_id.clone();
            }
            self.put(id, part);
        }
        self.tools = incoming.tools;
    }

    pub fn valid_depth(&self, maximum: u32) -> bool {
        self.parts
            .values()
            .filter_map(|part| match part {
                ChatPart::ToolCall {
                    arguments: Some(ChatContent::Json(value)),
                    ..
                }
                | ChatPart::ToolResult {
                    content: ChatContent::Json(value),
                    ..
                } => Some(value),
                _ => None,
            })
            .all(|value| crate::observe::depth(value) <= maximum)
            && self.tools.values().all(|draft| match &draft.arguments {
                Some(ChatContent::Json(value)) => crate::observe::depth(value) <= maximum,
                _ => true,
            })
    }

    pub fn data_bytes(&self) -> usize {
        self.parts
            .values()
            .fold(0usize, |n, p| n.saturating_add(p.data_bytes()))
            .saturating_add(self.tools.values().fold(0usize, |n, d| {
                n.saturating_add(d.fragment.capacity())
                    .saturating_add(match &d.arguments {
                        Some(ChatContent::Json(value)) => json_data_bytes(value),
                        _ => 0,
                    })
            }))
            .saturating_add(self.model.as_ref().map_or(0, String::capacity))
            .saturating_add(self.provider_status.as_ref().map_or(0, String::capacity))
            .saturating_add(self.finish.as_ref().map_or(0, String::capacity))
            .saturating_add(match &self.role {
                ChatRole::Other(role) => role.capacity(),
                _ => 0,
            })
            .saturating_add(self.usage.as_ref().map_or(0, |counters| {
                counters.iter().fold(
                    counters
                        .capacity()
                        .saturating_mul(std::mem::size_of::<UsageCounter>()),
                    |n, counter| n.saturating_add(counter.name.capacity()),
                )
            }))
            .saturating_add(self.delivery.as_ref().map_or(0, |d| {
                d.refs
                    .capacity()
                    .saturating_mul(std::mem::size_of::<ProtocolRecordId>())
            }))
    }

    pub fn deliver(
        &mut self,
        id: ChatMessageId,
        source: &Arc<ProtocolSource>,
        record: ProtocolRecordId,
        now: u64,
    ) -> Option<(ChatEvent, u64)> {
        let first = self.delivery.as_ref().is_none_or(|d| d.revision == 0);
        let delivery = self.delivery.get_or_insert_with(|| Delivery {
            id,
            revision: 0,
            first: now,
            state: self.state,
            refs: Vec::new(),
            quality: Vec::new(),
        });
        if self.touched || self.dirty || !self.changes.is_empty() {
            self.dirty |= record_reference(&mut delivery.refs, &mut delivery.quality, record);
        }
        self.touched = false;
        let changes = std::mem::take(&mut self.changes);
        let changed = std::mem::take(&mut self.dirty);
        if !first && !changed && changes.is_empty() {
            return None;
        }
        let base_revision = delivery.revision;
        let revision = base_revision.saturating_add(1);
        if self.state != ChatState::Streaming {
            self.tools.clear();
        }
        if !first && self.state == ChatState::Streaming {
            let changes = changes
                .into_iter()
                .filter_map(|(part_id, change)| match change {
                    PartChange::Append { text, .. } => {
                        Some(ChatDelta::AppendText { part_id, text })
                    }
                    PartChange::Put(_) | PartChange::Native(_) => self
                        .parts
                        .get(&part_id)
                        .cloned()
                        .map(|part| ChatDelta::PutPart { part }),
                })
                .collect();
            return Some((
                ChatEvent::Delta {
                    id: delivery.id,
                    base_revision,
                    revision,
                    changes,
                },
                revision,
            ));
        }
        let message = Arc::new(self.snapshot(
            source.clone(),
            now,
            revision,
            self.parts.values().cloned().collect(),
        )?);
        if first {
            self.pending_finish = (self.state != ChatState::Streaming).then(|| message.clone());
            Some((ChatEvent::Started(message), revision))
        } else {
            Some((ChatEvent::Finished(message), revision))
        }
    }

    pub fn delivered(&mut self, revision: u64) {
        if let Some(d) = self.delivery.as_mut() {
            d.revision = revision;
            d.state = self.state;
        }
    }

    pub fn pending_finish(&self) -> Option<Arc<ChatMessage>> {
        self.pending_finish.clone()
    }

    pub fn finish_delivered(&mut self) {
        self.pending_finish = None;
    }

    fn snapshot(
        &self,
        source: Arc<ProtocolSource>,
        now: u64,
        revision: u64,
        observed_parts: Vec<ChatPart>,
    ) -> Option<ChatMessage> {
        let d = self.delivery.as_ref()?;
        Some(ChatMessage {
            id: d.id,
            revision,
            origin: if source.side == ProtocolSide::ClientRequest {
                ChatOrigin::RequestContext
            } else {
                ChatOrigin::ResponseOutput
            },
            source,
            record_refs: d.refs.clone(),
            role: self.role.clone(),
            candidate_index: self.candidate,
            model: self.model.clone(),
            observed_parts,
            state: self.state,
            quality: d.quality.clone(),
            provider_status: self.provider_status.clone(),
            finish_reason: self.finish.clone(),
            usage: self.usage.clone(),
            first_seen_boottime_ns: d.first,
            last_seen_boottime_ns: now,
        })
    }

    pub fn needs_settle(&self) -> bool {
        self.delivery.as_ref().is_some_and(|d| {
            d.revision != 0 && (self.pending_finish.is_some() || d.state == ChatState::Streaming)
        })
    }

    pub fn settle(
        mut self,
        source: Arc<ProtocolSource>,
        failed: bool,
        now: u64,
    ) -> Option<ChatEvent> {
        let d = self.delivery.as_ref()?;
        if d.revision == 0 {
            return None;
        }
        if let Some(message) = self.pending_finish.take() {
            return Some(ChatEvent::Finished(message));
        }
        if d.state != ChatState::Streaming {
            return None;
        }
        let revision = d.revision.saturating_add(1);
        self.state = if failed {
            ChatState::Failed
        } else {
            ChatState::Interrupted
        };
        let parts = std::mem::take(&mut self.parts).into_values().collect();
        Some(ChatEvent::Finished(Arc::new(
            self.snapshot(source, now, revision, parts)?,
        )))
    }

    pub fn gap(&mut self) {
        for (id, change) in std::mem::take(&mut self.changes) {
            if let Some(original) = change.restore(self.parts.remove(&id)) {
                self.parts.insert(id, original);
            }
        }
        self.tools.clear();
        if let Some(d) = self.delivery.as_mut()
            && !d.quality.contains(&CaptureIssue::Gap)
        {
            d.quality.push(CaptureIssue::Gap);
        }
    }
}

fn part_native(part: &mut ChatPart) -> Option<&mut Option<NativeProtocolId>> {
    match part {
        ChatPart::ToolCall { native_id, .. } | ChatPart::ToolResult { native_id, .. } => {
            Some(native_id)
        }
        _ => None,
    }
}

impl PartChange {
    fn restore(self, mut part: Option<ChatPart>) -> Option<ChatPart> {
        match self {
            Self::Put(original) => return original,
            Self::Append { from, .. } => {
                if let Some(ChatPart::Text { text, .. }) = part.as_mut() {
                    text.truncate(from);
                }
            }
            Self::Native(original) => {
                if let Some(id) = part.as_mut().and_then(part_native) {
                    *id = original;
                }
            }
        }
        part
    }
}

pub(crate) fn record_reference(
    refs: &mut Vec<ProtocolRecordId>,
    quality: &mut Vec<CaptureIssue>,
    record: ProtocolRecordId,
) -> bool {
    if refs.len() < 2048 {
        refs.push(record);
    } else if !quality.contains(&CaptureIssue::Truncated) {
        quality.push(CaptureIssue::Truncated);
        return true;
    }
    false
}

pub(crate) fn usage(v: &Value) -> Vec<UsageCounter> {
    v.as_object()
        .into_iter()
        .flatten()
        .filter_map(|(name, n)| {
            n.as_u64().map(|value| UsageCounter {
                name: name.clone(),
                value,
            })
        })
        .collect()
}
