//! Local Group analysis of an observed A2A request and its actual trace ancestry.
mod collect;
pub(crate) mod gateway;
mod http;
mod openclaw;
mod otlp;
pub(crate) mod websocket;
pub(crate) use collect::{Collector, Intake, RecordedExchange};
pub use openclaw::OpenClawSpawnReceipt;
pub(crate) use openclaw::openclaw_receipts;

use crate::{
    context::FactReference,
    database::{
        StoreError,
        query::{protocol_references, same_tool_scope, traced_response_references},
    },
};
use as_types::{
    flow::{HttpsOrigin, NativeIdNamespace, ProtocolSide, traceparent_ids},
    process_tree::ProcessKey,
};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

#[derive(Deserialize)]
struct Capture {
    parent: ProcessKey,
    child: ProcessKey,
    parent_model_origin: String,
    spans: Vec<Span>,
    requests: Vec<Exchange>,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct Span {
    pub(crate) process: ProcessKey,
    pub(crate) trace_id: String,
    pub(crate) span_id: String,
    parent_span_id: Option<String>,
    kind: u8,
    attributes: HashMap<String, Value>,
}

impl Span {
    fn attribute(&self, key: &str) -> Option<&str> {
        self.attributes
            .get(key)?
            .as_str()
            .filter(|value| !value.is_empty())
    }

    fn number(&self, key: &str) -> Option<u16> {
        match self.attributes.get(key)? {
            Value::Number(value) => u16::try_from(value.as_u64()?).ok(),
            Value::String(value) => value.parse().ok(),
            _ => None,
        }
    }
}

#[derive(Deserialize, Serialize)]
struct Exchange {
    url: String,
    server_address: String,
    server_port: u16,
    client_port: u16,
    path: String,
    status: u16,
    traceparent: Option<String>,
    request: RpcRequest,
    reply: RpcReply,
}

#[derive(Deserialize, Serialize)]
struct RpcRequest {
    jsonrpc: String,
    id: Value,
    method: String,
    params: RequestParams,
}

#[derive(Deserialize, Serialize)]
struct RequestParams {
    message: Message,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Message {
    message_id: String,
    context_id: Option<String>,
}

#[derive(Deserialize, Serialize)]
struct RpcReply {
    jsonrpc: String,
    id: Value,
    result: Option<TaskResult>,
    error: Option<Value>,
}

#[derive(Deserialize, Serialize)]
#[serde(untagged)]
enum TaskResult {
    Wrapped { task: Task },
    Task(Task),
    Other(Value),
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Task {
    kind: Option<String>,
    id: String,
    context_id: String,
    status: TaskStatus,
    #[serde(default)]
    history: Vec<Message>,
}

#[derive(Deserialize, Serialize)]
struct TaskStatus {
    state: String,
}

impl Exchange {
    fn task(&self) -> Option<&Task> {
        if self.status != 200
            || self.request.jsonrpc != "2.0"
            || self.reply.jsonrpc != "2.0"
            || !(self.request.id.is_string()
                || self.request.id.is_i64()
                || self.request.id.is_u64())
            || self.request.id != self.reply.id
            || self.reply.error.is_some()
            || !matches!(self.request.method.as_str(), "message/send" | "SendMessage")
        {
            return None;
        }
        let (task, valid_state) = match (self.request.method.as_str(), self.reply.result.as_ref()?)
        {
            ("message/send", TaskResult::Task(task)) if task.kind.as_deref() == Some("task") => (
                task,
                matches!(
                    task.status.state.as_str(),
                    "submitted"
                        | "working"
                        | "input-required"
                        | "auth-required"
                        | "completed"
                        | "failed"
                        | "canceled"
                        | "rejected"
                ),
            ),
            ("SendMessage", TaskResult::Wrapped { task }) => (
                task,
                matches!(
                    task.status.state.as_str(),
                    "TASK_STATE_SUBMITTED"
                        | "TASK_STATE_WORKING"
                        | "TASK_STATE_INPUT_REQUIRED"
                        | "TASK_STATE_AUTH_REQUIRED"
                        | "TASK_STATE_COMPLETED"
                        | "TASK_STATE_FAILED"
                        | "TASK_STATE_CANCELED"
                        | "TASK_STATE_REJECTED"
                ),
            ),
            _ => return None,
        };
        let message = &self.request.params.message;
        (!task.id.is_empty()
            && !task.context_id.is_empty()
            && !message.message_id.is_empty()
            && message
                .context_id
                .as_ref()
                .is_none_or(|id| id == &task.context_id)
            && valid_state
            && task
                .history
                .iter()
                .any(|item| item.message_id == message.message_id))
        .then_some(task)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Delegation {
    request_id: Value,
    message_id: String,
    task_id: Option<String>,
    context_id: Option<String>,
    terminal_state: Option<String>,
    parent_tool_id: Option<String>,
    parent_tool_span: Option<String>,
    client_span: Option<String>,
    server_span: Option<String>,
    trace_id: Option<String>,
    tool_call: Vec<FactReference>,
    tool_result: Vec<FactReference>,
    child_responses: Vec<FactReference>,
    gap: Option<Gap>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Gap {
    #[serde(rename = "original_task_unavailable")]
    OriginalTask,
    #[serde(rename = "task_terminal_unavailable")]
    TaskTerminal,
    #[serde(rename = "trace_handoff_unavailable")]
    TraceHandoff,
    #[serde(rename = "parent_tool_facts_unavailable")]
    ParentToolFacts,
    #[serde(rename = "child_model_exchange_unavailable")]
    ChildModelExchange,
}

type Index<'a> = HashMap<(ProcessKey, &'a str, &'a str), &'a Span>;

fn hex_id(id: &str, length: usize) -> bool {
    id.len() == length
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && id.bytes().any(|b| b != b'0')
}

fn ancestors<'a>(index: &Index<'a>, span: &'a Span) -> impl Iterator<Item = &'a Span> {
    std::iter::successors(
        index
            .get(&(
                span.process,
                &span.trace_id,
                span.parent_span_id.as_deref().unwrap_or(""),
            ))
            .copied(),
        |parent| {
            index
                .get(&(
                    parent.process,
                    &parent.trace_id,
                    parent.parent_span_id.as_deref().unwrap_or(""),
                ))
                .copied()
        },
    )
    .take(index.len())
}

fn unique<T>(mut items: impl Iterator<Item = T>) -> Option<T> {
    let item = items.next()?;
    items.next().is_none().then_some(item)
}

fn acyclic(index: &Index<'_>) -> bool {
    let mut checked = HashSet::new();
    for &key in index.keys() {
        if checked.contains(&key) {
            continue;
        }
        let mut chain = HashSet::new();
        let mut current = key;
        while let Some(span) = index.get(&current) {
            if checked.contains(&current) {
                break;
            }
            if !chain.insert(current) {
                return false;
            }
            let Some(parent) = span.parent_span_id.as_deref() else {
                break;
            };
            current.2 = parent;
        }
        checked.extend(chain);
    }
    true
}

fn analyze_exchange<'a>(
    parent: ProcessKey,
    child: ProcessKey,
    parent_model_origin: Option<&str>,
    spans: &'a [Span],
    index: &Index<'a>,
    conn: &Connection,
    exchange: &'a Exchange,
) -> Result<Delegation, StoreError> {
    let mut out = Delegation {
        request_id: exchange.request.id.clone(),
        message_id: exchange.request.params.message.message_id.clone(),
        task_id: None,
        context_id: None,
        terminal_state: None,
        parent_tool_id: None,
        parent_tool_span: None,
        client_span: None,
        server_span: None,
        trace_id: None,
        tool_call: Vec::new(),
        tool_result: Vec::new(),
        child_responses: Vec::new(),
        gap: Some(Gap::OriginalTask),
    };
    let Some(task) = exchange.task() else {
        return Ok(out);
    };
    out.task_id = Some(task.id.to_owned());
    out.context_id = Some(task.context_id.to_owned());
    if !matches!(
        task.status.state.as_str(),
        "completed"
            | "failed"
            | "canceled"
            | "rejected"
            | "TASK_STATE_COMPLETED"
            | "TASK_STATE_FAILED"
            | "TASK_STATE_CANCELED"
            | "TASK_STATE_REJECTED"
    ) {
        out.gap = Some(Gap::TaskTerminal);
        return Ok(out);
    }
    out.terminal_state = Some(task.status.state.to_owned());
    out.gap = Some(Gap::TraceHandoff);
    let Ok(target) = url::Url::parse(&exchange.url) else {
        return Ok(out);
    };
    if !matches!(target.scheme(), "http" | "https")
        || target.host_str() != Some(exchange.server_address.as_str())
        || target.port_or_known_default() != Some(exchange.server_port)
        || target.path() != exchange.path
        || !target.username().is_empty()
        || target.password().is_some()
        || target.fragment().is_some()
    {
        return Ok(out);
    }
    let Some((trace, sender)) = exchange.traceparent.as_deref().and_then(traceparent_ids) else {
        return Ok(out);
    };
    let Some(client) = index.get(&(parent, trace, sender)).filter(|span| {
        span.kind == 3
            && span.attribute("http.request.method") == Some("POST")
            && span.attribute("url.full") == Some(exchange.url.as_str())
            && span.attribute("server.address") == Some(exchange.server_address.as_str())
            && span.number("server.port") == Some(exchange.server_port)
            && span.number("http.response.status_code") == Some(exchange.status)
    }) else {
        return Ok(out);
    };
    let Some(server) = unique(spans.iter().filter(|span| {
        span.process == child
            && span.trace_id == trace
            && span.kind == 2
            && span.parent_span_id.as_deref() == Some(sender)
            && span.attribute("http.request.method") == Some("POST")
            && span.attribute("url.path") == Some(exchange.path.as_str())
            && span.attribute("server.address") == Some(exchange.server_address.as_str())
            && span.number("server.port") == Some(exchange.server_port)
            && span.number("network.peer.port") == Some(exchange.client_port)
            && span.number("http.response.status_code") == Some(exchange.status)
    })) else {
        return Ok(out);
    };
    let Some(tool) = unique(
        ancestors(index, client)
            .filter(|span| span.attribute("gen_ai.operation.name") == Some("execute_tool")),
    ) else {
        return Ok(out);
    };
    let Some(call) = tool.attribute("gen_ai.tool.call.id") else {
        return Ok(out);
    };
    let Some(invocation) = unique(spans.iter().filter(|span| {
        span.process == child
            && span.trace_id == trace
            && span.attribute("gen_ai.operation.name") == Some("invoke_agent")
            && span.attribute("gen_ai.conversation.id") == Some(task.context_id.as_str())
            && span
                .attribute("gen_ai.task.id")
                .is_none_or(|id| id == task.id)
            && ancestors(index, span).any(|ancestor| ancestor.span_id == server.span_id)
    })) else {
        return Ok(out);
    };

    let calls = protocol_references(
        conn,
        parent,
        ProtocolSide::UpstreamResponse,
        NativeIdNamespace::ToolCall,
        parent_model_origin,
        call,
    )?;
    let results = protocol_references(
        conn,
        parent,
        ProtocolSide::ClientRequest,
        NativeIdNamespace::ToolResultReference,
        parent_model_origin,
        call,
    )?;
    out.gap = Some(Gap::ParentToolFacts);
    let Some(first_call) = calls.first() else {
        return Ok(out);
    };
    if calls
        .iter()
        .any(|record| record.exchange != first_call.exchange)
        || results.is_empty()
    {
        return Ok(out);
    }
    let results = results
        .into_iter()
        .filter_map(|result| match same_tool_scope(conn, *first_call, result) {
            Ok(true) => Some(Ok(result)),
            Ok(false) => None,
            Err(error) => Some(Err(error)),
        })
        .collect::<Result<Vec<_>, _>>()?;
    if results.is_empty() {
        return Ok(out);
    }
    out.parent_tool_id = Some(call.to_owned());
    out.parent_tool_span = Some(tool.span_id.to_owned());
    out.client_span = Some(sender.to_owned());
    out.server_span = Some(server.span_id.to_owned());
    out.trace_id = Some(trace.to_owned());
    out.tool_call = calls.into_iter().map(FactReference::Protocol).collect();
    out.tool_result = results.into_iter().map(FactReference::Protocol).collect();

    out.gap = Some(Gap::ChildModelExchange);
    let mut model_records = Vec::new();
    let mut missing_model = false;
    for model in spans.iter().filter(|span| {
        span.process == child
            && span.trace_id == trace
            && matches!(
                span.attribute("gen_ai.operation.name"),
                Some("chat" | "generate_content")
            )
            && span.attribute("gen_ai.conversation.id") == Some(task.context_id.as_str())
            && ancestors(index, span).any(|ancestor| ancestor.span_id == invocation.span_id)
    }) {
        let Some(response) = model.attribute("gen_ai.response.id") else {
            missing_model = true;
            continue;
        };
        let mut records = Vec::new();
        for client in spans.iter().filter(|span| {
            span.process == child
                && span.trace_id == trace
                && span.kind == 3
                && span.attribute("http.request.method") == Some("POST")
                && ancestors(index, span).any(|ancestor| ancestor.span_id == model.span_id)
        }) {
            let Some(url) = client
                .attribute("url.full")
                .and_then(|value| url::Url::parse(value).ok())
            else {
                continue;
            };
            let Ok(origin) = HttpsOrigin::parse(&url.origin().ascii_serialization()) else {
                continue;
            };
            records.extend(traced_response_references(
                conn,
                child,
                &origin.url(),
                trace,
                &client.span_id,
                response,
            )?);
        }
        let Some(first) = records.first() else {
            missing_model = true;
            continue;
        };
        if records
            .iter()
            .any(|record| record.exchange != first.exchange)
            || model_records.iter().any(|(key, _)| *key == first.exchange)
        {
            missing_model = true;
            continue;
        }
        model_records.push((first.exchange, records));
    }
    if !model_records.is_empty() {
        out.child_responses = model_records
            .into_iter()
            .flat_map(|(_, records)| records)
            .map(FactReference::Protocol)
            .collect();
        if !missing_model {
            out.gap = None;
        }
    }
    Ok(out)
}

#[derive(Debug, Serialize)]
pub struct Analysis {
    pub(crate) parent: ProcessKey,
    pub(crate) child: ProcessKey,
    pub(crate) delegations: Vec<Delegation>,
}

/// Verify a local archive against the existing read-only business database.
pub fn analyze_archive(input: &str, database: &Path) -> Result<Analysis, StoreError> {
    let mut database = Connection::open_with_flags(
        database,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    database.pragma_update(None, "trusted_schema", false)?;
    let tx = database.transaction()?;
    analyze(input, &tx)
}

pub(crate) fn analyze(input: &str, conn: &Connection) -> Result<Analysis, StoreError> {
    let capture: Capture = serde_json::from_str(input)?;
    let origin =
        HttpsOrigin::parse(&capture.parent_model_origin).map_err(|_| StoreError::Invalid)?;
    if origin.url() != capture.parent_model_origin {
        return Err(StoreError::Invalid);
    }
    if capture.parent == capture.child || capture.parent.boot_id != capture.child.boot_id {
        return Err(StoreError::Invalid);
    }
    let mut senders = HashSet::new();
    let mut requests = HashSet::new();
    for exchange in &capture.requests {
        if !requests.insert((
            exchange.url.as_str(),
            exchange.client_port,
            &exchange.request.id,
        )) {
            return Err(StoreError::Conflict);
        }
        if let Some(sender) = exchange.traceparent.as_deref().and_then(traceparent_ids)
            && !senders.insert(sender)
        {
            return Err(StoreError::Conflict);
        }
    }
    let mut index = HashMap::new();
    for span in &capture.spans {
        if ![capture.parent, capture.child].contains(&span.process)
            || !hex_id(&span.trace_id, 32)
            || !hex_id(&span.span_id, 16)
            || span
                .parent_span_id
                .as_ref()
                .is_some_and(|id| !hex_id(id, 16))
            || index
                .insert(
                    (span.process, span.trace_id.as_str(), span.span_id.as_str()),
                    span,
                )
                .is_some()
        {
            return Err(StoreError::Invalid);
        }
    }
    if !acyclic(&index) {
        return Err(StoreError::Invalid);
    }
    let delegations = capture
        .requests
        .iter()
        .map(|exchange| {
            analyze_exchange(
                capture.parent,
                capture.child,
                Some(&capture.parent_model_origin),
                &capture.spans,
                &index,
                conn,
                exchange,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Analysis {
        parent: capture.parent,
        child: capture.child,
        delegations,
    })
}

pub(crate) fn recorded_trace(record: &RecordedExchange) -> Option<&str> {
    record
        .exchange
        .traceparent
        .as_deref()
        .and_then(traceparent_ids)
        .map(|(trace, _)| trace)
}

pub(crate) fn resolve_record(
    record: &RecordedExchange,
    spans: &[Span],
    conn: &Connection,
) -> Result<Delegation, StoreError> {
    let mut index = Index::new();
    for span in spans {
        if index
            .insert((span.process, &span.trace_id, &span.span_id), span)
            .is_some()
        {
            return analyze_exchange(
                record.parent,
                record.child,
                None,
                &[],
                &Index::new(),
                conn,
                &record.exchange,
            );
        }
    }
    if !acyclic(&index) {
        index.clear();
    }
    analyze_exchange(
        record.parent,
        record.child,
        None,
        spans,
        &index,
        conn,
        &record.exchange,
    )
}

pub(crate) fn recorded_handoff(record: &RecordedExchange) -> Option<String> {
    let (trace, client) = traceparent_ids(record.exchange.traceparent.as_deref()?)?;
    Some(format!("{trace}-{client}"))
}
