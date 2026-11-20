//! In-memory data charging. No JSON encoding; excludes allocator bookkeeping and Arc headers.
use crate::{dispatch_event::ProcessInfo, flow::*, workload::Workload};
use std::mem::{size_of, size_of_val};

fn sum(values: impl IntoIterator<Item = usize>) -> usize {
    values.into_iter().fold(0, usize::saturating_add)
}
fn vector<T>(values: &Vec<T>, heap: impl Fn(&T) -> usize) -> usize {
    values
        .capacity()
        .saturating_mul(size_of::<T>())
        .saturating_add(sum(values.iter().map(heap)))
}
fn optional<T>(value: &Option<T>, heap: impl Fn(&T) -> usize) -> usize {
    value.as_ref().map(heap).unwrap_or(0)
}
fn string(value: &String) -> usize {
    value.capacity()
}
fn bytes(value: &Vec<u8>) -> usize {
    value.capacity()
}
fn native(value: &NativeProtocolId) -> usize {
    value
        .field_path
        .capacity()
        .saturating_add(match &value.value {
            NativeIdValue::JsonString(v) | NativeIdValue::JsonNumber(v) => v.capacity(),
        })
}
fn process(p: &ProcessInfo) -> usize {
    sum([
        optional(&p.namespace_tgids, |v| vector(v, |_| 0)),
        optional(&p.workload, |w| match w {
            Workload::Host => 0,
            Workload::Container(c) => sum([
                c.runtime.capacity(),
                optional(&c.namespace, string),
                c.id.capacity(),
            ]),
        }),
        optional(&p.name, bytes),
        optional(&p.executable, bytes),
        optional(&p.cwd, bytes),
        optional(&p.parent_name, bytes),
        optional(&p.parent_executable, bytes),
        optional(&p.argv, |v| vector(v, bytes)),
        optional(&p.parent_argv, |v| vector(v, bytes)),
    ])
}
fn source(s: &ProtocolSource) -> usize {
    sum([
        process(&s.actor.info),
        s.origin.capacity(),
        s.route_label.capacity(),
        s.request_traceparent.as_ref().map_or(0, String::capacity),
    ])
}
/// Logical JSON data already decoded by the provider parser; never reformatted for charging.
pub fn json_data_bytes(v: &serde_json::Value) -> usize {
    match v {
        serde_json::Value::Null | serde_json::Value::Bool(_) => 0,
        serde_json::Value::Number(n) => n.as_str().len(),
        serde_json::Value::String(s) => s.capacity(),
        serde_json::Value::Array(a) => vector(a, json_data_bytes),
        serde_json::Value::Object(o) => sum(o.iter().map(|(k, v)| {
            sum([
                size_of::<String>(),
                size_of::<serde_json::Value>(),
                k.capacity(),
                json_data_bytes(v),
            ])
        })),
    }
}
fn content(c: &ChatContent) -> usize {
    match c {
        ChatContent::Text(t) => t.capacity(),
        ChatContent::Json(v) => json_data_bytes(v),
    }
}
fn part(p: &ChatPart) -> usize {
    match p {
        ChatPart::Text { text, .. } => text.capacity(),
        ChatPart::ToolCall {
            native_id,
            name,
            arguments,
            ..
        } => sum([
            optional(native_id, native),
            name.capacity(),
            optional(arguments, content),
        ]),
        ChatPart::ToolResult {
            native_id,
            content: c,
            ..
        } => optional(native_id, native).saturating_add(content(c)),
        ChatPart::Opaque {
            provider_type,
            reason,
            ..
        } => provider_type.capacity().saturating_add(reason.capacity()),
        ChatPart::MediaRef {
            kind, safe_label, ..
        } => kind.capacity().saturating_add(safe_label.capacity()),
    }
}
impl ChatPart {
    /// Inline value and owned data capacity, excluding allocator overhead.
    pub fn data_bytes(&self) -> usize {
        size_of_val(self).saturating_add(part(self))
    }
}
fn body(b: &EvidenceBody) -> usize {
    vector(&b.segments, |s| match s {
        EvidenceSegment::Bytes { data, .. } => data.capacity(),
        EvidenceSegment::Missing { reason, .. } => reason.capacity(),
    })
}
fn session(observed: &SessionObservation) -> usize {
    let field = match &observed.field {
        SessionField::Header { name } => name.capacity(),
        SessionField::JsonPointer { pointer } => pointer.capacity(),
    };
    field.saturating_add(vector(&observed.values, |value| match value {
        SessionValue::Header(value) => value.capacity(),
        SessionValue::Json(NativeIdValue::JsonString(value) | NativeIdValue::JsonNumber(value)) => {
            value.capacity()
        }
    }))
}
fn message_heap(m: &ChatMessage) -> usize {
    sum([
        source(&m.source),
        vector(&m.record_refs, |_| 0),
        match &m.role {
            ChatRole::Other(r) => r.capacity(),
            _ => 0,
        },
        optional(&m.model, string),
        vector(&m.observed_parts, part),
        vector(&m.quality, |_| 0),
        optional(&m.provider_status, string),
        optional(&m.finish_reason, string),
        optional(&m.usage, |u| vector(u, |c| c.name.capacity())),
    ])
}
impl ChatMessage {
    /// Inline structure plus owned data capacities; not wire length, allocator overhead or RSS.
    pub fn data_bytes(&self) -> usize {
        size_of_val(self).saturating_add(message_heap(self))
    }
}
impl NativeIdValue {
    /// Original scalar storage; the lexical value is not reformatted.
    pub fn data_bytes(&self) -> usize {
        size_of_val(self).saturating_add(match self {
            Self::JsonString(s) | Self::JsonNumber(s) => s.capacity(),
        })
    }
}
impl crate::dispatch_event::EventData {
    /// Reachable event data, excluding allocator bookkeeping.
    pub fn retained_bytes(&self) -> usize {
        use crate::dispatch_event::EventPayload;
        size_of_val(self)
            .saturating_add(optional(&self.header().actor_cgroup_lineage, |ids| {
                vector(ids, |_| 0)
            }))
            .saturating_add(optional(&self.header().actor_namespace_tids, |ids| {
                vector(ids, |_| 0)
            }))
            .saturating_add(process(&self.process))
            .saturating_add(optional(&self.syscall, |snapshot| {
                vector(&snapshot.arguments, |_| 0)
            }))
            .saturating_add(optional(&self.kernel_stack, |snapshot| {
                vector(&snapshot.instruction_pointers, |_| 0)
            }))
            .saturating_add(optional(&self.user_stack, |value| {
                vector(&value.snapshot.instruction_pointers, |_| 0)
            }))
            .saturating_add(match &self.payload {
                EventPayload::Process(p) => optional(&p.target_executable, bytes)
                    .saturating_add(optional(&p.target_argv, |v| vector(v, bytes)))
                    .saturating_add(optional(&p.child_process, |c| {
                        size_of::<ProcessInfo>().saturating_add(process(c))
                    })),
                EventPayload::File(f) => {
                    optional(&f.path, bytes).saturating_add(optional(&f.new_path, bytes))
                }
                EventPayload::Network(n) => optional(&n.target_domain, bytes).saturating_add(
                    optional(&n.dns_snapshot, |snapshot| {
                        bytes(&snapshot.data).saturating_add(match &snapshot.analysis {
                            Some(crate::dispatch_event::DnsQueryAnalysis::Malformed { reason }) => {
                                reason.capacity()
                            }
                            _ => 0,
                        })
                    }),
                ),
            })
    }
}
impl crate::main_ports::RuleHit {
    /// The exact retained hit strings; no current-rule lookup or formatting.
    pub fn retained_bytes(&self) -> usize {
        sum([
            size_of_val(self),
            self.group_id.capacity(),
            self.rule_id.0.capacity(),
            optional(&self.deny_message, string),
        ])
    }
}
impl ChatEvent {
    /// Reachable current typed data charge; sharing does not copy the source records.
    pub fn data_bytes(&self) -> usize {
        size_of_val(self).saturating_add(match self {
            Self::Observation(o) => sum([
                size_of_val(o.as_ref()),
                source(&o.source),
                optional(&o.event_name, string),
                body(&o.body),
                vector(&o.native_ids, native),
                vector(&o.quality, |_| 0),
                optional(&o.request_session, session),
            ]),
            Self::CredentialUse(c) => sum([
                size_of_val(c.as_ref()),
                process(&c.actor.info),
                c.destination_origin.capacity(),
                optional(&c.auth_field, string),
                optional(&c.reason, string),
            ]),
            Self::RuleFeedback(f) => match &f.state {
                RuleFeedbackState::Applied { modified_body, .. } => body(modified_body),
                RuleFeedbackState::Prepared
                | RuleFeedbackState::Submitted { .. }
                | RuleFeedbackState::Failed { .. } => 0,
            },
            Self::Started(m) | Self::Finished(m) => m.data_bytes(),
            Self::Delta { changes, .. } => vector(changes, |c| match c {
                ChatDelta::AppendText { text, .. } => text.capacity(),
                ChatDelta::PutPart { part: p } => part(p),
            }),
            Self::ExchangeFailed { reason, .. } => reason.capacity(),
            Self::Gap(g) => g.kind.capacity().saturating_add(g.reason.capacity()),
            Self::Status(s) => vector(&s.decoder_profiles, |_| 0),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use as_kernel_common::event_frame::{
        EVENT_FRAME_HEADER_LEN, EVENT_FRAME_VERSION, EventFrameHeader,
    };
    use zerocopy::IntoBytes;
    #[test]
    fn created_child_owned_data_is_part_of_the_event_charge() {
        let header = EventFrameHeader {
            version: EVENT_FRAME_VERSION.into(),
            header_len: (EVENT_FRAME_HEADER_LEN as u16).into(),
            record_len: (EVENT_FRAME_HEADER_LEN as u32).into(),
            kind: (crate::dispatch_event::EventKind::Process as u16).into(),
            stage: (crate::dispatch_event::EventStage::Observation as u16).into(),
            operation: (crate::event_operation::ProcessOperation::Create as u32).into(),
            source_instance: [1; 16],
            sequence_domain: 1.into(),
            sequence: 1.into(),
            source_boottime_ns: 0.into(),
            actor_tgid: 0.into(),
            actor_tid: 0.into(),
            actor_birth_boottime_ns: 0.into(),
            actor_cgroup_id: 0.into(),
            available: 0.into(),
            common_process_len: 0.into(),
        };
        let mut event = crate::dispatch_event::decode(header.as_bytes()).unwrap();
        let before = event.retained_bytes();
        let crate::dispatch_event::EventPayload::Process(p) = &mut event.payload else {
            panic!("process payload")
        };
        p.child_process = Some(Box::new(ProcessInfo {
            name: Some(Vec::with_capacity(65536)),
            ..Default::default()
        }));
        assert!(event.retained_bytes() >= before + 65536 + size_of::<ProcessInfo>());
        let mut network_header = header;
        network_header.kind = (crate::dispatch_event::EventKind::Network as u16).into();
        network_header.operation = (crate::event_operation::NetworkOperation::Send as u32).into();
        network_header.stage = (crate::dispatch_event::EventStage::ControlledRequest as u16).into();
        let mut network_event = crate::dispatch_event::decode(network_header.as_bytes()).unwrap();
        let before = network_event.retained_bytes();
        let crate::dispatch_event::EventPayload::Network(network) = &mut network_event.payload
        else {
            panic!("network payload")
        };
        network.dns_snapshot = Some(crate::dispatch_event::DnsSendSnapshot {
            requested_bytes: 0,
            attempted_bytes: 0,
            send_flags: 0,
            data: Vec::with_capacity(65536),
            analysis: Some(crate::dispatch_event::DnsQueryAnalysis::Malformed {
                reason: String::with_capacity(4096),
            }),
        });
        assert!(network_event.retained_bytes() >= before + 65536 + 4096);
    }
}
