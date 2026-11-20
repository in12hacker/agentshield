//! Typed read-only business queries. The host supplies original received facts, never client edges.
use crate::{
    attribution::{AttributionPattern, AttributionRequest, ToolAttribution},
    audit::{AskAuditContext, explain_ask},
    business_log::{BusinessLog, GroupPage, StoreError, sessions::*},
    context::*,
};
use as_main::messages::{
    CommandReply, ContentAccess, MainMessages,
    connections::Connection,
    protocol::{CommandId, CommandResult, ControlCommand, Down, FailureCode, FlowCommand, Up},
};
use as_types::{
    DecisionId,
    configuration::Access,
    flow::{ChatMessageId, ExchangeKey},
    main_ports::ModuleError,
    process_tree::EventKey,
};
use futures::{FutureExt, future::BoxFuture};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::oneshot;

/// Client selectors only; the external schema cannot carry source facts or execution relationships.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum BusinessQuery {
    /// Current driver and Flow status as two original command results.
    ProtectionOverview,
    /// Source-backed model field groups and actual public Gateway Sessions.
    ChatSessions {
        /// Fixed source-specific directory anchor from the preceding page, or first page when None.
        before: Option<ChatSessionRef>,
        /// Requested item count, 1..=64.
        limit: u16,
    },
    /// A page of original exchanges with retained chat messages.
    ChatExchanges {
        /// Original directory anchor, or all retained model exchanges when None.
        session: Option<ChatSessionRef>,
        after: Option<ExchangeKey>,
        limit: u16,
    },
    /// A source-specific page of original committed model messages or native Gateway observations.
    ChatMessages {
        /// Actual model exchange or proven native Session/unassociated-observation scope.
        scope: ChatMessageScope,
        /// Same-source continuation; native cursors retain the first page's committed read cutoff.
        after: Option<ChatMessageCursor>,
        /// Requested logical-message/observation item count, 1..=64; revisions stay together.
        limit: u16,
    },
    /// Retained A2A delegations, ordered by local commit row.
    GroupDelegations {
        #[serde(default, with = "as_types::decimal::optional")]
        before: Option<u64>,
        limit: u16,
    },
    /// Inspect an original reference, optionally comparing candidates for an event anchor.
    ActivityContext {
        anchor: FactReference,
        attribution: Option<AttributionRequest>,
    },
    /// Read a retained original directly, without expanding its related activity graph.
    OriginalRecord {
        reference: FactReference,
        #[serde(with = "as_types::decimal")]
        body_offset: u64,
        body_limit: u32,
    },
    /// Attach context to an original question already received by this middleware.
    AskContext {
        decision_id: DecisionId,
        attribution: Option<AttributionRequest>,
    },
    /// Compare an original event against retained tool-call evidence.
    ToolAttribution {
        event: EventKey,
        pattern: AttributionPattern,
        after: Option<ChatMessageId>,
        limit: u16,
    },
}
/// Business wrappers reference the same source structures, not a second fact model.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BusinessData {
    ProtectionOverview {
        driver: Box<CommandResult>,
        flow: Box<CommandResult>,
    },
    ChatSessions {
        /// Source-specific directory items with original anchors and source fields.
        sessions: Vec<ChatSession>,
        /// Last returned anchor only if another directory page actually exists.
        next_before: Option<ChatSessionRef>,
    },
    ChatExchanges {
        exchanges: Vec<ExchangeKey>,
        next_after: Option<ExchangeKey>,
    },
    ChatMessages {
        /// Selected original model messages/native observations with no source conversion.
        messages: Vec<ChatMessageEntry>,
        /// Last returned item and, for native pages, the same fixed committed cutoff.
        next_after: Option<ChatMessageCursor>,
    },
    GroupDelegations {
        #[serde(flatten)]
        page: GroupPage,
    },
    ActivityContext {
        view: ContextView,
        attribution: Option<Box<ToolAttribution>>,
    },
    AskContext {
        context: Box<AskAuditContext>,
    },
    ToolAttribution {
        view: Box<ToolAttribution>,
    },
}
#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum BusinessResult {
    Completed {
        data: BusinessData,
    },
    Failed {
        code: FailureCode,
        message: &'static str,
    },
}
fn denied(error: ModuleError) -> BusinessResult {
    BusinessResult::Failed {
        code: FailureCode::from(error),
        message: "business query unavailable",
    }
}

/// Admit work through the real connection, then retain only the resulting permissions or reply receivers.
pub(crate) fn start_query(
    messages: &Arc<MainMessages>,
    connection: &Connection,
    log: &BusinessLog,
    query: BusinessQuery,
) -> BoxFuture<'static, BusinessResult> {
    if matches!(query, BusinessQuery::ProtectionOverview) {
        if let Err(error) = connection.authorize(Access::ReadOnly) {
            return futures::future::ready(denied(error)).boxed();
        }
        let driver = command_id().and_then(|id| {
            messages.submit(
                connection,
                Down::Control {
                    id,
                    command: ControlCommand::GetStatus,
                },
            )
        });
        let flow = command_id().and_then(|id| {
            messages.submit(
                connection,
                Down::Flow {
                    id,
                    command: FlowCommand::Status,
                },
            )
        });
        return async move {
            let (driver, flow) = tokio::join!(status_result(driver), status_result(flow));
            BusinessResult::Completed {
                data: BusinessData::ProtectionOverview {
                    driver: Box::new(driver),
                    flow: Box::new(flow),
                },
            }
        }
        .boxed();
    }
    let reader = match messages.content_access(connection) {
        Ok(access) => access,
        Err(error) => return futures::future::ready(denied(error)).boxed(),
    };
    let log = log.clone();
    async move { query_content(&reader, &log, query).await }.boxed()
}

fn command_id() -> Result<CommandId, ModuleError> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).map_err(|_| ModuleError::Unavailable)?;
    Ok(CommandId(bytes))
}

async fn status_result(
    submitted: Result<Option<oneshot::Receiver<CommandReply>>, ModuleError>,
) -> CommandResult {
    let error = match submitted {
        Ok(Some(receiver)) => {
            if let Ok(CommandReply {
                message: Up::Reply { result, .. },
                ..
            }) = receiver.await
            {
                return result;
            }
            ModuleError::Unavailable
        }
        Ok(None) => ModuleError::Unavailable,
        Err(error) => error,
    };
    CommandResult::Failed {
        code: FailureCode::from(error),
        message: error.to_string(),
        facts: None,
    }
}

/// Read through an actual backend content grant; recheck it before returning sensitive data.
async fn query_content(
    reader: &ContentAccess,
    log: &BusinessLog,
    query: BusinessQuery,
) -> BusinessResult {
    if let Err(e) = reader.authorize() {
        return denied(e);
    }
    let data = match query {
        BusinessQuery::ProtectionOverview => return denied(ModuleError::InvalidInput),
        BusinessQuery::ChatSessions { before, limit } => {
            match log.chat_sessions(before, limit).await {
                Ok((sessions, next_before)) => BusinessData::ChatSessions {
                    sessions,
                    next_before,
                },
                Err(error) => return store_failure(error),
            }
        }
        BusinessQuery::ChatExchanges {
            session,
            after,
            limit,
        } => match log.chat_exchanges(session, after, limit).await {
            Ok((exchanges, next_after)) => BusinessData::ChatExchanges {
                exchanges,
                next_after,
            },
            Err(error) => return store_failure(error),
        },
        BusinessQuery::ChatMessages {
            scope,
            after,
            limit,
        } => match log.chat_messages(scope, after, limit).await {
            Ok((messages, next_after)) => BusinessData::ChatMessages {
                messages,
                next_after,
            },
            Err(error) => return store_failure(error),
        },
        BusinessQuery::GroupDelegations { before, limit } => {
            let page = log.group_delegations(before, limit).await;
            if let Err(error) = reader.authorize() {
                return denied(error);
            }
            match page {
                Ok(page) => BusinessData::GroupDelegations { page },
                Err(error) => return store_failure(error),
            }
        }
        BusinessQuery::ActivityContext {
            anchor,
            attribution,
        } => {
            let (mut view, attribution) = match log.activity_context(anchor, attribution).await {
                Ok(value) => value,
                Err(error) => return store_failure(error),
            };
            view.defer_originals();
            BusinessData::ActivityContext {
                view,
                attribution: attribution.map(Box::new),
            }
        }
        BusinessQuery::OriginalRecord {
            reference,
            body_offset,
            body_limit,
        } => {
            match log
                .original_record(reference, body_offset, body_limit)
                .await
            {
                Ok(view) => BusinessData::ActivityContext {
                    view,
                    attribution: None,
                },
                Err(error) => return store_failure(error),
            }
        }
        BusinessQuery::AskContext {
            decision_id,
            attribution,
        } => match explain_ask(reader, log, decision_id, attribution).await {
            Ok(context) => BusinessData::AskContext {
                context: Box::new(context),
            },
            Err(e) => return denied(e),
        },
        BusinessQuery::ToolAttribution {
            event,
            pattern,
            after,
            limit,
        } => match log.tool_attribution(event, pattern, after, limit).await {
            Ok(view) => BusinessData::ToolAttribution {
                view: Box::new(view),
            },
            Err(error) => return store_failure(error),
        },
    };
    if let Err(e) = reader.authorize() {
        return denied(e);
    }
    BusinessResult::Completed { data }
}
fn store_failure(error: StoreError) -> BusinessResult {
    BusinessResult::Failed {
        code: match error {
            StoreError::NotProvided => FailureCode::NotFound,
            StoreError::Conflict => FailureCode::Conflict,
            StoreError::Invalid => FailureCode::Invalid,
            _ => FailureCode::Unavailable,
        },
        message: "original business database read failed",
    }
}
