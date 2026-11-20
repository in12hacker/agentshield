//! Single-use adapter for the controlled native/TLS smoke coordinator, not a production intake.
//!
//! The coordinator owns a live fixture application which parses the actual provider ToolCall and
//! executes its file open explicitly. Its declaration is required; native facts alone do not create
//! this bridge. No stock application/OpenClaw execution association is supplied by this example.
use as_middleware::codec::decode_operation_result;
use as_middleware::{
    context::{ContextView, EvidenceRelation, FactReference, RelationKind, SourceRecord},
    execution_feedback::{FeedbackPreparation, prepare_feedback},
};
use as_types::{
    flow::{
        ChatMessage, ChatPart, ChatPartRef, NativeIdNamespace, NativeIdValue, ProtocolObservation,
    },
    process_tree::{EventKey, ProcessKey},
};
use serde::Deserialize;
use serde_json::{json, value::RawValue};
use std::io::Read;
use std::sync::Arc;

const MAX_INPUT: u64 = 2 * 1024 * 1024;

/// Explicit statement from the trusted test coordinator's actual owned execution workflow.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ControlledExecution {
    operation: EventKey,
    call: ChatPartRef,
    executor: ProcessKey,
    native_call_id: NativeIdValue,
    response_id: NativeIdValue,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    completion: Box<RawValue>,
    observation: ProtocolObservation,
    message: ChatMessage,
    controlled_execution: ControlledExecution,
}

fn run() -> Result<serde_json::Value, &'static str> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(MAX_INPUT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "input_failed")?;
    if bytes.len() as u64 > MAX_INPUT {
        return Err("input_too_large");
    }
    let input: Input = serde_json::from_slice(&bytes).map_err(|_| "invalid_current_input")?;
    let completion = decode_operation_result(input.completion.get().as_bytes())
        .map_err(|_| "invalid_completion")?;
    let declaration = input.controlled_execution;
    let event = EventKey::from(completion.event.as_ref());
    let actor = completion.event.header();
    if declaration.operation != event
        || declaration.call.message != input.message.id
        || input.message.source.actor.process != Some(declaration.executor)
        || actor.actor_tgid != Some(declaration.executor.host_tgid)
        || actor.actor_birth_boottime_ns != Some(declaration.executor.birth_boottime_ns)
    {
        return Err("controlled_execution_source_mismatch");
    }
    if !input.message.observed_parts.iter().any(|part| {
        matches!(part,
        ChatPart::ToolCall { id, native_id:Some(native), .. }
        if *id==declaration.call.part_id && native.namespace==NativeIdNamespace::ToolCall
        && native.value==declaration.native_call_id)
    }) || !input.observation.native_ids.iter().any(|native| {
        native.namespace == NativeIdNamespace::Response && native.value == declaration.response_id
    }) {
        return Err("controlled_execution_call_mismatch");
    }
    let records = [
        SourceRecord::OperationResult(Arc::new(completion.clone())),
        SourceRecord::Protocol(Arc::new(input.observation.clone())),
        SourceRecord::Message(Arc::new(input.message.clone())),
    ];
    let evidence = [
        FactReference::Completion(event),
        FactReference::Protocol(input.observation.id),
        FactReference::Part(declaration.call),
    ];
    let relations = [EvidenceRelation {
        kind: RelationKind::ToolExecution,
        from: FactReference::Part(declaration.call),
        to: FactReference::Event(event),
        evidence: evidence.to_vec(),
    }];
    let context = ContextView {
        anchor: FactReference::Completion(event),
        source_records: records.to_vec(),
        relations: relations.to_vec(),
        gaps: vec![],
    };
    Ok(match prepare_feedback(&completion, &context) {
        FeedbackPreparation::NotApplicable => json!({"status":"not_applicable"}),
        FeedbackPreparation::Request(request) => json!({"status":"request","request":request}),
        FeedbackPreparation::Gap(gap) => json!({"status":"gap","gap":gap}),
    })
}

fn main() {
    match run() {
        Ok(value) => println!("{value}"),
        Err(code) => {
            println!("{}", json!({"status":"failed","code":code}));
            std::process::exit(1);
        }
    }
}
