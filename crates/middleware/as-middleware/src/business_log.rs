//! One structured SQLite business log; queries do not install an in-memory history.
pub use crate::database::group::{GroupPage, GroupRecord};
pub use crate::database::query::BusinessLogCounts;
/// Source-specific Session/chat models and pure business projections; no database owner or I/O.
pub mod sessions;
pub use crate::database::{BusinessLog, RecordOutcome, StoreError, StoredReceipt};

use crate::{
    database::{self, group_gateway, sessions as sql},
    gateway::{self, GatewayDocument},
};
use as_types::process_tree::ProcessKey;

impl BusinessLog {
    /// Read source-specific chat with an immutable committed cutoff for native continuation pages.
    pub async fn chat_messages(
        &self,
        scope: sessions::ChatMessageScope,
        after: Option<sessions::ChatMessageCursor>,
        limit: u16,
    ) -> Result<
        (
            Vec<sessions::ChatMessageEntry>,
            Option<sessions::ChatMessageCursor>,
        ),
        StoreError,
    > {
        use sessions::*;
        if !(1..=64).contains(&limit) {
            return Err(StoreError::Invalid);
        }
        let session = match scope {
            ChatMessageScope::ModelHttp { exchange } => {
                let after = match after {
                    Some(ChatMessageCursor::ModelHttp { message }) => Some(message),
                    None => None,
                    _ => return Err(StoreError::Invalid),
                };
                let (messages, next) = self.model_messages(exchange, after, limit).await?;
                return Ok((
                    messages
                        .into_iter()
                        .map(|message| ChatMessageEntry::ModelHttp { message })
                        .collect(),
                    next.map(|message| ChatMessageCursor::ModelHttp { message }),
                ));
            }
            ChatMessageScope::NativeGateway { session } => session,
        };
        if matches!(session, Some(ChatSessionRef::ModelHttp { .. })) {
            return Err(StoreError::Invalid);
        }
        let (groups, raw, next) = self
            .call(move |conn| {
                let tx = conn.transaction()?;
                let latest: i64 =
                    tx.query_row("SELECT coalesce(max(id),0) FROM deliveries", [], |r| {
                        r.get(0)
                    })?;
                let (after, cutoff) = match after {
                    Some(ChatMessageCursor::NativeGateway {
                        reference: reference @ ChatMessageRef::NativeGateway { .. },
                        read_through,
                    }) if read_through > 0 && read_through <= latest as u64 => {
                        (Some(reference), read_through as i64)
                    }
                    None => (None, latest),
                    _ => return Err(StoreError::Invalid),
                };
                let anchor = session.as_ref().map(|s| sql::entry(&tx, s)).transpose()?;
                let mut groups = group_messages(sql::read_chat_indexes(&tx, anchor, cutoff)?);
                if let Some(after) = after {
                    let ChatMessageRef::NativeGateway { object } = &after else {
                        return Err(StoreError::Invalid);
                    };
                    sql::validate_reference(&tx, &object.message)?;
                    let index = groups
                        .iter()
                        .position(|g| g.reference == after)
                        .ok_or(StoreError::Invalid)?;
                    groups.drain(..=index);
                }
                let more = groups.len() > usize::from(limit);
                groups.truncate(usize::from(limit));
                let next =
                    groups
                        .last()
                        .filter(|_| more)
                        .map(|g| ChatMessageCursor::NativeGateway {
                            reference: g.reference.clone(),
                            read_through: cutoff as u64,
                        });
                let mut raw = std::collections::BTreeMap::new();
                for group in &groups {
                    for index in &group.indexes {
                        let m = index.reference.message.id as i64;
                        if let std::collections::btree_map::Entry::Vacant(entry) = raw.entry(m) {
                            entry.insert(sql::original_message(&tx, m)?);
                        }
                    }
                }
                let requests: Vec<_> = raw.values().filter_map(|r| r.request).collect();
                for request in requests {
                    if let std::collections::btree_map::Entry::Vacant(entry) = raw.entry(request) {
                        entry.insert(sql::original_message(&tx, request)?);
                    }
                }
                tx.commit()?;
                Ok((groups, raw, next))
            })?
            .await?;
        // All content reconstruction happens after releasing the short SQL read transaction.
        let mut documents = std::collections::BTreeMap::new();
        for (id, raw) in raw {
            let message = group_gateway::assemble(raw.frames, raw.reference.from_client)?;
            let value = gateway::decode_message(&message).map_err(|_| StoreError::Invalid)?;
            documents.insert(id, (raw.connection, raw.reference, value, raw.request));
        }
        let mut projections = std::collections::BTreeMap::new();
        for (id, (connection, reference, value, request)) in &documents {
            let doc = GatewayDocument {
                connection,
                reference,
                value,
            };
            let req =
                request
                    .and_then(|r| documents.get(&r))
                    .map(|(connection, reference, value, _)| GatewayDocument {
                        connection,
                        reference,
                        value,
                    });
            projections.insert(*id, project_gateway(&doc, req.as_ref()));
        }
        let messages = groups
            .into_iter()
            .map(|group| {
                let mut observations = Vec::new();
                for index in &group.indexes {
                    let p = projections
                        .get_mut(&(index.reference.message.id as i64))
                        .ok_or(StoreError::Invalid)?;
                    let i = p
                        .chats
                        .iter()
                        .position(|c| c.reference == index.reference)
                        .ok_or(StoreError::Invalid)?;
                    let chat = p.chats.remove(i);
                    let (connection, reference, value, _) = documents
                        .get(&(index.reference.message.id as i64))
                        .ok_or(StoreError::Invalid)?;
                    let doc = GatewayDocument {
                        connection,
                        reference,
                        value,
                    };
                    observations.push(materialize_chat(
                        &doc,
                        chat,
                        index.session.clone(),
                        index.handoff.clone(),
                    ));
                }
                Ok(assemble_entry(group, observations))
            })
            .collect::<Result<Vec<_>, StoreError>>()?;
        Ok((messages, next))
    }

    pub(crate) async fn record_gateway(
        &self,
        capture: [u8; 32],
        from_client: bool,
        sender: ProcessKey,
        receiver: ProcessKey,
        message: crate::agent_graph::websocket::Message,
    ) -> Result<(), StoreError> {
        // Decode once before the transaction. This admitted work retains both the tree and wire.
        let decoded = gateway::decode_message(&message);
        self.write_call(move |conn| {
            let tx = conn.transaction()?;
            let delivery = database::record(&tx, "native_gateway")?;
            let reference = group_gateway::record_message(
                &tx,
                capture,
                from_client,
                sender,
                receiver,
                message,
                decoded.as_ref().err().copied(),
            )?;
            tx.execute(
                "INSERT INTO gateway_message_deliveries VALUES(?1,?2)",
                rusqlite::params![reference.id as i64, delivery],
            )?;
            if let Ok(value) = decoded {
                let connection = group_gateway::connection(&tx, &capture)?;
                let doc = GatewayDocument {
                    connection: &connection,
                    reference: &reference,
                    value: &value,
                };
                if let Some(fact) = crate::agent_graph::gateway::project(&doc) {
                    group_gateway::index_fact(&tx, reference.id as i64, &fact)?;
                }
                if let Some(rpc) = doc.rpc() {
                    sql::save_rpc(&tx, &reference, &rpc)?;
                }
                let mut relevant = false;
                if doc.value.get("type").and_then(serde_json::Value::as_str) != Some("res") {
                    let p = sessions::project_gateway(&doc, None);
                    relevant = !p.chats.is_empty()
                        || p.sessions
                            .iter()
                            .any(|s| sessions::nonempty(&s.session_id).is_some());
                    sql::save_projection(&tx, &doc, &p, None)?;
                }
                let (requests, replies) = sql::rpc_peers(&tx, reference.id as i64)?;
                if let ([request], [reply]) = (requests.as_slice(), replies.as_slice()) {
                    // The current document is borrowed; only the already stored peer is decoded.
                    let peer = if *request == reference.id as i64 {
                        *reply
                    } else {
                        *request
                    };
                    let peer_ref = group_gateway::reference(&tx, peer)?;
                    let peer_message = group_gateway::assemble(
                        group_gateway::frames(&tx, peer)?,
                        peer_ref.from_client,
                    )?;
                    if let Ok(peer_value) = gateway::decode_message(&peer_message) {
                        let peer_doc = GatewayDocument {
                            connection: &connection,
                            reference: &peer_ref,
                            value: &peer_value,
                        };
                        let (req, res) = if from_client {
                            (&doc, &peer_doc)
                        } else {
                            (&peer_doc, &doc)
                        };
                        let projection = sessions::project_gateway(res, Some(req));
                        relevant |= !projection.chats.is_empty() || !projection.sessions.is_empty();
                        sql::save_projection(&tx, res, &projection, Some(req))?;
                        if let Some(cursor) = sessions::history_input_cursor(req, &projection) {
                            let candidates = sql::history_cursor_candidates(
                                &tx,
                                &connection,
                                res.reference.id as i64,
                                cursor,
                            )?;
                            let inherited = sessions::continue_history(candidates);
                            sql::link_history_sessions(&tx, res.reference.id as i64, &inherited)?;
                        }
                    }
                }
                if relevant || !requests.is_empty() || !replies.is_empty() {
                    let mut touched = requests;
                    touched.extend(replies);
                    touched.push(reference.id as i64);
                    touched.sort_unstable();
                    touched.dedup();
                    let mut candidates = sql::session_candidates(&tx, &connection)?;
                    // Fixed directory anchors use the first confirmed observation, never a capture/key map.
                    for i in 0..candidates.len() {
                        if !candidates[i].valid {
                            continue;
                        }
                        let Some(identity) =
                            sessions::nonempty(&candidates[i].observation.session_id)
                        else {
                            continue;
                        };
                        let existing = candidates
                            .iter()
                            .find(|s| {
                                s.entry.is_some()
                                    && sessions::nonempty(&s.observation.session_id)
                                        == Some(identity)
                            })
                            .and_then(|s| s.entry);
                        if existing.is_none() {
                            candidates[i].entry =
                                Some(sql::register_native(&tx, delivery, candidates[i].id)?);
                        }
                    }
                    let runs: Vec<_> = candidates
                        .iter()
                        .filter(|s| touched.contains(&(s.observation.reference.message.id as i64)))
                        .flat_map(|s| {
                            [
                                s.observation.run_id.clone(),
                                s.observation.last_run_id.clone(),
                            ]
                        })
                        .flatten()
                        .collect();
                    let chats = sql::chat_candidates(&tx, &connection, &touched, &runs)?;
                    let valid: Vec<_> = candidates
                        .iter()
                        .filter(|s| s.valid)
                        .map(|s| s.observation.clone())
                        .collect();
                    for chat in chats {
                        let result = sessions::resolve_membership(&chat, &valid);
                        let session = result
                            .proof
                            .as_ref()
                            .and_then(|p| {
                                candidates.iter().find(|s| {
                                    s.entry.is_some() && s.observation.session_id == p.session_id
                                })
                            })
                            .and_then(|s| s.entry);
                        let handoff = result
                            .handoff
                            .as_ref()
                            .map(|s| sql::session_row(&tx, &s.reference))
                            .transpose()?;
                        let native_id = if result
                            .gaps
                            .iter()
                            .any(|g| g.reason == sessions::SessionGapReason::IdentityConflict)
                        {
                            None
                        } else {
                            sessions::usable_id(&chat.native_id)
                                .or_else(|| sessions::usable_id(&chat.wrapper_id))
                        };
                        let anchor = match (session, native_id) {
                            (Some(session), Some(id)) => {
                                sql::message_anchor(&tx, session, id)?.unwrap_or(chat.id)
                            }
                            _ => chat.id,
                        };
                        sql::save_membership(
                            &tx,
                            chat.id,
                            delivery,
                            session,
                            handoff,
                            anchor,
                            &result.gaps,
                        )?;
                    }
                }
            }
            tx.commit()?;
            Ok(())
        })?
        .await
    }
}
