//! Private relational retirement of this database's original write units.
use super::{Inner, StoreError};
use as_types::log_storage::{LogRetention, sqlite_file_bytes};
use rusqlite::{Connection, OptionalExtension, params};
use std::io;

// Original root objects. Supporting rows never become independent history records.
const ROOTS: &[&str] = &[
    "protocol_records",
    "messages",
    "message_revisions",
    "deltas",
    "questions",
    "operation_reports",
    "native_request_handoffs",
    "native_tool_executions",
    "native_request_returns",
    "native_futex_selections",
    "group_tcp_exchanges",
    "group_unverified_streams",
    "group_gateway_connections",
    "group_gateway_messages",
    "group_otlp_batches",
    "group_unframed",
    "credential_facts",
    "source_gaps",
    "flow_status",
    "feedback",
    "exchange_failures",
    "context_gaps",
    "relations",
];
fn root_key(table: &str) -> &'static str {
    if table == "group_gateway_connections" {
        "capture"
    } else {
        "id"
    }
}
const SUPPORT: &[&str] = &[
    "deliveries",
    "events",
    "event_headers",
    "process_payloads",
    "file_payloads",
    "network_payloads",
    "rule_hits",
    "process_info",
    "protocol_sources",
    "actors",
    "channels",
    "credential_ids",
    "process_keys",
    "part_sets",
    "evidence_sets",
    "facts",
];
fn backfill(table: &str) -> String {
    let key = root_key(table);
    // Unary + removes INTEGER PRIMARY KEY affinity without casting its value.
    // Both columns of (kind, object) must remain usable by the correlated lookup.
    format!(
        "INSERT OR IGNORE INTO retention_records(generation,kind,object) SELECT (SELECT generation FROM retention_state WHERE id=1)+row_number() OVER(ORDER BY rowid),'{table}',{key} FROM {table} p WHERE NOT EXISTS(SELECT 1 FROM retention_records r WHERE r.kind='{table}' AND r.object=+p.{key}) ORDER BY rowid;"
    )
}
pub(super) fn initialize(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch("CREATE TEMP TABLE IF NOT EXISTS retention_state(id INTEGER PRIMARY KEY CHECK(id=1),generation INTEGER NOT NULL);
      INSERT OR IGNORE INTO retention_state VALUES(1,0);
      CREATE TABLE IF NOT EXISTS retention_records(id INTEGER PRIMARY KEY AUTOINCREMENT,generation INTEGER NOT NULL,kind TEXT NOT NULL,object BLOB NOT NULL,UNIQUE(kind,object));
      CREATE INDEX IF NOT EXISTS retention_generation ON retention_records(generation,id);")?;
    for table in ROOTS {
        let key = root_key(table);
        // Existing objects retain their own proven table order. No cross-table event time is invented.
        conn.execute_batch(&backfill(table))?;
        conn.execute_batch(&format!("
          UPDATE retention_state SET generation=(SELECT coalesce(max(generation),0) FROM retention_records) WHERE id=1;
          CREATE TEMP TRIGGER IF NOT EXISTS retention_insert_{table} AFTER INSERT ON main.{table} BEGIN
            INSERT INTO retention_records(generation,kind,object) VALUES((SELECT generation FROM retention_state WHERE id=1),'{table}',NEW.{key});
          END;
          CREATE TEMP TRIGGER IF NOT EXISTS retention_delete_{table} AFTER DELETE ON main.{table} BEGIN
            DELETE FROM retention_records WHERE kind='{table}' AND object=OLD.{key};
          END;"))?;
    }
    Ok(())
}
pub(super) fn begin_write(conn: &Connection) -> Result<(), StoreError> {
    // A frozen Ask context is also readable through a query-only connection.
    // Required writes still encounter their original SQLite read-only error.
    let read_only: bool = conn.query_row("PRAGMA query_only", [], |r| r.get(0))?;
    if read_only {
        return Ok(());
    }
    conn.execute(
        "UPDATE retention_state SET generation=generation+1 WHERE id=1",
        [],
    )?;
    Ok(())
}
pub(super) fn checkpoint(conn: &Connection) -> Result<(), StoreError> {
    let Some(path) = conn.path() else {
        return Ok(());
    };
    let mut wal = std::ffi::OsString::from(path);
    wal.push("-wal");
    if std::fs::metadata(std::path::Path::new(&wal)).map_or(0, |m| m.len()) >= 4 * 1024 * 1024 {
        truncate_wal(conn)?;
    }
    Ok(())
}
fn truncate_wal(conn: &Connection) -> Result<(), StoreError> {
    let busy: i32 = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0))?;
    if busy != 0 {
        return Err(StoreError::Io(io::Error::new(
            io::ErrorKind::WouldBlock,
            "business checkpoint is busy",
        )));
    }
    Ok(())
}
impl LogRetention for Inner {
    fn retire_oldest(&self) -> io::Result<bool> {
        let mut owner = self
            .database
            .lock()
            .map_err(|_| io::Error::other("business database state"))?;
        let Some(conn) = owner.as_mut() else {
            return Ok(false);
        };
        let before = sqlite_file_bytes(&self.path)?;
        truncate_wal(conn).map_err(io::Error::other)?;
        if sqlite_file_bytes(&self.path)? < before {
            return Ok(true);
        }
        let pins = self
            .pins
            .lock()
            .map_err(|_| io::Error::other("business retention pins"))?;
        let questions = self
            .questions
            .lock()
            .map_err(|_| io::Error::other("business question retention"))?;
        retire(conn, &pins, &questions).map_err(io::Error::other)
    }
}

// A reference expression is generated only from this validated schema, not client input.
struct Edge {
    child: String,
    parent: String,
    relation: String,
    owned: bool,
}
fn quoted(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}
fn ownership(table: &str, column: &str) -> bool {
    matches!(
        (table, column),
        ("evidence_segments", "set_id")
            | ("parts", "set_id")
            | ("namespace_tgids", "info")
            | (
                "protocol_native_ids"
                    | "protocol_session_fields"
                    | "protocol_session_values"
                    | "part_native_ids"
                    | "record_quality"
                    | "message_quality"
                    | "message_record_refs"
                    | "usage"
                    | "delta_changes"
                    | "question_choices"
                    | "ask_window_items"
                    | "relation_evidence"
                    | "group_unverified_frames"
                    | "status_profiles",
                "owner"
            )
            | (
                "event_cgroup_lineage" | "event_actor_namespace_tids",
                "header"
            )
            | (
                "event_boot_scopes" | "event_syscalls" | "event_stacks",
                "event"
            )
            | ("event_stack_frames", "stack")
            | ("file_mount_effects", "file_payload")
            | ("network_dns_snapshots", "network_payload")
    )
}
fn edges(conn: &Connection) -> Result<(Vec<String>, Vec<Edge>), StoreError> {
    let mut query = conn.prepare("SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%' AND name NOT LIKE 'retention_%' ORDER BY name")?;
    let tables = query
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut edges = Vec::new();
    for child in &tables {
        let mut query = conn.prepare(&format!("PRAGMA foreign_key_list({})", quoted(child)))?;
        let values = query
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        for (parent, from, to) in values {
            edges.push(Edge {
                child: child.clone(),
                parent,
                relation: format!("c.{}=p.{}", quoted(&from), quoted(&to)),
                owned: ownership(child, &from),
            });
        }
    }
    let mut add = |child: &str, parent: &str, relation: &str, owned| {
        edges.push(Edge {
            child: child.into(),
            parent: parent.into(),
            relation: relation.into(),
            owned,
        })
    };
    for (kind, table) in [
        ("file", "file_payloads"),
        ("process", "process_payloads"),
        ("network", "network_payloads"),
    ] {
        add(
            "events",
            table,
            &format!("c.payload_kind='{kind}' AND c.payload=p.id"),
            false,
        );
    }
    for (kind, table) in [
        ("observation", "protocol_records"),
        ("started", "message_revisions"),
        ("finished", "message_revisions"),
        ("delta", "deltas"),
        ("credential_use", "credential_facts"),
        ("rule_feedback", "feedback"),
        ("exchange_failed", "exchange_failures"),
        ("gap", "source_gaps"),
        ("status", "flow_status"),
    ] {
        add(
            "chat_inputs",
            table,
            &format!("c.event_kind='{kind}' AND c.payload_row=p.id"),
            false,
        );
    }
    for (kind, table) in [
        ("question", "questions"),
        ("operation", "operation_reports"),
        ("message", "messages"),
        ("protocol", "protocol_records"),
        ("native_tool", "native_tool_executions"),
        ("native_request_handoff", "native_request_handoffs"),
        ("native_request_return", "native_request_returns"),
        ("native_futex_selection", "native_futex_selections"),
    ] {
        add(
            "fact_records",
            table,
            &format!("c.record_kind='{kind}' AND c.record_id=p.id"),
            false,
        );
    }
    add(
        "argv",
        "process_info",
        "c.info=p.id AND c.owner IN ('argv','parent_argv')",
        true,
    );
    add(
        "argv",
        "process_payloads",
        "c.info=p.id AND c.owner='target_argv'",
        true,
    );
    add(
        "message_record_refs",
        "protocol_records",
        "c.flow_instance=p.flow_instance AND c.exchange_sequence=p.exchange_sequence AND c.sequence=p.sequence",
        false,
    );
    // Losing a defining child retires its complete original object, never a modified subset.
    add(
        "message_revisions",
        "message_record_refs",
        "c.id=p.owner",
        false,
    );
    add("relations", "relation_evidence", "c.id=p.owner", false);
    add("ask_windows", "ask_window_items", "c.id=p.owner", false);
    add("questions", "ask_windows", "c.id=p.question", false);
    add(
        "ask_window_items",
        "message_revisions",
        "c.message=p.message AND c.revision=p.revision",
        false,
    );
    // A delta chain and its stored message base are one reconstructable original unit.
    add(
        "deltas",
        "messages",
        "EXISTS(SELECT 1 FROM fact_records f WHERE f.record_kind='message' AND f.record_id=p.id AND f.reference=c.message_ref)",
        false,
    );
    add(
        "messages",
        "deltas",
        "EXISTS(SELECT 1 FROM fact_records f WHERE f.record_kind='message' AND f.record_id=c.id AND f.reference=p.message_ref)",
        false,
    );
    add(
        "feedback_bindings",
        "facts",
        "c.denial_ref=p.reference OR c.call_ref=p.reference",
        false,
    );
    add("context_gaps", "facts", "c.at_ref=p.reference", false);
    Ok((tables, edges))
}
fn extend(conn: &Connection, edges: &[Edge]) -> Result<usize, StoreError> {
    let mut changed = 0;
    for edge in edges {
        changed += conn.execute(&format!("INSERT OR IGNORE INTO temp.retiring SELECT ?1,c.rowid FROM {} c JOIN {} p ON {} JOIN temp.retiring r ON r.kind=?2 AND r.object=p.rowid",quoted(&edge.child),quoted(&edge.parent),edge.relation),params![edge.child,edge.parent])?;
    }
    changed += conn.execute("INSERT OR IGNORE INTO temp.retiring SELECT 'facts',f.rowid FROM facts f WHERE EXISTS(SELECT 1 FROM fact_records fr JOIN temp.retiring r ON r.kind='fact_records' AND r.object=fr.rowid WHERE fr.reference=f.reference) AND NOT EXISTS(SELECT 1 FROM fact_records fr WHERE fr.reference=f.reference AND NOT EXISTS(SELECT 1 FROM temp.retiring r WHERE r.kind='fact_records' AND r.object=fr.rowid))",[])?;
    changed += conn.execute("INSERT OR IGNORE INTO temp.retiring SELECT 'messages',m.id FROM messages m WHERE EXISTS(SELECT 1 FROM message_revisions v JOIN temp.retiring r ON r.kind='message_revisions' AND r.object=v.id WHERE v.message=m.id) AND NOT EXISTS(SELECT 1 FROM message_revisions v WHERE v.message=m.id AND NOT EXISTS(SELECT 1 FROM temp.retiring r WHERE r.kind='message_revisions' AND r.object=v.id))",[])?;
    Ok(changed)
}
fn erase(conn: &Connection, tables: &[String]) -> Result<(), StoreError> {
    for table in tables {
        conn.execute(
            &format!(
                "DELETE FROM {} WHERE rowid IN(SELECT object FROM temp.retiring WHERE kind=?1)",
                quoted(table)
            ),
            [table],
        )?;
    }
    conn.execute("DELETE FROM temp.retiring", [])?;
    Ok(())
}
fn retire(
    conn: &mut Connection,
    pins: &std::collections::BTreeMap<String, usize>,
    questions: &std::collections::BTreeSet<[u8; 16]>,
) -> Result<bool, StoreError> {
    let (tables, edges) = edges(conn)?;
    let mut after = 0i64;
    loop {
        let candidate=conn.query_row("SELECT generation FROM retention_records WHERE id>?1 AND generation<(SELECT max(generation) FROM retention_records) ORDER BY id LIMIT 1",[after],|r|r.get::<_,i64>(0)).optional()?;
        let Some(generation) = candidate else {
            return Ok(false);
        };
        after = conn.query_row(
            "SELECT max(id) FROM retention_records WHERE generation=?1",
            [generation],
            |r| r.get(0),
        )?;
        let tx = conn.transaction()?;
        tx.execute_batch("PRAGMA defer_foreign_keys=ON; CREATE TEMP TABLE IF NOT EXISTS retiring(kind TEXT NOT NULL,object INTEGER NOT NULL,PRIMARY KEY(kind,object)); DELETE FROM temp.retiring;")?;
        for table in ROOTS {
            tx.execute(&format!("INSERT OR IGNORE INTO temp.retiring SELECT ?1,p.rowid FROM {table} p JOIN retention_records r ON r.kind=?1 AND r.object=p.{} WHERE r.generation=?2",root_key(table)),params![table,generation])?;
        }
        while extend(&tx, &edges)? != 0 {}
        let mut latest = false;
        for table in ROOTS {
            latest |= tx.query_row(&format!("SELECT EXISTS(SELECT 1 FROM {table} p JOIN temp.retiring d ON d.kind=?1 AND d.object=p.rowid JOIN retention_records r ON r.kind=?1 AND r.object=p.{} WHERE r.generation=(SELECT max(generation) FROM retention_records))",root_key(table)),[table],|r|r.get::<_,bool>(0))?;
        }
        let mut pinned = latest;
        for reference in pins.keys() {
            pinned |= tx.query_row("SELECT EXISTS(SELECT 1 FROM facts f JOIN temp.retiring r ON r.kind='facts' AND r.object=f.rowid WHERE f.reference=?1)",[reference],|r|r.get::<_,bool>(0))?;
            pinned |= tx.query_row("SELECT EXISTS(SELECT 1 FROM feedback_bindings b JOIN temp.retiring r ON r.kind='feedback_bindings' AND r.object=b.rowid WHERE b.denial_ref=?1 OR b.call_ref=?1)",[reference],|r|r.get::<_,bool>(0))?;
            pinned |= tx.query_row("SELECT EXISTS(SELECT 1 FROM feedback b JOIN temp.retiring r ON r.kind='feedback' AND r.object=b.id WHERE b.denial_ref=?1 OR b.call_ref=?1)",[reference],|r|r.get::<_,bool>(0))?;
        }
        for question in questions {
            pinned |= tx.query_row("SELECT EXISTS(SELECT 1 FROM questions q JOIN temp.retiring r ON r.kind='questions' AND r.object=q.id WHERE q.decision_id=?1)",[question.as_slice()],|r|r.get::<_,bool>(0))?;
        }
        if pinned {
            continue;
        }
        erase(&tx, &tables)?;
        // Supporting values are collected only after all original incoming references disappear.
        loop {
            let mut added = 0;
            for table in SUPPORT {
                let incoming = edges
                    .iter()
                    .filter(|e| e.parent == *table && !e.owned)
                    .map(|e| {
                        format!(
                            "NOT EXISTS(SELECT 1 FROM {} c WHERE {})",
                            quoted(&e.child),
                            e.relation
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" AND ");
                if !incoming.is_empty() {
                    added += tx.execute(&format!("INSERT OR IGNORE INTO temp.retiring SELECT ?1,p.rowid FROM {} p WHERE {incoming}",quoted(table)),[table])?;
                }
            }
            if added == 0 {
                break;
            }
            while extend(&tx, &edges)? != 0 {}
            erase(&tx, &tables)?;
        }

        tx.commit()?;
        truncate_wal(conn)?;
        return Ok(true);
    }
}

#[cfg(test)]
mod tests;
