use super::archive::{
    Archive, Binding, Capture, ObjectProof, OriginalIdentity, ReadString, ReceiptIdentity, Run,
    Scope, ScopeField, ScopeFields, Tool,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, value::RawValue};
use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

#[derive(Serialize)]
pub(super) struct ReportView<'a> {
    pub sources: Vec<SourceView<'a>>,
    pub coverage: Coverage,
}

#[derive(Serialize)]
pub(super) struct SourceView<'a> {
    source: &'a Path,
    capture: &'a Capture,
    runs: Vec<RunView<'a>>,
    callbacks: Vec<CallbackView<'a>>,
    delegation_candidates: Vec<DelegationCandidate<'a>>,
    pub coverage: Coverage,
}

#[derive(Serialize)]
struct RunView<'a> {
    #[serde(flatten)]
    run: &'a Run<'a>,
    window: Option<Window>,
    tools: Vec<ToolView<'a>>,
}

#[derive(Serialize)]
struct ToolView<'a> {
    #[serde(flatten)]
    tool: &'a Tool<'a>,
    callback_sequences: Vec<u64>,
    run_timing: Timing<&'a str>,
    response_timing: Timing<ResponseRef<'a>>,
}

#[derive(Serialize)]
struct CallbackView<'a> {
    original: &'a RawValue,
    source_status: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    relation: Option<Relation<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    gap: Option<Gap>,
}

#[derive(Clone, Copy, Serialize)]
struct Relation<'a> {
    run_id: &'a str,
    tool_call_id: &'a str,
    response_id: &'a str,
    tool_start_line: usize,
}

#[derive(Clone, Copy, Serialize)]
struct ResponseRef<'a> {
    response_id: &'a str,
    callback_sequence: u64,
    created_ms: Option<u64>,
}

#[derive(Serialize)]
struct DelegationCandidate<'a> {
    parent_run_id: &'a str,
    tool_call_id: &'a str,
    result_line: usize,
    child_run_id: Cow<'a, str>,
    child_session_key: Cow<'a, str>,
    observed_child_run: Option<&'a str>,
}

// The native Gateway tool event carries structured details. Text content
// can contain arbitrary JSON and does not carry this receipt's provenance.
#[derive(Deserialize)]
struct SpawnResult<'a> {
    line: usize,
    #[serde(borrow)]
    original: SpawnToolEvent<'a>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpawnToolEvent<'a> {
    name: &'a str,
    tool_call_id: &'a str,
    phase: &'a str,
    is_error: bool,
    #[serde(borrow)]
    result: SpawnToolResult<'a>,
}

#[derive(Deserialize)]
struct SpawnToolResult<'a> {
    #[serde(borrow)]
    details: SpawnReceipt<'a>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpawnReceipt<'a> {
    status: &'a str,
    #[serde(borrow)]
    run_id: Cow<'a, str>,
    #[serde(borrow)]
    child_session_key: Cow<'a, str>,
}

fn delegation_candidates<'a>(runs: &Runs<'a>, tools: &Tools<'a>) -> Vec<DelegationCandidate<'a>> {
    let mut candidates = Vec::new();
    for (&(parent_run_id, tool_call_id), tool) in tools {
        if tool.name != "sessions_spawn" {
            continue;
        }
        for raw in &tool.results {
            let Ok(received) = serde_json::from_str::<SpawnResult<'a>>(raw.get()) else {
                continue;
            };
            let event = received.original;
            let receipt = event.result.details;
            if event.name != tool.name
                || event.tool_call_id != tool_call_id
                || event.phase != "result"
                || event.is_error
                || received.line <= tool.start_line
                || receipt.status != "accepted"
                || receipt.run_id.is_empty()
                || receipt.child_session_key.is_empty()
            {
                continue;
            }
            let observed_child_run = runs
                .get(receipt.run_id.as_ref())
                .filter(|child| {
                    child.run_id != parent_run_id
                        && child.session_key.as_deref() == Some(receipt.child_session_key.as_ref())
                })
                .map(|child| child.run_id.as_str());
            candidates.push(DelegationCandidate {
                parent_run_id,
                tool_call_id,
                result_line: received.line,
                child_run_id: receipt.run_id,
                child_session_key: receipt.child_session_key,
                observed_child_run,
            });
        }
    }
    candidates.sort_by_key(|candidate| candidate.result_line);
    candidates
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum Gap {
    IncompleteCapture,
    MissingProof,
    AmbiguousProof,
    MissingRunIdentity,
    ConflictingEvidence,
}

#[derive(Clone, Copy, Default, Serialize)]
pub(super) struct Coverage {
    runs: usize,
    tool_calls: usize,
    linked_tool_calls: usize,
    callbacks: usize,
    linked_callbacks: usize,
}

impl Coverage {
    pub(super) fn add(&mut self, other: Self) {
        self.runs += other.runs;
        self.tool_calls += other.tool_calls;
        self.linked_tool_calls += other.linked_tool_calls;
        self.callbacks += other.callbacks;
        self.linked_callbacks += other.linked_callbacks;
    }
}

#[derive(Clone, Copy, Serialize)]
struct Window {
    start_ms: u64,
    end_ms: u64,
}

impl Window {
    fn from_run(run: &Run<'_>) -> Option<Self> {
        fn unique_time<'a>(
            mut events: impl Iterator<Item = &'a super::archive::Lifecycle>,
        ) -> Option<u64> {
            let first = events.next()?;
            if events.next().is_some() {
                return None;
            }
            first.ts
        }
        let start_ms = unique_time(run.lifecycle.iter().filter(|e| e.data.phase == "start"))?;
        let end_ms = unique_time(
            run.lifecycle
                .iter()
                .filter(|e| matches!(e.data.phase.as_str(), "end" | "error")),
        )?;
        (start_ms <= end_ms).then_some(Self { start_ms, end_ms })
    }
}

#[derive(Serialize)]
struct Timing<T> {
    possible: Vec<T>,
    excluded: Vec<T>,
    unavailable: Vec<T>,
}

impl<T> Timing<T> {
    fn classify(items: impl Iterator<Item = (T, Option<bool>)>) -> Self {
        let mut timing = Self {
            possible: Vec::new(),
            excluded: Vec::new(),
            unavailable: Vec::new(),
        };
        for (item, comparison) in items {
            match comparison {
                Some(true) => timing.possible.push(item),
                Some(false) => timing.excluded.push(item),
                None => timing.unavailable.push(item),
            }
        }
        timing
    }
}

type Runs<'a> = BTreeMap<&'a str, &'a Run<'a>>;
type Tools<'a> = BTreeMap<(&'a str, &'a str), &'a Tool<'a>>;

pub(super) fn analyze<'a>(
    path: &'a Path,
    archive: &'a Archive<'a>,
) -> Result<SourceView<'a>, String> {
    let capture = &archive.runtime.capture;
    if !archive.runtime.artifact_binding
        || capture.boot_id.is_empty()
        || capture.pid == 0
        || capture.start_ticks.parse::<u64>().is_err()
    {
        return Err("missing verified capture identity".into());
    }

    let ordered_runs: Vec<&Run<'a>> = match (&archive.protocol_runs, &archive.protocol) {
        (Some(runs), None) => runs.values().collect(),
        (None, Some(run)) => vec![run],
        _ => return Err("expected one native protocol run collection".into()),
    };
    let (runs, tools) = index_protocol(&ordered_runs)?;
    let delegation_candidates = delegation_candidates(&runs, &tools);
    let windows: Vec<(&str, Option<Window>)> = ordered_runs
        .iter()
        .map(|run| (run.run_id.as_str(), Window::from_run(run)))
        .collect();

    let mut callbacks = Vec::with_capacity(archive.records.len());
    let mut sequences = BTreeSet::new();
    let mut tool_callbacks: BTreeMap<(&str, &str), Vec<u64>> = BTreeMap::new();
    let mut responses: BTreeMap<&str, Vec<ResponseRef<'a>>> = BTreeMap::new();
    let mut linked_callbacks = 0;
    for record in &archive.records {
        let original: OriginalIdentity =
            serde_json::from_str(record.original.get()).map_err(|e| e.to_string())?;
        if original.sequence == 0 || !sequences.insert(original.sequence) {
            return Err(format!(
                "invalid or duplicate callback sequence {}",
                original.sequence
            ));
        }
        let outcome = if original.pid != capture.pid || original.sequence != record.binding.sequence
        {
            Err(Gap::ConflictingEvidence)
        } else if original.stage != 5 || original.fault != 0 {
            Err(Gap::IncompleteCapture)
        } else {
            confirm(&record.binding, &runs, &tools)
        };
        let (relation, gap) = match outcome {
            Ok((relation, created_ms)) => {
                linked_callbacks += 1;
                tool_callbacks
                    .entry((relation.run_id, relation.tool_call_id))
                    .or_default()
                    .push(original.sequence);
                responses
                    .entry(relation.run_id)
                    .or_default()
                    .push(ResponseRef {
                        response_id: relation.response_id,
                        callback_sequence: original.sequence,
                        created_ms,
                    });
                (Some(relation), None)
            }
            Err(gap) => (None, Some(gap)),
        };
        callbacks.push(CallbackView {
            original: record.original,
            source_status: &record.binding.status,
            relation,
            gap,
        });
    }

    let coverage = Coverage {
        runs: runs.len(),
        tool_calls: tools.len(),
        linked_tool_calls: tool_callbacks.len(),
        callbacks: callbacks.len(),
        linked_callbacks,
    };
    let runs = ordered_runs
        .into_iter()
        .zip(&windows)
        .map(|(run, &(_, window))| {
            let response_observations = responses.get(run.run_id.as_str()).into_iter().flatten();
            let tools = run.tools.iter().map(|tool| {
                let run_timing = Timing::classify(windows.iter().map(|&(id, window)| {
                    (
                        id,
                        window
                            .zip(tool.start_ts)
                            .map(|(w, start)| (w.start_ms..=w.end_ms).contains(&start)),
                    )
                }));
                let response_timing =
                    Timing::classify(response_observations.clone().map(|&response| {
                        (
                            response,
                            response
                                .created_ms
                                .zip(tool.start_ts)
                                .map(|(created, start)| created <= start),
                        )
                    }));
                ToolView {
                    tool,
                    callback_sequences: tool_callbacks
                        .remove(&(run.run_id.as_str(), tool.id.as_str()))
                        .unwrap_or_default(),
                    run_timing,
                    response_timing,
                }
            });
            RunView {
                run,
                window,
                tools: tools.collect(),
            }
        })
        .collect();

    Ok(SourceView {
        source: path,
        capture,
        runs,
        callbacks,
        delegation_candidates,
        coverage,
    })
}

fn index_protocol<'a>(ordered_runs: &[&'a Run<'a>]) -> Result<(Runs<'a>, Tools<'a>), String> {
    let mut runs = BTreeMap::new();
    let mut tools = BTreeMap::new();
    for &run in ordered_runs {
        let id = run.run_id.as_str();
        if id.is_empty() || runs.insert(id, run).is_some() {
            return Err(format!("invalid or duplicate run ID {id:?}"));
        }
        if let Some(raw) = run.terminal_receipt {
            let receipt: Option<ReceiptIdentity> =
                serde_json::from_str(raw.get()).map_err(|e| e.to_string())?;
            if let Some(receipt) = receipt
                && (receipt.run_id != id
                    || receipt
                        .session_id
                        .as_ref()
                        .zip(run.session_id.as_ref())
                        .is_some_and(|(a, b)| a != b))
            {
                return Err(format!("terminal receipt identity differs for run {id:?}"));
            }
        }
        for tool in &run.tools {
            if tool.id.is_empty() || tools.insert((id, tool.id.as_str()), tool).is_some() {
                return Err(format!(
                    "invalid or duplicate tool ID {:?} in run {id:?}",
                    tool.id
                ));
            }
        }
    }
    Ok((runs, tools))
}

fn identity(
    scopes: &[Scope],
    select: impl Fn(&ScopeFields) -> Option<&ScopeField>,
) -> Result<Option<&str>, Gap> {
    let mut identity = None;
    for scope in scopes {
        let Some(field) = select(&scope.fields) else {
            continue;
        };
        let Some(value) = &field.value else {
            continue;
        };
        let Some(value_text) = value
            .value
            .as_ref()
            .and_then(Value::as_str)
            .filter(|_| value.kind == "string")
        else {
            continue;
        };
        if scope.entry.value != field.field.holder
            || value.reference.as_deref() != Some(field.field.value_tagged.as_str())
            || identity.is_some_and(|other| other != value_text)
        {
            return Err(Gap::ConflictingEvidence);
        }
        identity = Some(value_text);
    }
    Ok(identity)
}

fn read_string(field: &ReadString, holder: &str, value: &str) -> bool {
    field.status == "read"
        && field.holder.as_deref() == Some(holder)
        && field.value.as_deref() == Some(value)
}

fn confirm<'a>(
    binding: &'a Binding<'a>,
    runs: &Runs<'a>,
    tools: &Tools<'a>,
) -> Result<(Relation<'a>, Option<u64>), Gap> {
    let [pair] = binding.tool_response_objects.as_slice() else {
        return Err(if binding.tool_response_objects.is_empty() {
            Gap::MissingProof
        } else {
            Gap::AmbiguousProof
        });
    };
    let run_id = identity(&binding.scopes, |s| s.run_id.as_ref())?
        .filter(|id| !id.is_empty())
        .ok_or(Gap::MissingRunIdentity)?;
    let run = runs.get(run_id).ok_or(Gap::MissingRunIdentity)?;
    let tool = tools
        .get(&(run_id, pair.tool_call_id.as_str()))
        .ok_or(Gap::MissingProof)?;
    let mut matches = pair.protocol_matches.iter().filter(|m| m.run_id == run_id);
    let matched = matches.next().ok_or(Gap::MissingProof)?;
    if matches.next().is_some() {
        return Err(Gap::AmbiguousProof);
    }
    if disagree(matched.session_id.as_deref(), run.session_id.as_deref())
        || disagree(matched.session_key.as_deref(), run.session_key.as_deref())
        || matched.tool_start_line != tool.start_line
        || pair.tool_name != tool.name
        || disagree(
            identity(&binding.scopes, |s| s.session_id.as_ref())?,
            run.session_id.as_deref(),
        )
        || disagree(
            identity(&binding.scopes, |s| s.session_key.as_ref())?,
            run.session_key.as_deref(),
        )
    {
        return Err(Gap::ConflictingEvidence);
    }

    let proof: ObjectProof =
        serde_json::from_str(pair.original_object_proof.get()).map_err(|_| Gap::MissingProof)?;
    let call = &proof.context_fields.tool_call;
    let response = &proof.context_fields.assistant_message;
    if proof.status != "linked_objects"
        || call.holder.is_empty()
        || call.holder != response.holder
        || call.value_tagged.is_empty()
        || response.value_tagged.is_empty()
        || proof.response_content.holder != response.value_tagged
        || proof.response_content.value_tagged.is_empty()
        || !proof.same_tool_in_response.iter().any(|item| {
            item.value_tagged == call.value_tagged
                && item.array == proof.response_content.value_tagged
        })
        || !read_string(&proof.tool.id, &call.value_tagged, &tool.id)
        || !read_string(&proof.tool.name, &call.value_tagged, &tool.name)
        || pair.response_id.is_empty()
        || !read_string(
            &proof.response.response_id,
            &response.value_tagged,
            &pair.response_id,
        )
    {
        return Err(Gap::ConflictingEvidence);
    }

    let created_ms = proof
        .response_timestamp
        .and_then(|field| field.value)
        .and_then(|value| value.value)
        .and_then(millis);
    Ok((
        Relation {
            run_id,
            tool_call_id: &tool.id,
            response_id: &pair.response_id,
            tool_start_line: tool.start_line,
        },
        created_ms,
    ))
}

fn disagree(left: Option<&str>, right: Option<&str>) -> bool {
    left.zip(right).is_some_and(|(a, b)| a != b)
}

fn millis(value: Value) -> Option<u64> {
    let number = value.as_f64()?;
    if number.fract() == 0.0 && (0.0..=9_007_199_254_740_991.0).contains(&number) {
        Some(number as u64)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> Value {
        let mut input = json!({
            "runtime": {"artifact_binding": true, "capture": {"boot_id": "boot", "pid": 42, "start_ticks": "9"}},
            "protocol_runs": {
                "first": {"run_id": "first", "session_id": "s1", "lifecycle": [
                    {"ts": 100, "data": {"phase": "start"}}, {"ts": 300, "data": {"phase": "end"}}
                ], "tools": [{"id": "call", "name": "read", "start_line": 5, "start_ts": 200,
                            "arguments": null, "results": [{"line": 6, "original": {"isError": false}}]}]},
                "second": {"run_id": "second", "session_id": "s2", "lifecycle": [
                    {"ts": 150, "data": {"phase": "start"}}, {"ts": 350, "data": {"phase": "end"}}
                ], "tools": [{"id": "call", "name": "read", "start_line": 8, "start_ts": 210,
                            "results": [{"line": 9, "original": {"isError": true}}]}]}
            },
            "records": [{
                "original": {"sequence": 1, "pid": 42, "stage": 5, "fault": 0},
                "binding": {"sequence": 1, "status": "INCOMPLETE", "scopes": [],
                            "tool_response_objects": []}
            }]
        });
        input["records"][0]["binding"]["scopes"] = json!([{
            "entry": {"value": "run-holder"},
            "fields": {"runId": {"field": {"holder": "run-holder", "value_tagged": "run-slot"},
                      "value": {"type": "string", "reference": "run-slot", "value": "first"}}}
        }]);
        input["records"][0]["binding"]["tool_response_objects"] = json!([{
            "tool_call_id": "call", "tool_name": "read", "response_id": "response",
            "protocol_matches": [
                {"run_id": "first", "session_id": "s1", "tool_start_line": 5},
                {"run_id": "second", "session_id": "s2", "tool_start_line": 8}
            ],
            "original_object_proof": {
                "status": "linked_objects",
                "context_fields": {"toolCall": {"holder": "scope", "value_tagged": "tool"},
                                   "assistantMessage": {"holder": "scope", "value_tagged": "message"}},
                "response_content": {"holder": "message", "value_tagged": "array"},
                "same_tool_in_response": [{"array": "array", "value_tagged": "tool"}],
                "response": {"responseId": {"status": "read", "holder": "message", "value": "response"}},
                "tool": {"id": {"status": "read", "holder": "tool", "value": "call"},
                         "name": {"status": "read", "holder": "tool", "value": "read"}},
                "response_timestamp": {"value": {"value": 240.0}}
            }
        }]);
        input
    }

    fn view(input: Value) -> Value {
        let json = input.to_string();
        let archive = serde_json::from_str(&json).expect("decode fixture");
        serde_json::to_value(analyze(Path::new("fixture.json"), &archive).expect("analyze fixture"))
            .expect("serialize view")
    }

    #[test]
    fn original_run_and_object_scope_select_the_call_even_when_time_disagrees() {
        let output = view(fixture());
        let first = &output["runs"][0]["tools"][0];
        let second = &output["runs"][1]["tools"][0];
        assert_eq!(first["callback_sequences"], json!([1]));
        assert_eq!(second["callback_sequences"], json!([]));
        assert_eq!(first["run_timing"]["possible"], json!(["first", "second"]));
        assert_eq!(
            first["response_timing"]["excluded"][0]["callback_sequence"],
            1
        );
        assert_eq!(output["callbacks"][0]["relation"]["run_id"], "first");
        assert_eq!(output["coverage"]["tool_calls"], 2);
        assert_eq!(output["coverage"]["linked_tool_calls"], 1);
        assert_eq!(first.get("arguments"), Some(&Value::Null));
        assert!(second.get("arguments").is_none());
    }

    #[test]
    fn a_single_timing_candidate_does_not_replace_a_conflicting_object_reference() {
        let mut input = fixture();
        input["protocol_runs"]["second"]["lifecycle"][0]["ts"] = json!(230);
        input["records"][0]["binding"]["tool_response_objects"][0]["original_object_proof"]["same_tool_in_response"]
            [0]["value_tagged"] = json!("different-object");
        let output = view(input);
        let first = &output["runs"][0]["tools"][0];
        assert_eq!(first["run_timing"]["possible"], json!(["first"]));
        assert_eq!(first["callback_sequences"], json!([]));
        assert!(output["callbacks"][0].get("relation").is_none());
        assert_eq!(output["callbacks"][0]["gap"], "conflicting_evidence");
        assert_eq!(output["coverage"]["linked_tool_calls"], 0);
    }

    #[test]
    fn delegation_receipt_stays_a_candidate_and_requires_the_original_tool_event() {
        let mut input = fixture();
        input["protocol_runs"]["second"]["session_key"] = json!("child-session");
        input["protocol_runs"]["first"]["tools"][0] = json!({
            "id": "spawn", "name": "sessions_spawn", "start_line": 10, "results": [
                {"line": 11, "original": {"name": "sessions_spawn", "toolCallId": "spawn", "phase": "result", "isError": false,
                    "result": {"details": {"status": "accepted", "runId": "second", "childSessionKey": "child-session"}}}},
                {"line": 12, "original": {"name": "sessions_spawn", "toolCallId": "different", "phase": "result", "isError": false,
                    "result": {"details": {"status": "accepted", "runId": "second", "childSessionKey": "child-session"}}}},
                {"line": 13, "original": {"name": "sessions_spawn", "toolCallId": "spawn", "phase": "result", "isError": false,
                    "result": {"content": [{"type": "text", "text": r#"{"status":"accepted","runId":"second","childSessionKey":"child-session"}"#}]}}}
            ]
        });
        assert_eq!(
            view(input.clone())["delegation_candidates"],
            json!([{
                "parent_run_id": "first", "tool_call_id": "spawn", "result_line": 11,
                "child_run_id": "second", "child_session_key": "child-session", "observed_child_run": "second"
            }])
        );
        input["protocol_runs"]["second"]["session_key"] = json!("another-session");
        let output = view(input);
        assert_eq!(
            output["delegation_candidates"][0]["observed_child_run"],
            Value::Null
        );
        assert_eq!(output["coverage"]["linked_callbacks"], 0);
    }
}
