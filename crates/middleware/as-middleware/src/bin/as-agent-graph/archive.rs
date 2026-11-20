use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, value::RawValue};
use std::collections::BTreeMap;

#[derive(Deserialize)]
pub(super) struct Archive<'a> {
    pub runtime: Runtime,
    #[serde(borrow)]
    pub protocol_runs: Option<BTreeMap<String, Run<'a>>>,
    #[serde(borrow)]
    pub protocol: Option<Run<'a>>,
    #[serde(borrow)]
    pub records: Vec<Record<'a>>,
}

#[derive(Deserialize)]
pub(super) struct Runtime {
    pub artifact_binding: bool,
    pub capture: Capture,
}

#[derive(Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Capture {
    pub boot_id: String,
    pub pid: u32,
    pub start_ticks: String,
}

// Keep an explicit JSON null distinct from an absent property.
fn present_raw<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<&'de RawValue>, D::Error> {
    <&RawValue>::deserialize(deserializer).map(Some)
}

#[derive(Deserialize, Serialize)]
pub(super) struct Run<'a> {
    pub run_id: String,
    pub session_id: Option<String>,
    pub session_key: Option<String>,
    pub request_id: Option<String>,
    pub request_line: Option<usize>,
    pub reply_line: Option<usize>,
    pub terminal_line: Option<usize>,
    pub terminal_state: Option<String>,
    #[serde(
        default,
        borrow,
        deserialize_with = "present_raw",
        skip_serializing_if = "Option::is_none"
    )]
    pub terminal_receipt: Option<&'a RawValue>,
    #[serde(default, skip_serializing)]
    pub lifecycle: Vec<Lifecycle>,
    #[serde(default, borrow, skip_serializing)]
    pub tools: Vec<Tool<'a>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ReceiptIdentity {
    pub run_id: String,
    pub session_id: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct Lifecycle {
    pub ts: Option<u64>,
    pub data: LifecycleData,
}

#[derive(Deserialize)]
pub(super) struct LifecycleData {
    pub phase: String,
}

#[derive(Deserialize, Serialize)]
pub(super) struct Tool<'a> {
    pub id: String,
    pub name: String,
    pub start_line: usize,
    pub start_ts: Option<u64>,
    #[serde(
        default,
        borrow,
        deserialize_with = "present_raw",
        skip_serializing_if = "Option::is_none"
    )]
    pub arguments: Option<&'a RawValue>,
    #[serde(default, borrow)]
    pub results: Vec<&'a RawValue>,
}

#[derive(Deserialize)]
pub(super) struct Record<'a> {
    #[serde(borrow)]
    pub original: &'a RawValue,
    #[serde(borrow)]
    pub binding: Binding<'a>,
}

#[derive(Deserialize)]
pub(super) struct OriginalIdentity {
    pub sequence: u64,
    pub pid: u32,
    pub stage: u8,
    pub fault: i64,
}

#[derive(Deserialize)]
pub(super) struct Binding<'a> {
    pub sequence: u64,
    pub status: String,
    #[serde(default)]
    pub scopes: Vec<Scope>,
    #[serde(default, borrow)]
    pub tool_response_objects: Vec<Pair<'a>>,
}

#[derive(Deserialize)]
pub(super) struct Scope {
    pub entry: ScopeEntry,
    pub fields: ScopeFields,
}

#[derive(Deserialize)]
pub(super) struct ScopeEntry {
    pub value: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ScopeFields {
    pub run_id: Option<ScopeField>,
    pub session_id: Option<ScopeField>,
    pub session_key: Option<ScopeField>,
}

#[derive(Deserialize)]
pub(super) struct ScopeField {
    pub field: Slot,
    pub value: Option<ScopeValue>,
}

#[derive(Deserialize)]
pub(super) struct ScopeValue {
    #[serde(rename = "type")]
    pub kind: String,
    pub value: Option<Value>,
    pub reference: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct Pair<'a> {
    pub tool_call_id: String,
    pub tool_name: String,
    pub response_id: String,
    #[serde(borrow)]
    pub original_object_proof: &'a RawValue,
    pub protocol_matches: Vec<ProtocolMatch>,
}

#[derive(Deserialize)]
pub(super) struct ProtocolMatch {
    pub run_id: String,
    pub session_id: Option<String>,
    pub session_key: Option<String>,
    pub tool_start_line: usize,
}

#[derive(Deserialize)]
pub(super) struct ObjectProof {
    pub status: String,
    pub context_fields: ContextFields,
    pub response_content: Slot,
    pub same_tool_in_response: Vec<ContentReference>,
    pub response: ResponseFields,
    pub tool: ToolFields,
    pub response_timestamp: Option<ResponseTimestamp>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ContextFields {
    pub tool_call: Slot,
    pub assistant_message: Slot,
}

#[derive(Deserialize)]
pub(super) struct Slot {
    pub holder: String,
    pub value_tagged: String,
}

#[derive(Deserialize)]
pub(super) struct ContentReference {
    pub array: String,
    pub value_tagged: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ResponseFields {
    pub response_id: ReadString,
}

#[derive(Deserialize)]
pub(super) struct ToolFields {
    pub id: ReadString,
    pub name: ReadString,
}

#[derive(Deserialize)]
pub(super) struct ReadString {
    pub status: String,
    pub holder: Option<String>,
    pub value: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct ResponseTimestamp {
    pub value: Option<TimestampValue>,
}

#[derive(Deserialize)]
pub(super) struct TimestampValue {
    pub value: Option<Value>,
}
