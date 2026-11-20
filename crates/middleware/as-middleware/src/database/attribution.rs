//! Candidate selection over retained source records in the existing SQLite owner.
use super::{BusinessLog, StoreError, chat, event, key, query::page_limit};
use crate::{
    attribution::{
        ActorScope, AttributionCandidate, AttributionPattern, TargetComparison, ThreadIdentity,
        ToolAttribution,
    },
    context::FactReference,
};
use as_types::{
    dispatch_event::EventData,
    flow::{ChatMessageId, ChatPart, ChatPartRef},
    process_tree::{EventKey, ProcessKey},
};
use rusqlite::{Connection, OptionalExtension, params};

fn original_event(conn: &Connection, reference: EventKey) -> Result<EventData, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT q.event FROM fact_records f JOIN questions q ON q.id=f.record_id
         WHERE f.reference=?1 AND f.record_kind='question'
         UNION
         SELECT o.event FROM fact_records f JOIN operation_reports o ON o.id=f.record_id
         WHERE f.reference=?1 AND f.record_kind='operation'",
    )?;
    let mut original = None;
    for id in stmt.query_map([key(&FactReference::Event(reference))?], |row| row.get(0))? {
        let event = event::read_event(conn, id?)?;
        if EventKey::from(&event) != reference
            || original.as_ref().is_some_and(|prior| prior != &event)
        {
            return Err(StoreError::Conflict);
        }
        original = Some(event);
    }
    original.ok_or(StoreError::NotProvided)
}

fn actor_identity(
    event: &EventData,
    scope: ActorScope,
) -> Result<(ProcessKey, Option<ThreadIdentity>), StoreError> {
    let h = event.header();
    let (tgid, birth) = match scope {
        ActorScope::PhysicalParent => (
            event.process.parent_pid,
            event.process.parent_birth_boottime_ns,
        ),
        ActorScope::SameProcess | ActorScope::SameThread => {
            (h.actor_tgid, h.actor_birth_boottime_ns)
        }
    };
    let process = ProcessKey {
        boot_id: event.host_boot_id().ok_or(StoreError::NotProvided)?,
        host_tgid: tgid.ok_or(StoreError::NotProvided)?,
        birth_boottime_ns: birth.ok_or(StoreError::NotProvided)?,
    };
    let thread = match scope {
        ActorScope::SameThread => Some(ThreadIdentity {
            tid: h.actor_tid.ok_or(StoreError::NotProvided)?,
            birth_boottime_ns: h
                .actor_thread_birth_boottime_ns
                .ok_or(StoreError::NotProvided)?,
        }),
        ActorScope::SameProcess | ActorScope::PhysicalParent => None,
    };
    Ok((process, thread))
}

impl BusinessLog {
    /// Compare retained originals within one SQLite read transaction.
    pub async fn tool_attribution(
        &self,
        event: EventKey,
        pattern: AttributionPattern,
        after: Option<ChatMessageId>,
        limit: u16,
    ) -> Result<ToolAttribution, StoreError> {
        self.call(move |conn| {
            let tx = conn.transaction()?;
            let result = read_attribution(&tx, event, pattern, after, limit)?;
            tx.commit()?;
            Ok(result)
        })?
        .await
    }
}

pub(super) fn read_attribution(
    conn: &Connection,
    event: EventKey,
    pattern: AttributionPattern,
    after: Option<ChatMessageId>,
    limit: u16,
) -> Result<ToolAttribution, StoreError> {
    let limit = page_limit(limit)?;
    let original = original_event(conn, event)?;
    if !pattern.target.applies(&original) {
        return Err(StoreError::Invalid);
    }
    let (process, thread) = actor_identity(&original, pattern.actor)?;
    let event_boottime_ns = original
        .header()
        .source_boottime_ns
        .ok_or(StoreError::NotProvided)?;
    let event_target = pattern.target.observed_target(&original);
    let after_row = after.map(|id| {
                conn.query_row(
                    "SELECT id FROM messages WHERE flow_instance=?1 AND exchange_sequence=?2 AND sequence=?3",
                    params![id.exchange.flow_instance.as_slice(), id.exchange.sequence.to_be_bytes().as_slice(), id.sequence.to_be_bytes().as_slice()],
                    |row| row.get::<_, i64>(0),
                ).optional()?.ok_or(StoreError::Invalid)
            }).transpose()?;
    let mut owners = {
        let mut stmt = conn.prepare(
                    "SELECT r.id FROM message_revisions r
                     JOIN messages m ON m.id=r.message
                     JOIN protocol_sources s ON s.id=r.source
                     JOIN actors a ON a.id=s.actor
                     JOIN process_keys p ON p.id=a.process
                     WHERE r.state='complete' AND r.derived=0 AND m.origin='response_output'
                       AND s.side='upstream_response'
                       AND r.last_seen_boottime_ns BETWEEN ?1 AND ?2
                       AND p.boot_id=?3 AND p.host_tgid=?4 AND p.birth_boottime_ns=?5
                       AND (?6 IS NULL OR (a.tid=?6 AND a.thread_birth_boottime_ns=?7))
                       AND (?8 IS NULL OR m.id>?8)
                       AND NOT EXISTS(SELECT 1 FROM message_revisions newer WHERE newer.message=m.id AND newer.revision>r.revision)
                       AND EXISTS(SELECT 1 FROM parts WHERE set_id=r.parts AND type='tool_call')
                     ORDER BY m.id LIMIT ?9",
                )?;
        let lower = event_boottime_ns
            .saturating_sub(pattern.lookback_ns)
            .to_be_bytes();
        let upper = event_boottime_ns.to_be_bytes();
        let birth = process.birth_boottime_ns.to_be_bytes();
        let thread_birth = thread.as_ref().map(|t| t.birth_boottime_ns.to_be_bytes());
        stmt.query_map(
            params![
                lower.as_slice(),
                upper.as_slice(),
                process.boot_id.as_slice(),
                process.host_tgid,
                birth.as_slice(),
                thread.as_ref().map(|t| t.tid),
                thread_birth.as_ref().map(|value| value.as_slice()),
                after_row,
                limit + 1
            ],
            |row| row.get::<_, i64>(0),
        )?
        .collect::<Result<Vec<_>, _>>()?
    };
    let more = owners.len() > usize::from(limit);
    owners.truncate(usize::from(limit));
    let mut candidates = Vec::new();
    let mut next_after = None;
    for owner in owners {
        let message = chat::read_revision(conn, owner)?;
        next_after = Some(message.id);
        for part in &message.observed_parts {
            if let ChatPart::ToolCall { id, arguments, .. } = part {
                candidates.push(AttributionCandidate {
                    call: ChatPartRef {
                        message: message.id,
                        part_id: *id,
                    },
                    message_revision: message.revision,
                    response_observed_ns: message.last_seen_boottime_ns,
                    target_comparison: if message.quality.is_empty() {
                        pattern
                            .target
                            .compare(event_target.as_ref(), arguments.as_ref())
                    } else {
                        TargetComparison::SourceIncomplete
                    },
                });
            }
        }
    }

    Ok(ToolAttribution {
        event,
        pattern,
        required_connection_process: process,
        required_connection_thread: thread,
        event_boottime_ns,
        event_target,
        candidates,
        next_after: next_after.filter(|_| more),
    })
}
