//! Structured spawn receipts carried by original Responses requests.
use as_types::flow::{
    EvidenceSegment, NativeIdNamespace, NativeIdValue, NativeProtocolId, ProtocolObservation,
    ProtocolSide, ProtocolUnit, ProviderProfile,
};
use serde::Deserialize;
use serde_json::value::RawValue;
use std::collections::HashMap;

/// The original child identity reported by an accepted OpenClaw spawn.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenClawSpawnReceipt {
    status: String,
    /// Original child run ID.
    pub run_id: String,
    /// Original child session key.
    pub child_session_key: String,
}

impl OpenClawSpawnReceipt {
    /// Whether the original tool reported acceptance with both child identifiers.
    pub fn accepted(&self) -> bool {
        self.status == "accepted" && !self.run_id.is_empty() && !self.child_session_key.is_empty()
    }
}

pub(crate) struct ReceiptPosition {
    pub(crate) call_index: usize,
    pub(crate) output_index: usize,
    pub(crate) tool_call_id: String,
    pub(crate) receipt: OpenClawSpawnReceipt,
}

#[derive(Deserialize)]
struct Request<'a> {
    #[serde(borrow)]
    input: Vec<&'a RawValue>,
}

#[derive(Deserialize)]
struct Call {
    #[serde(rename = "type")]
    kind: String,
    name: String,
    call_id: String,
}

#[derive(Deserialize)]
struct Output {
    #[serde(rename = "type")]
    kind: String,
    call_id: String,
    output: String,
}

fn input_index(id: &NativeProtocolId) -> Option<usize> {
    id.field_path
        .strip_prefix("/input/")?
        .strip_suffix("/call_id")?
        .parse()
        .ok()
}

fn positions(request: Request<'_>, ids: &[NativeProtocolId]) -> Vec<ReceiptPosition> {
    let mut calls = HashMap::new();
    let mut outputs = HashMap::new();
    for id in ids {
        let positions = match id.namespace {
            NativeIdNamespace::ToolCall => &mut calls,
            NativeIdNamespace::ToolResultReference => &mut outputs,
            _ => continue,
        };
        let (NativeIdValue::JsonString(value), Some(index)) = (&id.value, input_index(id)) else {
            continue;
        };
        positions
            .entry(value.as_str())
            .and_modify(|position| *position = None)
            .or_insert(Some(index));
    }
    let mut receipts: Vec<_> = outputs
        .into_iter()
        .filter_map(|(id, output_index)| {
            let output_index = output_index?;
            let call_index = calls.get(id).copied().flatten()?;
            if id.is_empty() || call_index >= output_index {
                return None;
            }
            let call: Call = serde_json::from_str(request.input.get(call_index)?.get()).ok()?;
            let output: Output =
                serde_json::from_str(request.input.get(output_index)?.get()).ok()?;
            if call.kind != "function_call"
                || call.name != "sessions_spawn"
                || call.call_id != id
                || output.kind != "function_call_output"
                || output.call_id != id
            {
                return None;
            }
            let receipt: OpenClawSpawnReceipt = serde_json::from_str(&output.output).ok()?;
            receipt.accepted().then_some(ReceiptPosition {
                call_index,
                output_index,
                tool_call_id: output.call_id,
                receipt,
            })
        })
        .collect();
    receipts.sort_unstable_by_key(|position| position.output_index);
    receipts
}

pub(crate) fn openclaw_receipts(observation: &ProtocolObservation) -> Vec<ReceiptPosition> {
    if observation.source.profile != ProviderProfile::OpenAiResponses
        || observation.source.side != ProtocolSide::ClientRequest
        || observation.unit != ProtocolUnit::HttpJsonBody
        || !observation.quality.is_empty()
        || !observation
            .native_ids
            .iter()
            .any(|id| id.namespace == NativeIdNamespace::ToolResultReference)
    {
        return Vec::new();
    }
    let [EvidenceSegment::Bytes { offset: 0, data }] = observation.body.segments.as_slice() else {
        return Vec::new();
    };
    if observation.body.original_length != Some(data.len() as u64) {
        return Vec::new();
    }
    serde_json::from_slice(data)
        .map(|request| positions(request, &observation.native_ids))
        .unwrap_or_default()
}
