//! Original Group TCP/OTLP and Flow receipt indexes in the business owner.
use super::{BusinessLog, StoreError, group_gateway, parts, row, schema};
use crate::{
    agent_graph::{self, Delegation, Intake, RecordedExchange, Span},
    context::FactReference,
};
use as_types::{
    driver::{NativeGroupObservation, NativeGroupTcp},
    flow::{EvidenceBody, ExchangeKey, ProtocolObservation, ProtocolRecordId},
    process_tree::ProcessKey,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use sha2::{Digest, Sha256};

/// One stable row in the Group view.
#[derive(Debug, Serialize)]
pub struct GroupRecord {
    #[serde(with = "as_types::decimal")]
    pub id: u64,
    #[serde(flatten)]
    evidence: GroupEvidence,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum GroupEvidence {
    A2a {
        capture_id: [u8; 32],
        parent: ProcessKey,
        child: ProcessKey,
        delegation: Box<Delegation>,
    },
    OpenclawToolReceipt {
        actor: ProcessKey,
        receipt: FactReference,
        call_index: u32,
        output_index: u32,
        tool_call_id: String,
        child_run_id: String,
        child_session_key: String,
        gateway: group_gateway::GatewayEvidence,
        gaps: Vec<&'static str>,
    },
}

/// A page of retained delegations, in descending commit order.
#[derive(Debug, Serialize)]
pub struct GroupPage {
    pub records: Vec<GroupRecord>,
    #[serde(with = "as_types::decimal::optional")]
    pub next_before: Option<u64>,
}

pub(super) fn process_row(conn: &Connection, key: ProcessKey) -> Result<i64, StoreError> {
    let existing = conn.query_row(
        "SELECT id FROM process_keys WHERE boot_id=?1 AND host_tgid=?2 AND birth_boottime_ns=?3 ORDER BY id LIMIT 1",
        params![key.boot_id.as_slice(), key.host_tgid, key.birth_boottime_ns.to_be_bytes().as_slice()],
        |row| row.get(0),
    ).optional()?;
    match existing {
        Some(id) => Ok(id),
        None => row::put(
            conn,
            "process_keys",
            schema::PROCESS_KEY,
            row::object(&key)?,
        ),
    }
}

const PAGE: &str = "WITH page AS (
    SELECT id FROM group_tcp_exchanges WHERE (?1 IS NULL OR id<?1)
    UNION ALL
    SELECT id FROM group_openclaw_receipts WHERE (?1 IS NULL OR id<?1)
    ORDER BY id DESC LIMIT ?2
    ) SELECT page.id,e.fact,NOT EXISTS (
    SELECT 1 FROM group_tcp_exchanges conflict JOIN process_keys other ON other.id=conflict.parent
    WHERE conflict.handoff=e.handoff AND conflict.id<>e.id
      AND other.boot_id=actor.boot_id AND other.host_tgid=actor.host_tgid
      AND other.birth_boottime_ns=actor.birth_boottime_ns)
    FROM page LEFT JOIN group_tcp_exchanges e ON e.id=page.id
    LEFT JOIN process_keys actor ON actor.id=e.parent ORDER BY page.id DESC";

// Both producers hold the business writer transaction. Neither table deletes rows.
const NEXT_ID: &str = "SELECT MAX(
    COALESCE((SELECT MAX(id) FROM group_tcp_exchanges),0),
    COALESCE((SELECT MAX(id) FROM group_openclaw_receipts),0)
    ) + 1";

pub(super) fn index_openclaw_receipts(
    conn: &Connection,
    protocol: i64,
    observation: &ProtocolObservation,
) -> Result<(), StoreError> {
    if observation.source.actor.process.is_none() {
        return Ok(());
    }
    for position in agent_graph::openclaw_receipts(observation) {
        let id = conn.query_row(NEXT_ID, [], |row| row.get::<_, i64>(0))?;
        conn.execute(
            "INSERT INTO group_openclaw_receipts
             (id,protocol,call_index,output_index,tool_call,child_run,child_session)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                id,
                protocol,
                u32::try_from(position.call_index).map_err(|_| StoreError::Invalid)?,
                u32::try_from(position.output_index).map_err(|_| StoreError::Invalid)?,
                position.tool_call_id,
                position.receipt.run_id,
                position.receipt.child_session_key,
            ],
        )?;
    }
    Ok(())
}

fn openclaw_evidence(conn: &Connection, id: i64) -> Result<GroupEvidence, StoreError> {
    let (actor, receipt, call_index, output_index, tool_call_id, child_run_id, child_session_key) =
        conn.query_row(
            "SELECT p.boot_id,p.host_tgid,p.birth_boottime_ns,
                r.flow_instance,r.exchange_sequence,r.sequence,
                g.call_index,g.output_index,g.tool_call,g.child_run,g.child_session
         FROM group_openclaw_receipts g JOIN protocol_records r ON r.id=g.protocol
         JOIN protocol_sources s ON s.id=r.source JOIN actors a ON a.id=s.actor
         JOIN process_keys p ON p.id=a.process WHERE g.id=?1",
            [id],
            |row| {
                Ok((
                    ProcessKey {
                        boot_id: row.get(0)?,
                        host_tgid: row.get(1)?,
                        birth_boottime_ns: u64::from_be_bytes(row.get(2)?),
                    },
                    FactReference::Protocol(ProtocolRecordId {
                        exchange: ExchangeKey {
                            flow_instance: row.get(3)?,
                            sequence: u64::from_be_bytes(row.get(4)?),
                        },
                        sequence: u64::from_be_bytes(row.get(5)?),
                    }),
                    row.get::<_, u32>(6)?,
                    row.get::<_, u32>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, String>(10)?,
                ))
            },
        )?;
    let (gateway, gaps) = group_gateway::resolve(conn, actor, &child_run_id, &child_session_key)?;
    Ok(GroupEvidence::OpenclawToolReceipt {
        actor,
        receipt,
        call_index,
        output_index,
        tool_call_id,
        child_run_id,
        child_session_key,
        gateway,
        gaps,
    })
}

impl BusinessLog {
    pub(crate) async fn record_group_unframed(
        &self,
        boot: [u8; 16],
        facts: Vec<NativeGroupTcp>,
    ) -> Result<(), StoreError> {
        if facts.is_empty() {
            return Ok(());
        }
        self.write_call(move |conn| {
            let tx = conn.transaction()?;
            {
                let mut insert =
                    tx.prepare("INSERT INTO group_unframed(boot,fact,body) VALUES(?1,?2,?3)")?;
                for mut fact in facts {
                    let body = match &mut fact.observation {
                        NativeGroupObservation::Packet { payload, .. } => {
                            Some(parts::write_evidence(
                                &tx,
                                &EvidenceBody::from_bytes(std::mem::take(payload)),
                            )?)
                        }
                        _ => None,
                    };
                    insert.execute(params![
                        boot.as_slice(),
                        serde_json::to_string(&fact)?,
                        body,
                    ])?;
                }
            }
            tx.commit()?;
            Ok(())
        })?
        .await
    }

    pub(crate) async fn record_group(&self, intake: Intake) -> Result<(), StoreError> {
        let intake = match intake {
            Intake::GatewayMessage {
                connection,
                from_client,
                sender,
                receiver,
                message,
            } => {
                return self
                    .record_gateway(connection, from_client, sender, receiver, message)
                    .await;
            }
            other => other,
        };
        self.write_call(move |conn| {
            let tx = conn.transaction()?;
            match intake {
                Intake::A2a(record, request, reply) => {
                    let existing = tx.query_row(
                        "SELECT 1 FROM group_tcp_exchanges WHERE capture=?1",
                        [record.capture.as_slice()], |r| r.get::<_, u32>(0),
                    ).optional()?;
                    if existing.is_some() {
                        return Err(StoreError::Conflict);
                    }
                    let request = parts::write_evidence(&tx, &EvidenceBody::from_bytes(request))?;
                    let response = parts::write_evidence(&tx, &EvidenceBody::from_bytes(reply))?;
                    let parent = process_row(&tx, record.parent)?;
                    let id = tx.query_row(NEXT_ID, [], |row| row.get::<_, i64>(0))?;
                    tx.execute("INSERT INTO group_tcp_exchanges(id,capture,request,response,parent,handoff,fact) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                        params![id, record.capture.as_slice(), request, response, parent, agent_graph::recorded_handoff(&record), serde_json::to_string(&record)?])?;
                }
                Intake::GatewayUnverified { source, generation, first_syn_ordinal, client_port, server_port, from_client, frames, reason } => {
                    group_gateway::record_unverified(&tx, crate::gateway::UnverifiedGatewaySource {
                        source, generation, first_syn_ordinal, client_port, server_port,
                    }, from_client, frames, reason)?;
                }
                Intake::GatewayHandshake(connection, request, reply) => {
                    group_gateway::record_handshake(&tx, *connection, request, reply)?;
                }
                Intake::GatewayMessage { .. } => return Err(StoreError::Invalid),
                Intake::GatewayGap { connection, from_client, frames, reason } => {
                    group_gateway::record_gap(&tx, connection, from_client, frames, reason)?;
                }
                Intake::Otlp(actor, raw, spans) => {
                    let process = process_row(&tx, actor)?;
                    let mut hash = Sha256::new();
                    hash.update(process.to_be_bytes());
                    hash.update(&raw);
                    let digest: [u8; 32] = hash.finalize().into();
                    let existing = tx.query_row(
                        "SELECT id FROM group_otlp_batches WHERE digest=?1",
                        [digest.as_slice()], |r| r.get::<_, i64>(0),
                    ).optional()?;
                    if existing.is_some() {
                        return Ok(());
                    }
                    let original = parts::write_evidence(&tx, &EvidenceBody::from_bytes(raw))?;
                    let batch = tx.query_row(
                        "INSERT INTO group_otlp_batches(digest,original) VALUES(?1,?2) RETURNING id",
                        params![digest.as_slice(), original], |r| r.get::<_, i64>(0),
                    )?;
                    for (span, digest) in spans {
                        tx.execute("INSERT INTO group_otlp_spans(batch,trace,span,actor,digest,fact) VALUES(?1,?2,?3,?4,?5,?6)
                                    ON CONFLICT(actor,trace,span,digest) DO NOTHING",
                            params![batch, span.trace_id, span.span_id, process, digest.as_slice(), serde_json::to_string(&span)?])?;
                    }
                }
            }
            tx.commit()?;
            Ok(())
        })?.await
    }

    pub(crate) async fn group_delegations(
        &self,
        before: Option<u64>,
        limit: u16,
    ) -> Result<GroupPage, StoreError> {
        let before = before
            .map(|id| i64::try_from(id).map_err(|_| StoreError::Invalid))
            .transpose()?;
        if !matches!(limit, 1..=64) || before == Some(0) {
            return Err(StoreError::Invalid);
        }
        self.call(move |conn| {
            let tx = conn.transaction()?;
            let mut statement = tx.prepare(PAGE)?;
            let mut span_query = tx.prepare(
                "SELECT s.fact FROM group_otlp_spans s JOIN process_keys p ON p.id=s.actor
                 WHERE s.trace=?1 AND p.boot_id=?2
                 AND ((p.host_tgid=?3 AND p.birth_boottime_ns=?4) OR
                      (p.host_tgid=?5 AND p.birth_boottime_ns=?6)) LIMIT 513",
            )?;
            let mut rows = statement.query(params![before, limit + 1])?;
            let mut records = Vec::new();
            while let Some(row) = rows.next()? {
                if records.len() == usize::from(limit) {
                    let next_before = records.last().map(|record: &GroupRecord| record.id);
                    return Ok(GroupPage {
                        records,
                        next_before,
                    });
                }
                let sql_id = row.get::<_, i64>(0)?;
                let id = u64::try_from(sql_id).map_err(|_| StoreError::Invalid)?;
                let Some(fact) = row.get::<_, Option<String>>(1)? else {
                    records.push(GroupRecord {
                        id,
                        evidence: openclaw_evidence(&tx, sql_id)?,
                    });
                    continue;
                };
                let fact: RecordedExchange = serde_json::from_str(&fact)?;
                let mut spans = Vec::<Span>::new();
                if row.get::<_, bool>(2)?
                    && let Some(trace) = agent_graph::recorded_trace(&fact)
                {
                    let mut originals = span_query.query(params![
                        trace,
                        fact.parent.boot_id.as_slice(),
                        fact.parent.host_tgid,
                        fact.parent.birth_boottime_ns.to_be_bytes().as_slice(),
                        fact.child.host_tgid,
                        fact.child.birth_boottime_ns.to_be_bytes().as_slice(),
                    ])?;
                    while let Some(original) = originals.next()? {
                        spans.push(serde_json::from_str(&original.get::<_, String>(0)?)?);
                    }
                    if spans.len() > 512 {
                        spans.clear();
                    }
                }
                records.push(GroupRecord {
                    id,
                    evidence: GroupEvidence::A2a {
                        capture_id: fact.capture,
                        parent: fact.parent,
                        child: fact.child,
                        delegation: Box::new(agent_graph::resolve_record(&fact, &spans, &tx)?),
                    },
                });
            }
            Ok(GroupPage {
                records,
                next_before: None,
            })
        })?
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_handoff_is_ambiguous_across_distinct_rows_of_the_same_process() {
        let db = Connection::open_in_memory().expect("database");
        schema::initialize(&db).expect("schema");
        let parent = ProcessKey {
            boot_id: [1; 16],
            host_tgid: 12,
            birth_boottime_ns: 90,
        };
        let first = process_row(&db, parent).expect("original parent");
        assert_eq!(process_row(&db, parent).expect("stable lookup"), first);
        let duplicate = row::put(
            &db,
            "process_keys",
            schema::PROCESS_KEY,
            row::object(&parent).expect("key"),
        )
        .expect("independent row for same key");
        let later = process_row(
            &db,
            ProcessKey {
                birth_boottime_ns: 91,
                ..parent
            },
        )
        .expect("distinct lifetime");
        let original =
            parts::write_evidence(&db, &EvidenceBody::from_bytes(vec![1])).expect("wire");
        for (index, actor) in [(1u8, first), (2, duplicate), (3, later)] {
            db.execute(
                "INSERT INTO group_tcp_exchanges(capture,request,response,parent,handoff,fact)
                       VALUES(?1,?2,?2,?3,'same-original-handoff','{}')",
                params![[index; 32].as_slice(), original, actor],
            )
            .expect("exchange");
        }
        let mut page = db.prepare(PAGE).expect("current WS projection");
        let rows = page
            .query_map(params![None::<i64>, 8], |r| r.get::<_, bool>(2))
            .expect("page")
            .collect::<Result<Vec<_>, _>>()
            .expect("rows");
        assert_eq!(rows, [true, false, false]);
    }
}
