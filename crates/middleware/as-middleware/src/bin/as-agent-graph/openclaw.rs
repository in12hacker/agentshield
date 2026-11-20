//! Read-only cross-check of originals from one independently verified OpenClaw Gateway instance.
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

#[derive(Deserialize)]
struct Frame {
    original: Option<Event>,
}

#[derive(Deserialize)]
struct Event {
    event: Option<String>,
    payload: Option<Payload>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Payload {
    run_id: Option<String>,
    session_key: Option<String>,
    agent_id: Option<String>,
    stream: Option<String>,
    seq: Option<u64>,
    state: Option<String>,
    data: Option<Tool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Tool {
    name: Option<String>,
    phase: Option<String>,
    tool_call_id: Option<String>,
    is_error: Option<bool>,
    result: Option<Box<RawValue>>,
}

#[derive(Deserialize)]
struct ToolResult {
    details: Option<as_middleware::agent_graph::OpenClawSpawnReceipt>,
}

#[derive(Deserialize)]
struct RegistryRow {
    database: String,
    run_id: String,
    child_session_key: String,
    requester_session_key: String,
    payload_json: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegistryPayload {
    run_id: String,
    child_session_key: String,
    requester_session_key: String,
    requester_agent_id: String,
    execution: Execution,
    delivery: Option<Status>,
}

#[derive(Deserialize)]
struct Status {
    status: String,
}

#[derive(Deserialize)]
struct Execution {
    status: String,
    outcome: Option<Status>,
}

#[derive(Serialize)]
pub(super) struct Handoff {
    parent_run_id: String,
    parent_session_key: String,
    parent_agent_id: String,
    tool_call_id: String,
    child_run_id: String,
    child_session_key: String,
    tool_start_line: Option<usize>,
    tool_result_line: usize,
    registry_row: Option<usize>,
    registry_database: Option<String>,
    child_terminal: Option<String>,
    delivery: Option<String>,
    delivery_line: Option<usize>,
    gap: Option<&'static str>,
}

fn same_parent(a: &Payload, b: &Payload) -> bool {
    a.run_id == b.run_id && a.session_key == b.session_key && a.agent_id == b.agent_id
}

fn unique<T>(mut values: impl Iterator<Item = T>) -> Option<T> {
    let value = values.next()?;
    values.next().is_none().then_some(value)
}

pub(super) fn analyze(gateway: &str, registry: &str) -> serde_json::Result<Vec<Handoff>> {
    let frames = gateway
        .lines()
        .enumerate()
        .map(|(i, line)| serde_json::from_str::<Frame>(line).map(|frame| (i + 1, frame.original)))
        .collect::<Result<Vec<_>, _>>()?;
    let rows = serde_json::from_str::<Vec<RegistryRow>>(registry)?;
    let payloads = rows
        .iter()
        .map(|row| serde_json::from_str::<RegistryPayload>(&row.payload_json))
        .collect::<Result<Vec<_>, _>>()?;
    let events = frames
        .iter()
        .filter_map(|(line, event)| {
            let event = event.as_ref()?;
            Some((*line, event.event.as_deref()?, event.payload.as_ref()?))
        })
        .collect::<Vec<_>>();
    let tools = events
        .iter()
        .filter_map(|&(line, kind, event)| {
            let tool = event.data.as_ref()?;
            (kind == "agent"
                && event.stream.as_deref() == Some("tool")
                && tool.name.as_deref() == Some("sessions_spawn"))
            .then_some((line, event, tool))
        })
        .collect::<Vec<_>>();
    let mut result = Vec::new();
    for &(line, event, tool) in &tools {
        let (Some(run), Some(session), Some(agent), Some(call), Some(seq), Some(raw_receipt)) = (
            &event.run_id,
            &event.session_key,
            &event.agent_id,
            &tool.tool_call_id,
            event.seq,
            tool.result.as_deref(),
        ) else {
            continue;
        };
        if tool.phase.as_deref() != Some("result")
            || tool.is_error != Some(false)
            || [run, session, agent, call]
                .iter()
                .any(|value| value.is_empty())
        {
            continue;
        }
        let Ok(ToolResult {
            details: Some(receipt),
        }) = serde_json::from_str(raw_receipt.get())
        else {
            continue;
        };
        if !receipt.accepted() || receipt.run_id == *run || receipt.child_session_key == *session {
            continue;
        }
        let start = unique(tools.iter().filter(|(_, original, other)| {
            same_parent(event, original)
                && other.tool_call_id.as_ref() == Some(call)
                && other.phase.as_deref() == Some("start")
                && original.seq.is_some_and(|start| start < seq)
        }));
        let same_tool_results = tools
            .iter()
            .filter(|(_, original, other)| {
                same_parent(event, original)
                    && other.tool_call_id.as_ref() == Some(call)
                    && other.phase.as_deref() == Some("result")
            })
            .count();
        let retained = unique(
            rows.iter()
                .zip(&payloads)
                .enumerate()
                .filter(|(_, (row, _))| {
                    row.run_id == receipt.run_id
                        && row.child_session_key == receipt.child_session_key
                }),
        );
        let mut handoff = Handoff {
            parent_run_id: run.clone(),
            parent_session_key: session.clone(),
            parent_agent_id: agent.clone(),
            tool_call_id: call.clone(),
            child_run_id: receipt.run_id,
            child_session_key: receipt.child_session_key,
            tool_start_line: start.map(|(line, _, _)| *line),
            tool_result_line: line,
            registry_row: None,
            registry_database: None,
            child_terminal: None,
            delivery: None,
            delivery_line: None,
            gap: Some("parent_tool_unverified"),
        };
        if start.is_some() && same_tool_results == 1 {
            handoff.gap = Some("child_registry_unverified");
            if let Some((index, (row, original))) = retained
                && original.run_id == handoff.child_run_id
                && original.child_session_key == handoff.child_session_key
                && row.requester_session_key == *session
                && original.requester_session_key == *session
                && original.requester_agent_id == *agent
            {
                handoff.registry_row = Some(index + 1);
                handoff.registry_database = Some(row.database.clone());
                handoff.gap = Some("child_terminal_unverified");
                if original.execution.status == "terminal"
                    && let Some(outcome) = &original.execution.outcome
                    && !outcome.status.is_empty()
                {
                    handoff.child_terminal = Some(outcome.status.clone());
                    handoff.gap = Some("delivery_unverified");
                    let announce = format!(
                        "announce:v1:{}:{}",
                        handoff.child_session_key, handoff.child_run_id
                    );
                    if let Some((line, _, _)) =
                        unique(events.iter().filter(|(_, kind, original)| {
                            *kind == "chat"
                                && original.state.as_deref() == Some("final")
                                && original.run_id.as_ref() == Some(&announce)
                                && original.session_key.as_ref() == Some(session)
                                && original.agent_id.as_ref() == Some(agent)
                        }))
                        && original
                            .delivery
                            .as_ref()
                            .is_some_and(|delivery| delivery.status == "delivered")
                    {
                        handoff.delivery_line = Some(*line);
                        handoff.delivery = Some("delivered".to_owned());
                        handoff.gap = None;
                    }
                }
            }
        }
        result.push(handoff);
    }
    Ok(result)
}
