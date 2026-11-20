//! Original Gateway frames and typed public-field indexes in the existing business transaction.
use super::{StoreError, group::process_row, parts};
use crate::agent_graph::{
    gateway::{AcceptedSpawn, ChildTerminal, GatewayFact, ParentPublication},
    websocket::{Message, WireFrame},
};
use crate::gateway::{
    GatewayConnection, GatewayDecodeIssue, GatewayFrameReference, GatewayReference,
    UnverifiedGatewaySource,
};
use as_types::{flow::EvidenceBody, process_tree::ProcessKey};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub(crate) struct Observation<T> {
    reference: GatewayReference,
    fact: T,
}

/// Every relevant original observation remains available even when its identities conflict.
#[derive(Debug, Default, Serialize)]
pub(crate) struct GatewayEvidence {
    accepted_spawns: Vec<Observation<AcceptedSpawn>>,
    child_terminals: Vec<Observation<ChildTerminal>>,
    parent_publications: Vec<Observation<ParentPublication>>,
}

pub(super) fn record_handshake(
    conn: &Connection,
    connection: GatewayConnection,
    request: Vec<u8>,
    reply: Vec<u8>,
) -> Result<(), StoreError> {
    if connection.client_port == 0
        || connection.server_port == 0
        || connection.client_port == connection.server_port
        || connection.client_socket_cookie == 0
        || connection.server_socket_cookie == 0
        || connection.client.boot_id != connection.server.boot_id
        || request.is_empty()
        || reply.is_empty()
    {
        return Err(StoreError::Invalid);
    }
    if conn
        .query_row(
            "SELECT 1 FROM group_gateway_connections WHERE capture=?1",
            [connection.capture.as_slice()],
            |r| r.get::<_, u8>(0),
        )
        .optional()?
        .is_some()
    {
        return Err(StoreError::Conflict);
    }
    let client = process_row(conn, connection.client)?;
    let server = process_row(conn, connection.server)?;
    let request = parts::write_evidence(conn, &EvidenceBody::from_bytes(request))?;
    let reply = parts::write_evidence(conn, &EvidenceBody::from_bytes(reply))?;
    conn.execute(
        "INSERT INTO group_gateway_connections
         (capture,source,generation,first_syn_ordinal,client_port,server_port,client,server,request,reply)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        params![connection.capture.as_slice(), connection.source.as_slice(),
            connection.generation.to_be_bytes().as_slice(),
            connection.first_syn_ordinal.to_be_bytes().as_slice(),
            connection.client_port, connection.server_port, client, server, request, reply],
    )?;
    conn.execute(
        "INSERT INTO group_gateway_socket_identity(capture,client_cookie,server_cookie)
         VALUES(?1,?2,?3)",
        params![
            connection.capture.as_slice(),
            connection.client_socket_cookie.to_be_bytes().as_slice(),
            connection.server_socket_cookie.to_be_bytes().as_slice(),
        ],
    )?;
    Ok(())
}

fn expected_actors(
    conn: &Connection,
    capture: &[u8; 32],
    from_client: bool,
) -> Result<(i64, i64), StoreError> {
    let (client, server) = conn.query_row(
        "SELECT client,server FROM group_gateway_connections WHERE capture=?1",
        [capture.as_slice()],
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
    )?;
    Ok(if from_client {
        (client, server)
    } else {
        (server, client)
    })
}

fn write_frames(
    conn: &Connection,
    capture: [u8; 32],
    from_client: bool,
    sender: Option<i64>,
    receiver: Option<i64>,
    frames: Vec<WireFrame>,
    reason: Option<&str>,
) -> Result<i64, StoreError> {
    if frames.is_empty() && reason.is_none() {
        return Err(StoreError::Invalid);
    }
    conn.execute(
        "INSERT INTO group_gateway_messages(connection,from_client,sender,receiver,reason)
         VALUES(?1,?2,?3,?4,?5)",
        params![capture.as_slice(), from_client, sender, receiver, reason],
    )?;
    let message = conn.last_insert_rowid();
    let mut previous_end = None;
    for (ordinal, frame) in frames.into_iter().enumerate() {
        let length = u64::try_from(frame.wire.len()).map_err(|_| StoreError::Invalid)?;
        if reason.is_none() {
            let end = frame
                .offset
                .checked_add(length)
                .ok_or(StoreError::Invalid)?;
            if length == 0 || previous_end.is_some_and(|previous| frame.offset < previous) {
                return Err(StoreError::Invalid);
            }
            previous_end = Some(end);
        }
        let wire = parts::write_evidence(conn, &EvidenceBody::from_bytes(frame.wire))?;
        conn.execute(
            "INSERT INTO group_gateway_frames(message,ordinal,connection,from_client,stream_offset,wire)
             VALUES(?1,?2,?3,?4,?5,?6)",
            params![message, i64::try_from(ordinal).map_err(|_| StoreError::Invalid)?,
                capture.as_slice(), from_client, frame.offset.to_be_bytes().as_slice(), wire],
        )?;
    }
    Ok(message)
}

/// Store one verified original message once, checking actual actors and recording only its decode issue.
pub(crate) fn record_message(
    conn: &Connection,
    capture: [u8; 32],
    from_client: bool,
    sender: ProcessKey,
    receiver: ProcessKey,
    message: Message,
    issue: Option<GatewayDecodeIssue>,
) -> Result<GatewayReference, StoreError> {
    let (expected_sender, expected_receiver) = expected_actors(conn, &capture, from_client)?;
    let sender = process_row(conn, sender)?;
    let receiver = process_row(conn, receiver)?;
    if (sender, receiver) != (expected_sender, expected_receiver) {
        return Err(StoreError::Conflict);
    }
    let message = write_frames(
        conn,
        capture,
        from_client,
        Some(sender),
        Some(receiver),
        message.frames,
        None,
    )?;
    conn.execute(
        "UPDATE group_gateway_messages SET decode_issue=?1 WHERE id=?2",
        params![issue.map(|i| super::row::string(&i)).transpose()?, message],
    )?;
    reference(conn, message)
}

/// Store an already-projected Group fact referencing its original message; no protocol parsing.
pub(crate) fn index_fact(
    conn: &Connection,
    message: i64,
    fact: &GatewayFact,
) -> Result<(), StoreError> {
    let (kind, run, session) = fact.index();
    conn.execute(
        "INSERT INTO group_gateway_facts(message,kind,run_id,session_key,fact) VALUES(?1,?2,?3,?4,?5)",
        params![message, kind, run, session, serde_json::to_string(fact)?],
    )?;
    Ok(())
}

pub(super) fn record_gap(
    conn: &Connection,
    capture: [u8; 32],
    from_client: bool,
    frames: Vec<WireFrame>,
    reason: &str,
) -> Result<(), StoreError> {
    if reason.is_empty() {
        return Err(StoreError::Invalid);
    }
    conn.query_row(
        "SELECT 1 FROM group_gateway_connections WHERE capture=?1",
        [capture.as_slice()],
        |row| row.get::<_, u8>(0),
    )?;
    write_frames(conn, capture, from_client, None, None, frames, Some(reason))?;
    Ok(())
}

pub(super) fn record_unverified(
    conn: &Connection,
    source: UnverifiedGatewaySource,
    from_client: bool,
    frames: Vec<WireFrame>,
    reason: &str,
) -> Result<(), StoreError> {
    if reason.is_empty()
        || source.client_port == 0
        || source.server_port == 0
        || source.client_port == source.server_port
    {
        return Err(StoreError::Invalid);
    }
    conn.execute(
        "INSERT INTO group_unverified_streams(source,generation,first_syn_ordinal,client_port,server_port,from_client,reason)
         VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![source.source.as_slice(), source.generation.to_be_bytes().as_slice(),
            source.first_syn_ordinal.to_be_bytes().as_slice(), source.client_port, source.server_port,
            from_client, reason],
    )?;
    let owner = conn.last_insert_rowid();
    for (ordinal, frame) in frames.into_iter().enumerate() {
        let wire = parts::write_evidence(conn, &EvidenceBody::from_bytes(frame.wire))?;
        conn.execute(
            "INSERT INTO group_unverified_frames(owner,ordinal,stream_offset,wire) VALUES(?1,?2,?3,?4)",
            params![owner, i64::try_from(ordinal).map_err(|_| StoreError::Invalid)?,
                frame.offset.to_be_bytes().as_slice(), wire],
        )?;
    }
    Ok(())
}

/// Rebuild the specified original message/frame coordinates from the same raw evidence rows.
pub(crate) fn reference(conn: &Connection, id: i64) -> Result<GatewayReference, StoreError> {
    let (capture, from_client) = conn.query_row(
        "SELECT connection,from_client FROM group_gateway_messages WHERE id=?1",
        [id],
        |row| Ok((row.get::<_, [u8; 32]>(0)?, row.get::<_, bool>(1)?)),
    )?;
    let mut statement = conn.prepare(
        "SELECT f.stream_offset,e.original_length FROM group_gateway_frames f
         JOIN evidence_sets e ON e.id=f.wire WHERE f.message=?1 ORDER BY f.ordinal",
    )?;
    let mut rows = statement.query([id])?;
    let mut frames = Vec::new();
    while let Some(row) = rows.next()? {
        frames.push(GatewayFrameReference {
            offset: u64::from_be_bytes(row.get(0)?),
            length: u64::from_be_bytes(row.get(1)?),
        });
    }
    Ok(GatewayReference {
        id: u64::try_from(id).map_err(|_| StoreError::Invalid)?,
        capture,
        from_client,
        frames,
    })
}

/// Read the stored verified connection and complete process/socket identities without filling values.
pub(crate) fn connection(
    conn: &Connection,
    capture: &[u8; 32],
) -> Result<GatewayConnection, StoreError> {
    Ok(conn.query_row("SELECT c.source,c.generation,c.first_syn_ordinal,c.client_port,c.server_port,
        p.boot_id,p.host_tgid,p.birth_boottime_ns,s.boot_id,s.host_tgid,s.birth_boottime_ns,i.client_cookie,i.server_cookie
        FROM group_gateway_connections c JOIN process_keys p ON p.id=c.client JOIN process_keys s ON s.id=c.server
        JOIN group_gateway_socket_identity i ON i.capture=c.capture WHERE c.capture=?1",[capture.as_slice()],|r| {
        Ok(GatewayConnection { capture:*capture, source:r.get(0)?, generation:u64::from_be_bytes(r.get(1)?),
            first_syn_ordinal:u64::from_be_bytes(r.get(2)?),client_port:r.get(3)?,server_port:r.get(4)?,
            client:ProcessKey {boot_id:r.get(5)?,host_tgid:r.get(6)?,birth_boottime_ns:u64::from_be_bytes(r.get(7)?)},
            server:ProcessKey {boot_id:r.get(8)?,host_tgid:r.get(9)?,birth_boottime_ns:u64::from_be_bytes(r.get(10)?)},
            client_socket_cookie:u64::from_be_bytes(r.get(11)?),server_socket_cookie:u64::from_be_bytes(r.get(12)?) })
    })?)
}

/// Read original wire, with no application interpretation.
pub(crate) fn frames(conn: &Connection, id: i64) -> Result<Vec<WireFrame>, StoreError> {
    let reason: Option<String> = conn.query_row(
        "SELECT reason FROM group_gateway_messages WHERE id=?1",
        [id],
        |r| r.get(0),
    )?;
    if reason.is_some() {
        return Err(StoreError::Invalid);
    }
    let rows = conn
        .prepare(
            "SELECT stream_offset,wire FROM group_gateway_frames WHERE message=?1 ORDER BY ordinal",
        )?
        .query_map([id], |r| {
            Ok((u64::from_be_bytes(r.get(0)?), r.get::<_, i64>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(offset, id)| {
            let body = parts::read_evidence(conn, id)?;
            let wire = match body.segments.into_iter().next() {
                Some(as_types::flow::EvidenceSegment::Bytes { offset: 0, data })
                    if body.original_length == Some(data.len() as u64) =>
                {
                    data
                }
                _ => return Err(StoreError::Invalid),
            };
            Ok(WireFrame { offset, wire })
        })
        .collect()
}

/// Reassemble only the supplied original complete-message frames using the existing passive WS codec.
pub(crate) fn assemble(frames: Vec<WireFrame>, from_client: bool) -> Result<Message, StoreError> {
    use crate::agent_graph::websocket::{Assembler, decode_frame};
    let maximum = frames.iter().try_fold(0usize, |sum, f| {
        sum.checked_add(f.wire.len()).ok_or(StoreError::Invalid)
    })?;
    let mut assembler = Assembler::new(from_client, maximum);
    let mut result = None;
    for f in frames {
        let decoded = decode_frame(&f.wire, from_client, maximum)
            .map_err(|_| StoreError::Invalid)?
            .ok_or(StoreError::Invalid)?;
        if let Some(message) = assembler
            .push(f.offset, f.wire, decoded)
            .map_err(|_| StoreError::Invalid)?
        {
            if result.is_some() {
                return Err(StoreError::Invalid);
            }
            result = Some(message);
        }
    }
    result.ok_or(StoreError::Invalid)
}

/// Query exact native run identities in the Flow actor's verified Gateway process only.
pub(super) fn resolve(
    conn: &Connection,
    actor: ProcessKey,
    child_run: &str,
    child_session: &str,
) -> Result<(GatewayEvidence, Vec<&'static str>), StoreError> {
    let announce = format!("announce:v1:{child_session}:{child_run}");
    let mut statement = conn.prepare(
        "SELECT f.message,f.fact FROM group_gateway_facts f
         JOIN group_gateway_messages m ON m.id=f.message
         JOIN group_gateway_connections c ON c.capture=m.connection
         JOIN process_keys p ON p.id=c.server
         WHERE p.boot_id=?1 AND p.host_tgid=?2 AND p.birth_boottime_ns=?3
           AND m.from_client=0 AND m.reason IS NULL
           AND ((f.kind IN ('accepted_spawn','child_terminal') AND f.run_id=?4)
                OR (f.kind='parent_publication' AND f.run_id=?5)) ORDER BY f.message",
    )?;
    let mut rows = statement.query(params![
        actor.boot_id.as_slice(),
        actor.host_tgid,
        actor.birth_boottime_ns.to_be_bytes().as_slice(),
        child_run,
        announce
    ])?;
    let mut evidence = GatewayEvidence::default();
    while let Some(row) = rows.next()? {
        let reference = reference(conn, row.get(0)?)?;
        match serde_json::from_str::<GatewayFact>(&row.get::<_, String>(1)?)? {
            GatewayFact::AcceptedSpawn(fact) => evidence
                .accepted_spawns
                .push(Observation { reference, fact }),
            GatewayFact::ChildTerminal(fact) => evidence
                .child_terminals
                .push(Observation { reference, fact }),
            GatewayFact::ParentPublication(fact) => evidence
                .parent_publications
                .push(Observation { reference, fact }),
        }
    }
    let mut gaps = Vec::new();
    let first_receipt = evidence.accepted_spawns.first().map(|o| &o.fact);
    let receipt_conflict = evidence.accepted_spawns.iter().any(|o| {
        o.fact.child_session_key != child_session
            || first_receipt.is_some_and(|first| !first.same_delegation(&o.fact))
    });
    if first_receipt.is_none() {
        gaps.push("gateway_receipt_unobserved");
    } else if receipt_conflict {
        gaps.push("gateway_receipt_conflict");
    }
    let terminal_matches = evidence
        .child_terminals
        .iter()
        .any(|o| o.fact.session_key == child_session);
    let terminal_conflict = evidence
        .child_terminals
        .iter()
        .enumerate()
        .any(|(index, o)| {
            o.fact.session_key != child_session
                || evidence.child_terminals[..index]
                    .iter()
                    .any(|other| o.fact.conflicts(&other.fact))
        });
    if !terminal_matches {
        gaps.push("child_terminal_unobserved");
    }
    if terminal_conflict {
        gaps.push("child_terminal_conflict");
    }
    if evidence.parent_publications.is_empty() {
        gaps.push("parent_publication_unobserved");
    } else if receipt_conflict || first_receipt.is_none() {
        gaps.push("parent_publication_unverified");
    } else if let Some(parent) = first_receipt {
        let publication_conflict =
            evidence
                .parent_publications
                .iter()
                .enumerate()
                .any(|(index, o)| {
                    o.fact.session_key != parent.parent_session_key
                        || o.fact
                            .agent_id
                            .as_ref()
                            .is_some_and(|agent| *agent != parent.parent_agent_id)
                        || evidence.parent_publications[..index]
                            .iter()
                            .any(|other| o.fact.conflicts(&other.fact))
                });
        if publication_conflict {
            gaps.push("parent_publication_conflict");
        }
    }
    Ok((evidence, gaps))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::schema;
    use serde_json::{Value, json};
    use tokio_tungstenite::tungstenite::{
        Bytes,
        protocol::frame::{
            Frame, FrameHeader,
            coding::{Data, OpCode},
        },
    };

    fn actor(pid: u32) -> ProcessKey {
        ProcessKey {
            boot_id: [1; 16],
            host_tgid: pid,
            birth_boottime_ns: 42,
        }
    }

    fn database() -> Result<Connection, StoreError> {
        let conn = Connection::open_in_memory()?;
        conn.pragma_update(None, "foreign_keys", true)?;
        schema::initialize(&conn)?;
        Ok(conn)
    }

    fn connection(
        conn: &Connection,
        capture: u8,
        server: ProcessKey,
    ) -> Result<GatewayConnection, StoreError> {
        let connection = GatewayConnection {
            capture: [capture; 32],
            source: [3; 16],
            generation: 2,
            first_syn_ordinal: u64::from(capture),
            client_port: 40200 + u16::from(capture),
            server_port: 18789,
            client: actor(10),
            server,
            client_socket_cookie: u64::MAX - u64::from(capture) * 2,
            server_socket_cookie: u64::MAX - u64::from(capture) * 2 + 1,
        };
        record_handshake(conn, connection.clone(),
            b"GET / HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n".to_vec(),
            b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n\r\n".to_vec())?;
        Ok(connection)
    }

    // Storage tests begin at the decoder's complete-message boundary; the real strict parser
    // supplies the derived fields and the locked WebSocket codec supplies the original wire.
    fn server_observation(
        conn: &Connection,
        connection: &GatewayConnection,
        offset: u64,
        event: Value,
    ) -> Result<Vec<u8>, StoreError> {
        let text = event.to_string();
        let mut wire = Vec::new();
        Frame::from_payload(
            FrameHeader {
                is_final: true,
                opcode: OpCode::Data(Data::Text),
                ..FrameHeader::default()
            },
            Bytes::copy_from_slice(text.as_bytes()),
        )
        .format(&mut wire)
        .expect("locked codec encoded the original test frame");
        let (sender, receiver) = expected_actors(conn, &connection.capture, false)?;
        let message = write_frames(
            conn,
            connection.capture,
            false,
            Some(sender),
            Some(receiver),
            vec![WireFrame {
                offset,
                wire: wire.clone(),
            }],
            None,
        )?;
        let reference = reference(conn, message)?;
        let value = as_types::json::parse(text.as_bytes())?;
        let doc = crate::gateway::GatewayDocument {
            connection,
            reference: &reference,
            value: &value,
        };
        if let Some(fact) = crate::agent_graph::gateway::project(&doc) {
            index_fact(conn, message, &fact)?;
        }
        Ok(wire)
    }

    fn receipt(parent: &str, session: &str) -> Value {
        json!({"type":"event","event":"agent","payload":{
            "runId":parent,"sessionKey":session,"agentId":"main","seq":9,"stream":"tool",
            "data":{"name":"sessions_spawn","phase":"result","toolCallId":"call|item","isError":false,
                "result":{"details":{"status":"accepted","runId":"child","childSessionKey":"child-session"}}}}})
    }

    fn terminal(status: &str) -> Value {
        json!({"type":"event","event":"sessions.changed","payload":{
            "session":{"key":"child-session","lastRunId":"child","status":status,
                "agentId":"helper","sessionId":"child-session-instance"}}})
    }

    fn publication() -> Value {
        json!({"type":"event","event":"chat","payload":{
            "runId":"announce:v1:child-session:child","sessionKey":"parent-session","agentId":"main",
            "seq":7,"state":"final"}})
    }

    #[test]
    fn socket_identity_is_required_and_kept_with_the_original_handshake_transaction()
    -> Result<(), StoreError> {
        let mut db = database()?;
        {
            let tx = db.transaction()?;
            let original = connection(&tx, 1, actor(20))?;
            let cookies = tx.query_row(
                "SELECT client_cookie,server_cookie FROM group_gateway_socket_identity WHERE capture=?1",
                [original.capture.as_slice()],
                |row| Ok((row.get::<_, [u8; 8]>(0)?, row.get::<_, [u8; 8]>(1)?)),
            )?;
            assert_eq!(u64::from_be_bytes(cookies.0), original.client_socket_cookie);
            assert_eq!(u64::from_be_bytes(cookies.1), original.server_socket_cookie);
            let mut serialized = serde_json::to_value(&original)?;
            assert_eq!(
                serialized["client_socket_cookie"],
                original.client_socket_cookie.to_string()
            );
            assert_eq!(
                serialized["server_socket_cookie"],
                original.server_socket_cookie.to_string()
            );
            serialized
                .as_object_mut()
                .ok_or(StoreError::Invalid)?
                .remove("client_socket_cookie");
            assert!(serde_json::from_value::<GatewayConnection>(serialized).is_err());
            for client_missing in [true, false] {
                let mut missing = original.clone();
                missing.capture = [2; 32];
                if client_missing {
                    missing.client_socket_cookie = 0;
                } else {
                    missing.server_socket_cookie = 0;
                }
                assert!(matches!(
                    record_handshake(&tx, missing, vec![1], vec![2]),
                    Err(StoreError::Invalid)
                ));
            }
            assert_eq!(
                tx.query_row(
                    "SELECT COUNT(*) FROM group_gateway_socket_identity",
                    [],
                    |row| row.get::<_, u32>(0)
                )?,
                1
            );
            assert_eq!(
                tx.query_row("SELECT COUNT(*) FROM evidence_sets", [], |row| row
                    .get::<_, u32>(0))?,
                2
            );
            // Neither the original handshake nor its identity pair survives a rolled-back intake.
        }
        for table in [
            "group_gateway_connections",
            "group_gateway_socket_identity",
            "evidence_sets",
        ] {
            assert_eq!(
                db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row
                    .get::<_, u32>(0))?,
                0
            );
        }
        Ok(())
    }

    #[test]
    fn repeated_connections_keep_refs_without_completing_the_child_from_announce()
    -> Result<(), StoreError> {
        let db = database()?;
        let server = actor(20);
        for capture in [1, 2] {
            let connection = connection(&db, capture, server)?;
            server_observation(&db, &connection, 0, receipt("parent", "parent-session"))?;
            server_observation(&db, &connection, 1000, publication())?;
            server_observation(
                &db,
                &connection,
                2000,
                json!({"type":"event","event":"agent","payload":{
                "runId":"announce:v1:child-session:child","sessionKey":"parent-session","stream":"lifecycle",
                "data":{"phase":"end","executionSettled":true}}}),
            )?;
        }
        let (evidence, gaps) = resolve(&db, server, "child", "child-session")?;
        assert_eq!(evidence.accepted_spawns.len(), 2);
        assert_eq!(evidence.parent_publications.len(), 2);
        assert!(evidence.child_terminals.is_empty());
        assert_eq!(gaps, ["child_terminal_unobserved"]);
        let mut wrong_lifetime = server;
        wrong_lifetime.birth_boottime_ns += 1;
        let wrong = connection(&db, 3, wrong_lifetime)?;
        server_observation(&db, &wrong, 0, terminal("done"))?;
        let (_, gaps) = resolve(&db, server, "child", "child-session")?;
        assert_eq!(gaps, ["child_terminal_unobserved"]);
        Ok(())
    }

    #[test]
    fn native_parent_and_terminal_conflicts_keep_all_observations_without_fallback()
    -> Result<(), StoreError> {
        let db = database()?;
        let server = actor(20);
        let connection = connection(&db, 1, server)?;
        server_observation(&db, &connection, 0, receipt("parent", "parent-session"))?;
        server_observation(&db, &connection, 1000, terminal("done"))?;
        server_observation(&db, &connection, 2000, publication())?;
        assert!(resolve(&db, server, "child", "child-session")?.1.is_empty());
        server_observation(
            &db,
            &connection,
            3000,
            receipt("other-parent", "parent-session"),
        )?;
        server_observation(&db, &connection, 4000, terminal("failed"))?;
        let (evidence, gaps) = resolve(&db, server, "child", "child-session")?;
        assert_eq!(evidence.accepted_spawns.len(), 2);
        assert_eq!(evidence.child_terminals.len(), 2);
        assert_eq!(evidence.parent_publications.len(), 1);
        assert_eq!(
            gaps,
            [
                "gateway_receipt_conflict",
                "child_terminal_conflict",
                "parent_publication_unverified"
            ]
        );
        assert_eq!(evidence.accepted_spawns[0].fact.tool_call_id, "call|item");
        Ok(())
    }

    #[test]
    fn exact_terminal_fields_and_raw_frame_have_one_original_storage_copy() -> Result<(), StoreError>
    {
        let db = database()?;
        let server = actor(20);
        let connection = connection(&db, 1, server)?;
        let original = server_observation(
            &db,
            &connection,
            23,
            json!({"type":"event","event":"agent","payload":{
            "runId":"child","sessionKey":"child-session","stream":"lifecycle","seq":12,
            "data":{"phase":"error","executionSettled":true,"status":null,"aborted":false,
                "outcome":{"status":"failed","observed":["",0,false]},"stopReason":""}}}),
        )?;
        let (evidence, gaps) = resolve(&db, server, "child", "child-session")?;
        let fact = &evidence.child_terminals[0].fact;
        assert_eq!(fact.status, Some(Value::Null));
        assert_eq!(fact.aborted, Some(json!(false)));
        assert_eq!(fact.stop_reason, Some(json!("")));
        assert_eq!(
            fact.outcome,
            Some(json!({"status":"failed","observed":["",0,false]}))
        );
        assert!(!gaps.contains(&"child_terminal_unobserved"));
        let wire_id = db.query_row("SELECT wire FROM group_gateway_frames", [], |r| {
            r.get::<_, i64>(0)
        })?;
        assert_eq!(
            parts::read_evidence(&db, wire_id)?,
            EvidenceBody::from_bytes(original.clone())
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM evidence_sets", [], |r| r
                .get::<_, u32>(0))?,
            3
        );
        assert_eq!(evidence.child_terminals[0].reference.frames[0].offset, 23);
        assert_eq!(
            evidence.child_terminals[0].reference.frames[0].length,
            original.len() as u64
        );
        Ok(())
    }

    #[test]
    fn gaps_preserve_overlapping_original_fragments_without_claiming_actors_or_facts()
    -> Result<(), StoreError> {
        let db = database()?;
        let server = actor(20);
        let connection = connection(&db, 1, server)?;
        record_gap(
            &db,
            connection.capture,
            false,
            vec![
                WireFrame {
                    offset: 50,
                    wire: vec![1, 2, 3],
                },
                WireFrame {
                    offset: 40,
                    wire: vec![4, 5],
                },
                WireFrame {
                    offset: 50,
                    wire: vec![6],
                },
            ],
            "ownership_unverified",
        )?;
        assert!(db.query_row(
            "SELECT sender IS NULL AND receiver IS NULL FROM group_gateway_messages",
            [],
            |r| r.get::<_, bool>(0)
        )?);
        let bodies = db
            .prepare("SELECT wire FROM group_gateway_frames ORDER BY ordinal")?
            .query_map([], |r| r.get::<_, i64>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(bodies.len(), 3);
        for (id, original) in bodies.into_iter().zip([vec![1, 2, 3], vec![4, 5], vec![6]]) {
            assert_eq!(
                parts::read_evidence(&db, id)?,
                EvidenceBody::from_bytes(original)
            );
        }
        record_unverified(
            &db,
            UnverifiedGatewaySource {
                source: [3; 16],
                generation: 2,
                first_syn_ordinal: 90,
                client_port: 40201,
                server_port: 18789,
            },
            false,
            vec![WireFrame {
                offset: 0,
                wire: b"HTTP/1.1 101".to_vec(),
            }],
            "upgrade_incomplete",
        )?;
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM group_unverified_streams", [], |r| r
                .get::<_, u32>(
                0
            ))?,
            1
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM group_gateway_facts", [], |r| r
                .get::<_, u32>(0))?,
            0
        );
        Ok(())
    }
}
