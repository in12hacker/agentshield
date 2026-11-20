//! Explicit software supply at the verified-message interface; never a native collection receipt.
use super::*;
use crate::{
    agent_graph::{Intake, websocket::WireFrame},
    business_log::sessions::*,
    gateway::GatewayConnection,
};
use as_types::{flow::*, process_tree::ProcessKey};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::{
    Bytes,
    protocol::frame::{
        Frame, FrameHeader,
        coding::{Data, OpCode},
    },
};

fn process(pid: u32) -> ProcessKey {
    ProcessKey {
        boot_id: [1; 16],
        host_tgid: pid,
        birth_boottime_ns: 42,
    }
}
async fn connect(
    log: &BusinessLog,
    capture: u8,
    server: u32,
) -> Result<GatewayConnection, StoreError> {
    let c = GatewayConnection {
        capture: [capture; 32],
        source: [2; 16],
        generation: 1,
        first_syn_ordinal: u64::from(capture),
        client_port: 40000 + u16::from(capture),
        server_port: 18789,
        client: process(10),
        server: process(server),
        client_socket_cookie: 10,
        server_socket_cookie: 20,
    };
    log.record_group(Intake::GatewayHandshake(
        Box::new(c.clone()),
        vec![1],
        vec![2],
    ))
    .await?;
    Ok(c)
}
async fn supply(
    log: &BusinessLog,
    c: &GatewayConnection,
    client: bool,
    text: &str,
) -> Result<(), StoreError> {
    let mut wire = Vec::new();
    Frame::from_payload(
        FrameHeader {
            is_final: true,
            opcode: OpCode::Data(Data::Text),
            mask: client.then_some([1, 2, 3, 4]),
            ..FrameHeader::default()
        },
        Bytes::copy_from_slice(text.as_bytes()),
    )
    .format(&mut wire)
    .map_err(|_| StoreError::Invalid)?;
    let message = group_gateway::assemble(vec![WireFrame { offset: 100, wire }], client)?;
    log.record_group(Intake::GatewayMessage {
        connection: c.capture,
        from_client: client,
        sender: if client { c.client } else { c.server },
        receiver: if client { c.server } else { c.client },
        message,
    })
    .await
}
async fn input(
    log: &BusinessLog,
    c: &GatewayConnection,
    client: bool,
    v: Value,
) -> Result<(), StoreError> {
    supply(log, c, client, &v.to_string()).await
}
fn scope(session: ChatSessionRef) -> ChatMessageScope {
    ChatMessageScope::NativeGateway {
        session: Some(session),
    }
}
fn observations(entry: &ChatMessageEntry) -> &[GatewayChatObservation] {
    let ChatMessageEntry::NativeGateway { observations, .. } = entry else {
        panic!("native entry");
    };
    observations
}
fn event(session: &str, run: &str, id: &str, seq: u64, text: &str) -> Value {
    json!({"type":"event","event":"chat","payload":{"sessionId":session,"sessionKey":"alias","runId":run,"state":"final",
        "message":{"role":"assistant","content":text,"__openclaw":{"id":id,"seq":seq}}}})
}

#[tokio::test]
async fn first_input_survives_late_handoff_reconnect_concurrency_and_rpc_conflict()
-> Result<(), StoreError> {
    let dir = tempfile::tempdir()?;
    let log = BusinessLog::open_path(&dir.path().join("business.sqlite"))?;
    let a = connect(&log, 1, 20).await?;
    let b = connect(&log, 2, 20).await?;
    let other = connect(&log, 3, 21).await?;
    let req = json!({"type":"req","id":"same-rpc","method":"sessions.create","params":{"key":"alias","agentId":"agent-original","message":""}});
    input(&log, &a, true, req.clone()).await?;
    let (unpaired, _) = log
        .chat_messages(ChatMessageScope::NativeGateway { session: None }, None, 64)
        .await?;
    assert_eq!(
        observations(&unpaired[0])[0].content,
        Some(ChatContent::Text(String::new()))
    );
    assert!(matches!(
        observations(&unpaired[0])[0].role,
        Some(GatewayChatRole::ProtocolInput {
            value: ChatRole::User,
            ..
        })
    ));
    assert!(observations(&unpaired[0])[0].handoff.is_none());
    // The same RPC ID on another connection is unrelated, even with one request on each.
    input(&log,&b,true,json!({"type":"req","id":"same-rpc","method":"sessions.create","params":{"key":"alias","message":"B"}})).await?;
    input(&log,&b,false,json!({"type":"res","id":"same-rpc","ok":true,"payload":{"ok":true,"sessionId":"B","key":"alias","runId":"run-b"}})).await?;
    input(&log,&a,false,json!({"type":"res","id":"same-rpc","ok":true,"payload":{"ok":true,"sessionId":"A","key":"alias","runId":"run-a"}})).await?;
    input(&log, &b, false, event("A", "run-a", "m", 7, "reconnect")).await?;
    input(
        &log,
        &other,
        false,
        event("A", "run-a", "m", 7, "other issuer"),
    )
    .await?;
    let (sessions, _) = log.chat_sessions(None, 64).await?;
    assert_eq!(sessions.len(), 3);
    let a_session=sessions.iter().find(|s|matches!(s,ChatSession::NativeGateway {namespace,session,..} if namespace.server==a.server && session.session_id.as_deref()==Some("A"))).expect("A").reference().clone();
    let (page, _) = log
        .chat_messages(scope(a_session.clone()), None, 64)
        .await?;
    assert_eq!(page.len(), 2);
    let first = page
        .iter()
        .flat_map(observations)
        .find(|o| o.reference.pointer == "/params/message")
        .expect("original first input");
    assert_eq!(first.run_id, None);
    assert_eq!(first.content, Some(ChatContent::Text(String::new())));
    assert_eq!(
        first.handoff.as_ref().and_then(|h| h.run_id.as_deref()),
        Some("run-a")
    );
    assert_eq!(
        first.handoff.as_ref().and_then(|h| h.agent_id.as_deref()),
        None
    );
    assert_eq!(
        log.chat_exchanges(Some(a_session.clone()), None, 64)
            .await?
            .0,
        Vec::<ExchangeKey>::new()
    );
    input(&log, &a, true, req).await?; // Competing original request invalidates only its former handoff.
    let (page, _) = log.chat_messages(scope(a_session), None, 64).await?;
    assert_eq!(page.len(), 1);
    let (unpaired, _) = log
        .chat_messages(ChatMessageScope::NativeGateway { session: None }, None, 64)
        .await?;
    assert_eq!(unpaired.len(), 2);
    assert!(unpaired.iter().all(|e|matches!(e,ChatMessageEntry::NativeGateway {gaps,..} if gaps.iter().any(|g|g.reason==SessionGapReason::RpcConflict))));
    log.close().await
}

#[tokio::test]
async fn full_delta_identity_order_and_cutoff_use_original_fields_without_rewriting_content()
-> Result<(), StoreError> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("business.sqlite");
    let log = BusinessLog::open_path(&path)?;
    let c = connect(&log, 1, 20).await?;
    input(
        &log,
        &c,
        true,
        json!({"type":"req","id":"full","method":"chat.startup","params":{"sessionKey":"alias"}}),
    )
    .await?;
    input(&log,&c,false,json!({"type":"res","id":"full","ok":true,"payload":{"sessionId":"A","sessionKey":"alias","deltaCursor":{"n":0},"messages":[
        {"role":"toolResult","content":["",null,0,false],"__openclaw":{"id":"old","seq":2,"recordTimestampMs":0,"transcriptPosition":null}},
        {"role":"assistant","content":"later","__openclaw":{"id":"later","seq":9}}
    ]}})).await?;
    let session = log.chat_sessions(None, 64).await?.0[0].reference().clone();
    let (page, cursor) = log.chat_messages(scope(session.clone()), None, 1).await?;
    assert_eq!(
        observations(&page[0])[0].transcript_position,
        Some(Value::Null)
    );
    assert_eq!(
        observations(&page[0])[0].record_timestamp_ms,
        Some(json!(0))
    );
    assert!(
        matches!(&observations(&page[0])[0].role,Some(GatewayChatRole::String {value:ChatRole::Tool,original,..}) if original=="toolResult")
    );
    input(
        &log,
        &c,
        true,
        json!({"type":"req","id":"delta","method":"chat.history","params":{"cursor":{"n":0}}}),
    )
    .await?;
    input(&log,&c,false,json!({"type":"res","id":"delta","ok":true,"payload":{"kind":"delta","messages":[
        {"sessionId":"A","sessionKey":"alias","messageId":"middle","messageSeq":5,"message":{"role":"assistant","content":"middle","__openclaw":{"id":"middle","seq":5,"runId":"original-message-run"}}},
        {"sessionId":"A","sessionKey":"alias","messageId":"old","messageSeq":2,"message":{"role":"toolResult","content":["",null,0,false],"__openclaw":{"id":"old","seq":2}}}
    ]}})).await?;
    let (continued, next) = log
        .chat_messages(scope(session.clone()), cursor, 64)
        .await?;
    assert_eq!(continued.len(), 1);
    assert_eq!(next, None);
    assert_eq!(
        observations(&continued[0])[0].content,
        Some(ChatContent::Text("later".into()))
    );
    let (fresh, _) = log.chat_messages(scope(session.clone()), None, 64).await?;
    assert_eq!(fresh.len(), 3);
    assert_eq!(observations(&fresh[0]).len(), 2);
    assert_eq!(
        observations(&fresh[1])[0].message_run_id.as_deref(),
        Some("original-message-run")
    );
    assert!(observations(&fresh[1])[0].wrapper.is_some());
    assert_eq!(observations(&fresh[1])[0].run_id, None);
    let db = Connection::open(&path)?;
    assert_eq!(
        db.query_row("SELECT count(*) FROM group_gateway_frames", [], |r| r
            .get::<_, i64>(0))?,
        4
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM evidence_sets", [], |r| r
            .get::<_, i64>(0))?,
        6
    );
    log.close().await?;
    let reopened = BusinessLog::open_path(&path)?;
    assert_eq!(
        reopened
            .chat_messages(scope(session), None, 64)
            .await?
            .0
            .len(),
        3
    );
    reopened.close().await
}

#[tokio::test]
#[ignore = "explicit authorized ordinary-client originals; software supply only"]
async fn authorized_native_originals_software_supply() -> Result<(), StoreError> {
    let path = std::env::var_os("AS_SESSION_SOFTWARE_ORIGINALS").ok_or(StoreError::NotProvided)?;
    let originals = as_types::json::parse(&std::fs::read(path)?)?;
    let dir = tempfile::tempdir()?;
    let log = BusinessLog::open_path(&dir.path().join("business.sqlite"))?;
    let mut connections = std::collections::BTreeMap::new();
    for item in originals.as_array().ok_or(StoreError::Invalid)? {
        let key = item["connection"].as_str().ok_or(StoreError::Invalid)?;
        if !connections.contains_key(key) {
            let c = connect(&log, (connections.len() + 1) as u8, 20).await?;
            connections.insert(key.to_owned(), c);
        }
        let c = connections.get(key).ok_or(StoreError::Invalid)?;
        input(
            &log,
            c,
            item["direction"] == "out",
            item["original"].clone(),
        )
        .await?;
    }
    let (sessions, _) = log.chat_sessions(None, 64).await?;
    assert!(!sessions.is_empty());
    let mut output = Vec::new();
    let mut chats = 0;
    for session in &sessions {
        let (messages, next) = log
            .chat_messages(scope(session.reference().clone()), None, 64)
            .await?;
        assert_eq!(next, None);
        chats += messages.len();
        output.push(json!({"session":session,"page":{"type":"chat_messages","messages":messages,"next_after":next}}));
    }
    assert!(chats > 0);
    let unbound = log
        .chat_messages(ChatMessageScope::NativeGateway { session: None }, None, 64)
        .await?
        .0;
    eprintln!(
        "SESSION_SOFTWARE_SUPPLY sessions={} message_items={} unbound_items={}",
        sessions.len(),
        chats,
        unbound.len()
    );
    if let Some(path) = std::env::var_os("AS_SESSION_SOFTWARE_REPLY") {
        std::fs::write(path, serde_json::to_vec(&output)?)?;
    }
    log.close().await
}

#[tokio::test]
async fn history_cursor_handoff_chains_and_reset_preserve_the_session_and_unbound_inputs()
-> Result<(), StoreError> {
    let dir = tempfile::tempdir()?;
    let log = BusinessLog::open_path(&dir.path().join("business.sqlite"))?;
    let c = connect(&log, 1, 20).await?;
    // Reverse arrival still pairs only this original RPC ID on this connection.
    input(&log,&c,false,json!({"type":"res","id":"full","ok":true,"payload":{"sessionId":"A","deltaCursor":"c0","messages":[]}})).await?;
    input(
        &log,
        &c,
        true,
        json!({"type":"req","id":"full","method":"chat.startup","params":{"sessionKey":"alias"}}),
    )
    .await?;
    let session = log.chat_sessions(None, 64).await?.0[0].reference().clone();
    for (id, cursor, next, kind, seq) in [
        ("d1", "c0", "c1", "delta", 1),
        ("d2", "c1", "c2", "delta", 2),
        ("reset", "c2", "reset", "reset", 0),
        ("after-reset", "c2", "c3", "delta", 3),
    ] {
        input(
            &log,
            &c,
            true,
            json!({"type":"req","id":id,"method":"chat.history","params":{"cursor":cursor}}),
        )
        .await?;
        input(&log,&c,false,json!({"type":"res","id":id,"ok":true,"payload":{"kind":kind,"deltaCursor":next,"messages":if kind=="reset" {json!([])}else{json!([
            {"messageId":id,"messageSeq":seq,"message":{"role":"assistant","content":id,"__openclaw":{"id":id,"seq":seq}}}
        ])}}})).await?;
    }
    assert_eq!(log.chat_sessions(None, 64).await?.0.len(), 1);
    assert_eq!(
        log.chat_messages(scope(session.clone()), None, 64)
            .await?
            .0
            .len(),
        2
    );
    let unbound = log
        .chat_messages(ChatMessageScope::NativeGateway { session: None }, None, 64)
        .await?
        .0;
    assert_eq!(unbound.len(), 1);
    assert_eq!(
        observations(&unbound[0])[0].content,
        Some(ChatContent::Text("after-reset".into()))
    );
    input(&log,&c,true,json!({"type":"req","id":"failed","method":"sessions.create","params":{"key":"alias","message":"failed original"}})).await?;
    input(
        &log,
        &c,
        false,
        json!({"type":"res","id":"failed","ok":false,"error":{"message":"original failure"}}),
    )
    .await?;
    let unbound = log
        .chat_messages(ChatMessageScope::NativeGateway { session: None }, None, 64)
        .await?
        .0;
    assert!(unbound.iter().any(|e|matches!(e,ChatMessageEntry::NativeGateway {gaps,..} if gaps.iter().any(|g|g.reason==SessionGapReason::RpcFailed))));
    // A late competing full request revokes its window/cursor proof instead of choosing a candidate.
    input(
        &log,
        &c,
        true,
        json!({"type":"req","id":"full","method":"chat.startup","params":{}}),
    )
    .await?;
    assert!(
        log.chat_messages(scope(session), None, 64)
            .await?
            .0
            .is_empty()
    );
    log.close().await
}

#[tokio::test]
async fn native_numbers_and_unsupported_fields_keep_types_and_original_positions()
-> Result<(), StoreError> {
    let dir = tempfile::tempdir()?;
    let log = BusinessLog::open_path(&dir.path().join("business.sqlite"))?;
    let c = connect(&log, 1, 20).await?;
    supply(&log,&c,false,r#"{"type":"event","event":"chat","payload":{"sessionId":"A","state":false,"message":{"role":null,"content":{"n":18446744073709551616,"e":1.2300e+400,"empty":["",null,0,false]},"__openclaw":{"id":18446744073709551616,"seq":18446744073709551615,"recordTimestampMs":0,"transcriptPosition":{"source":"","rawSeq":0}}}}}"#).await?;
    let session = log.chat_sessions(None, 64).await?.0[0].reference().clone();
    let (page, _) = log.chat_messages(scope(session), None, 64).await?;
    let o = &observations(&page[0])[0];
    assert_eq!(
        o.message_sequence,
        Some(NativeIdValue::JsonNumber(u64::MAX.to_string()))
    );
    assert_eq!(o.state, Some(json!(false)));
    assert!(matches!(
        &o.role,
        Some(GatewayChatRole::Json {
            value: Value::Null,
            ..
        })
    ));
    let serialized = serde_json::to_string(&page)?;
    assert!(serialized.contains("18446744073709551616"));
    assert!(serialized.contains("1.2300e+400"));
    input(&log,&c,false,json!({"type":"event","event":"chat","payload":{"message":{"role":"","content":{"sessionId":"not-a-session","runId":"not-a-run"}}}})).await?;
    assert_eq!(log.chat_sessions(None, 64).await?.0.len(), 1);
    let unbound = log
        .chat_messages(ChatMessageScope::NativeGateway { session: None }, None, 64)
        .await?
        .0;
    assert_eq!(unbound.len(), 1);
    assert!(
        matches!(&observations(&unbound[0])[0].role,Some(GatewayChatRole::String {value:ChatRole::Other(s),..}) if s.is_empty())
    );
    // Duplicate keys and non-text decode results keep raw transport identities, without projection.
    supply(&log,&c,false,r#"{"type":"event","event":"chat","payload":{"sessionId":"wrong","sessionId":"also-wrong"}}"#).await?;
    assert_eq!(log.chat_sessions(None, 64).await?.0.len(), 1);
    input(
        &log,
        &c,
        true,
        json!({"type":"req","id":"conflict-window","method":"chat.history","params":{}}),
    )
    .await?;
    input(&log,&c,false,json!({"type":"res","id":"conflict-window","ok":true,"payload":{"kind":"delta","messages":[
        {"sessionId":"B","session":{"sessionId":"C"},"messageId":"conflict","message":{"role":"assistant","content":null}}
    ]}})).await?;
    assert_eq!(log.chat_sessions(None, 64).await?.0.len(), 1);
    log.close().await
}

#[tokio::test]
async fn late_membership_keeps_the_existing_message_anchor_and_late_order_conflicts()
-> Result<(), StoreError> {
    let dir = tempfile::tempdir()?;
    let log = BusinessLog::open_path(&dir.path().join("business.sqlite"))?;
    let c = connect(&log, 1, 20).await?;
    let mut early = event("A", "late-run", "same-message", 7, "body");
    early["payload"]
        .as_object_mut()
        .expect("payload")
        .remove("sessionId");
    input(&log, &c, false, early).await?;
    input(
        &log,
        &c,
        false,
        event("A", "first-run", "same-message", 7, "body"),
    )
    .await?;
    let session = log.chat_sessions(None, 64).await?.0[0].reference().clone();
    let before = log.chat_messages(scope(session.clone()), None, 64).await?.0;
    let ChatMessageEntry::NativeGateway {
        reference: anchor, ..
    } = &before[0]
    else {
        panic!("native");
    };
    input(&log,&c,false,json!({"type":"event","event":"agent","payload":{"sessionId":"A","sessionKey":"alias","runId":"late-run"}})).await?;
    let after = log.chat_messages(scope(session.clone()), None, 64).await?.0;
    assert!(
        matches!(&after[0],ChatMessageEntry::NativeGateway {reference,observations,..} if reference==anchor && observations.len()==2)
    );
    input(&log,&c,true,json!({"type":"req","id":"send","method":"chat.send","params":{"sessionKey":"alias","message":""}})).await?;
    input(
        &log,
        &c,
        false,
        json!({"type":"res","id":"send","ok":true,"payload":{"runId":"first-run"}}),
    )
    .await?;
    let sent = log.chat_messages(scope(session.clone()), None, 64).await?.0;
    let submitted = sent
        .iter()
        .flat_map(observations)
        .find(|o| o.reference.pointer == "/params/message")
        .expect("chat.send input");
    assert_eq!(submitted.content, Some(ChatContent::Text(String::new())));
    assert_eq!(
        submitted
            .handoff
            .as_ref()
            .and_then(|h| h.session_id.as_deref()),
        None
    );
    assert_eq!(
        submitted.handoff.as_ref().and_then(|h| h.run_id.as_deref()),
        Some("first-run")
    );
    input(
        &log,
        &c,
        false,
        event("A", "first-run", "same-message", 8, "body"),
    )
    .await?;
    let conflict = log.chat_messages(scope(session), None, 64).await?.0;
    assert!(
        matches!(&conflict[0],ChatMessageEntry::NativeGateway {order,gaps,..} if matches!(order.as_ref(),GatewayChatOrder::Unresolved {..}) && gaps.iter().any(|g|g.reason==SessionGapReason::MessageOrderConflict))
    );
    log.close().await
}

#[tokio::test]
async fn stable_session_identity_is_not_rejected_by_selectors_aliases_or_agent_metadata()
-> Result<(), StoreError> {
    let dir = tempfile::tempdir()?;
    let log = BusinessLog::open_path(&dir.path().join("business.sqlite"))?;
    let c = connect(&log, 1, 20).await?;
    input(&log,&c,true,json!({"type":"req","id":"create","method":"sessions.create","params":{"key":" GLOBAL ","agentId":"MAIN","message":""}})).await?;
    input(&log,&c,false,json!({"type":"res","id":"create","ok":true,"payload":{"ok":true,"sessionId":"A","key":"global","runId":"r","entry":{"sessionId":"A"}}})).await?;
    let session=log.chat_sessions(None,64).await?.0.into_iter().find(|s|matches!(s,ChatSession::NativeGateway {session,..} if session.session_id.as_deref()==Some("A"))).expect("success identifies A").reference().clone();
    let first = log.chat_messages(scope(session.clone()), None, 64).await?.0;
    assert_eq!(first.len(), 1);
    assert_eq!(
        observations(&first[0])[0].content,
        Some(ChatContent::Text(String::new()))
    );
    assert_eq!(
        observations(&first[0])[0]
            .handoff
            .as_ref()
            .and_then(|s| s.session_key.as_deref()),
        Some("global")
    );
    let selected=log.call(|conn|Ok(conn.query_row("SELECT session_key,agent_id FROM gateway_session_observations WHERE object_pointer='/params'",[],|row|Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?)))?))?.await?;
    assert_eq!(selected, (" GLOBAL ".to_owned(), "MAIN".to_owned()));
    let mut alias = event("A", "r", "event-message", 1, "event");
    alias["payload"]["sessionKey"] = json!("another-alias");
    alias["payload"]["agentId"] = json!("different-observed-agent-metadata");
    input(&log, &c, false, alias).await?;
    input(&log,&c,true,json!({"type":"req","id":"delta","method":"chat.history","params":{"sessionKey":"history-selector"}})).await?;
    input(&log,&c,false,json!({"type":"res","id":"delta","ok":true,"payload":{"kind":"delta","sessionId":"A","sessionKey":"history-selector","messages":[
        {"sessionId":"A","sessionKey":"effective-key","agentId":"effective-agent","session":{"sessionId":"A","key":"effective-key","agentId":"effective-agent"},"messageId":"history-message","messageSeq":2,"message":{"role":"assistant","content":""}}
    ]}})).await?;
    let page = log.chat_messages(scope(session), None, 64).await?.0;
    assert_eq!(page.len(), 3);
    assert!(page.iter().all(|item|matches!(item,ChatMessageEntry::NativeGateway {gaps,..} if !gaps.iter().any(|g|g.reason==SessionGapReason::IdentityConflict))));
    input(&log,&c,false,json!({"type":"event","event":"chat","payload":{"sessionKey":"global","message":{"role":"assistant","content":"key alone"}}})).await?;
    let unbound = log
        .chat_messages(ChatMessageScope::NativeGateway { session: None }, None, 64)
        .await?
        .0;
    assert_eq!(unbound.len(), 1);
    assert_eq!(
        observations(&unbound[0])[0].content,
        Some(ChatContent::Text("key alone".to_owned()))
    );
    log.close().await
}

#[tokio::test]
async fn same_public_object_contradictions_and_competing_run_session_ids_remain_conflicts()
-> Result<(), StoreError> {
    let dir = tempfile::tempdir()?;
    let log = BusinessLog::open_path(&dir.path().join("business.sqlite"))?;
    let c = connect(&log, 1, 20).await?;
    input(&log,&c,true,json!({"type":"req","id":"create","method":"sessions.create","params":{"message":"keep input"}})).await?;
    input(&log,&c,false,json!({"type":"res","id":"create","ok":true,"payload":{"ok":true,"sessionId":"A","entry":{"sessionId":"B"}}})).await?;
    assert!(log.chat_sessions(None, 64).await?.0.is_empty());
    let unbound = log
        .chat_messages(ChatMessageScope::NativeGateway { session: None }, None, 64)
        .await?
        .0;
    assert!(
        matches!(&unbound[0],ChatMessageEntry::NativeGateway {gaps,..} if gaps.iter().any(|g|g.reason==SessionGapReason::IdentityConflict))
    );
    input(
        &log,
        &c,
        false,
        event("A", "competing-run", "run-message", 1, "run evidence"),
    )
    .await?;
    let session = log.chat_sessions(None, 64).await?.0[0].reference().clone();
    input(
        &log,
        &c,
        false,
        json!({"type":"event","event":"agent","payload":{"sessionId":"B","runId":"competing-run"}}),
    )
    .await?;
    assert!(
        log.chat_messages(scope(session), None, 64)
            .await?
            .0
            .is_empty()
    );
    let unbound = log
        .chat_messages(ChatMessageScope::NativeGateway { session: None }, None, 64)
        .await?
        .0;
    let run_message = unbound
        .iter()
        .find(|item| {
            observations(item)
                .iter()
                .any(|o| o.content == Some(ChatContent::Text("run evidence".to_owned())))
        })
        .expect("conflicted run observation");
    assert!(
        matches!(run_message,ChatMessageEntry::NativeGateway {gaps,..} if gaps.iter().any(|g|g.reason==SessionGapReason::IdentityConflict && g.references.len()>=2))
    );
    input(
        &log,
        &c,
        true,
        json!({"type":"req","id":"delta","method":"chat.history","params":{}}),
    )
    .await?;
    input(&log,&c,false,json!({"type":"res","id":"delta","ok":true,"payload":{"kind":"delta","messages":[
        {"sessionId":"A","sessionKey":"first-effective-key","session":{"sessionId":"A","key":"contradictory-effective-key"},"messageId":"conflict","message":{"role":"assistant","content":null}}
    ]}})).await?;
    let unbound = log
        .chat_messages(ChatMessageScope::NativeGateway { session: None }, None, 64)
        .await?
        .0;
    assert!(unbound.iter().any(|item|matches!(item,ChatMessageEntry::NativeGateway {gaps,observations,..} if observations.iter().any(|o|o.content==Some(ChatContent::Json(Value::Null))) && gaps.iter().any(|g|g.reason==SessionGapReason::IdentityConflict))));
    log.close().await
}
