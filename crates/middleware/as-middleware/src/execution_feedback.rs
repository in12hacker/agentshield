//! Deterministic routing of a real rule denial to its explicitly related original ToolCall.
//!
//! Routing never copies fixed text into a command or infers a relationship. The typed execution
//! entry consumes the original rule denial and a backend-owned port; durable dedup belongs to the business log.
//! Flow owns only the admitted in-flight write, and preparation is never delivery.

use crate::{
    activity::{execution_call, original_completion},
    context::{ContextGap, ContextGapReason, ContextView, FactReference, SourceRecord},
};
use as_main::messages::protocol::{OperationReport, OperationResult};
use as_types::{flow::RuleFeedbackRequest, operation::ExecutionOutcome, process_tree::EventKey};

/// Routing result only; a request has not been accepted, applied or sent by Flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedbackPreparation {
    /// No actual denial with nonempty original rule text was supplied.
    NotApplicable,
    /// Original references ready for the backend Flow feedback port.
    Request(RuleFeedbackRequest),
    /// Required explicit relationship or original evidence was unavailable.
    Gap(ContextGap),
}

/// Result of the one runtime-only feedback attempt; transport acceptance is not delivery.
#[derive(Debug)]
pub enum FeedbackExecution {
    /// No actual denied operation with fixed text.
    NotApplicable,
    /// An explicit source/relationship gap; the Flow port was not called.
    Gap(ContextGap),
    /// Another denied event already claimed feedback for this original tool result.
    AlreadyClaimed,
    /// A durable claim has no confirmed port result, including cancellation or process restart.
    Unconfirmed,
    /// Actual retained Flow fact, including a previously stored terminal result.
    Fact(as_types::flow::RuleFeedbackFact),
    /// Actual Flow-port failure, independently of the original operation result.
    Failed(as_types::main_ports::ModuleError),
}

/// Consume a trusted rule denial through the backend Flow port and persistent dedup.
/// A missing execution bridge never calls Flow; no external decoder can create these source values.
pub(crate) async fn execute_feedback(
    executor: &dyn as_types::flow::FlowFeedbackPort,
    log: &crate::business_log::BusinessLog,
    notice: std::sync::Arc<as_types::main_ports::MainNotice>,
    result: &OperationResult,
    cancelled: tokio_util::sync::CancellationToken,
) -> Result<FeedbackExecution, crate::business_log::StoreError> {
    if !has_fixed_denial(result) {
        return Ok(FeedbackExecution::NotApplicable);
    }
    use crate::business_log::StoreError;
    let event = FactReference::Event(EventKey::from(result.event.as_ref()));
    let event_pin = log.retain_facts(std::slice::from_ref(&event))?;
    let context = match log.activity(event.clone()).await {
        Ok(view) => view,
        Err(StoreError::NotProvided) => {
            return Ok(FeedbackExecution::Gap(ContextGap {
                at: event,
                reason: ContextGapReason::NotProvided,
            }));
        }
        Err(error) => return Err(error),
    };
    let request = match prepare_feedback(result, &context) {
        FeedbackPreparation::NotApplicable => return Ok(FeedbackExecution::NotApplicable),
        FeedbackPreparation::Gap(g) => return Ok(FeedbackExecution::Gap(g)),
        FeedbackPreparation::Request(r) => r,
    };
    let message = context
        .source_records
        .iter()
        .find_map(|r| match r {
            SourceRecord::Message(m) if m.id == request.call.message => Some(m.clone()),
            _ => None,
        })
        .ok_or(crate::business_log::StoreError::NotProvided)?;
    let call_pin = log.retain_facts(&[FactReference::Part(request.call)])?;
    let observations = context
        .source_records
        .iter()
        .filter_map(|r| match r {
            SourceRecord::Protocol(p) if message.record_refs.contains(&p.id) => Some(p.clone()),
            _ => None,
        })
        .collect();
    use crate::database::query::FeedbackClaim;
    let claim = log.claim_feedback(request).await?;
    match claim {
        FeedbackClaim::ConflictingEvidence => {
            return Ok(FeedbackExecution::Gap(ContextGap {
                at: event,
                reason: ContextGapReason::ConflictingEvidence,
            }));
        }
        FeedbackClaim::AlreadyClaimed => return Ok(FeedbackExecution::AlreadyClaimed),
        FeedbackClaim::Unconfirmed => return Ok(FeedbackExecution::Unconfirmed),
        FeedbackClaim::Existing(f) => return Ok(FeedbackExecution::Fact(f)),
        FeedbackClaim::Failed(e) => return Ok(FeedbackExecution::Failed(e)),
        FeedbackClaim::New => {}
    }
    // Keep the same guard through this caller's result commit as well as the
    // actual Flow transaction; either side can finish first.
    let retention_guard = std::sync::Arc::new((event_pin, call_pin));
    let result = executor
        .prepare(as_types::flow::RuleFeedbackPrepareInput {
            notice,
            call: request.call,
            message,
            observations,
            cancelled,
            retention_guard: Some(Box::new(retention_guard.clone())),
        })
        .await
        .map_err(flow_error);
    log.feedback_result(request, result.clone()).await?;
    drop(retention_guard);
    Ok(match result {
        Ok(f) => FeedbackExecution::Fact(f),
        Err(e) => FeedbackExecution::Failed(e),
    })
}

/// Prepare only from a real denied operation and its trusted, explicitly supplied activity view.
///
/// Fixed rule text is checked for presence only and stays with the original completion; whitespace
/// is not trimmed. No bridge is inferred from PID, ancestry, command, time or native tool ID equality.
pub fn prepare_feedback(result: &OperationResult, context: &ContextView) -> FeedbackPreparation {
    if !has_fixed_denial(result) {
        return FeedbackPreparation::NotApplicable;
    }
    match route(result, context) {
        Ok(request) => FeedbackPreparation::Request(request),
        Err(gap) => FeedbackPreparation::Gap(gap),
    }
}

pub(crate) fn has_fixed_denial(result: &OperationResult) -> bool {
    matches!(
        result.result,
        OperationReport::Receipt {
            outcome: ExecutionOutcome::Denied,
            ..
        }
    ) && result
        .hit
        .as_ref()
        .and_then(|hit| hit.deny_message.as_deref())
        .is_some_and(|text| !text.is_empty())
}

fn route(
    result: &OperationResult,
    context: &ContextView,
) -> Result<RuleFeedbackRequest, ContextGap> {
    let denial = EventKey::from(result.event.as_ref());
    let call = execution_call(context, denial)?;
    if original_completion(context, denial)? != result {
        return Err(ContextGap {
            at: FactReference::Completion(denial),
            reason: ContextGapReason::ConflictingEvidence,
        });
    }
    Ok(RuleFeedbackRequest { denial, call })
}

fn flow_error(error: as_types::flow::FlowError) -> as_types::main_ports::ModuleError {
    use as_types::{flow::FlowError, main_ports::ModuleError};
    match error {
        FlowError::InvalidInput => ModuleError::InvalidInput,
        FlowError::Unsupported => ModuleError::Unsupported,
        FlowError::Busy => ModuleError::Busy,
        FlowError::Stopped => ModuleError::Stopped,
        FlowError::Conflict => ModuleError::Conflict,
        _ => ModuleError::Unavailable,
    }
}

#[cfg(test)]
pub(crate) mod tests;
