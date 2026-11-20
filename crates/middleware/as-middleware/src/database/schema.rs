//! Current fixed relational schema; no migration or legacy-format fallback.
use super::{
    StoreError,
    row::{self, Columns, Kind::*},
};
use rusqlite::Connection;
pub(super) const PROCESS_KEY: Columns = &[
    ("boot_id", Bytes),
    ("host_tgid", Int),
    ("birth_boottime_ns", U64),
];
pub(super) const NATIVE_TOOL: Columns = &[
    ("event_ref", Text),
    ("delivery", Ref("deliveries")),
    ("origin_handoff", Ref("native_request_handoffs")),
    ("origin_candidate", Int),
];
pub(super) const INFO: Columns = &[
    ("kind", Text),
    ("exec_cookie", U64),
    ("pid_namespace_inode", U64),
    ("net_namespace_inode", U64),
    ("user_namespace_inode", U64),
    ("mount_namespace_inode", U64),
    ("name", Bytes),
    ("executable", Bytes),
    ("cwd", Bytes),
    ("uid", Int),
    ("euid", Int),
    ("suid", Int),
    ("fsuid", Int),
    ("gid", Int),
    ("egid", Int),
    ("sgid", Int),
    ("fsgid", Int),
    ("parent_pid", Int),
    ("parent_birth_boottime_ns", U64),
    ("parent_name", Bytes),
    ("parent_executable", Bytes),
    ("argv_present", Bool),
    ("parent_argv_present", Bool),
    ("namespace_tgids_present", Bool),
    ("workload_kind", Text),
    ("workload_runtime", Text),
    ("workload_namespace", Text),
    ("workload_id", Text),
];
pub(super) const ACTOR: Columns = &[
    ("process", Ref("process_keys")),
    ("info", Ref("process_info")),
    ("tid", Int),
    ("thread_birth_boottime_ns", U64),
    ("cgroup_id", U64),
    ("observed_boottime_ns", U64),
];
pub(super) const CHANNEL: Columns = &[
    ("kind", Text),
    ("source_instance", Bytes),
    ("object_id", U64),
    ("generation", U64),
];
pub(super) const CREDENTIAL_ID: Columns = &[
    ("flow_instance", Bytes),
    ("exchange_sequence", U64),
    ("sequence", U64),
];
pub(super) const SOURCE: Columns = &[
    ("actor", Ref("actors")),
    ("channel", Ref("channels")),
    ("side", Text),
    ("request_traceparent", Text),
    ("profile", Text),
    ("origin", Text),
    ("route_label", Text),
    ("config_version", Bytes),
    ("observed_at_utc_ms", Int),
    ("actual_auth_scope", Bytes),
    ("credential_use", Ref("credential_ids")),
];
pub(super) const PROTOCOL: Columns = &[
    ("flow_instance", Bytes),
    ("exchange_sequence", U64),
    ("sequence", U64),
    ("source", Ref("protocol_sources")),
    ("unit", Text),
    ("event_name", Text),
    ("body", Ref("evidence_sets")),
];
pub(super) const MESSAGE: Columns = &[
    ("flow_instance", Bytes),
    ("exchange_sequence", U64),
    ("sequence", U64),
    ("origin", Text),
    ("first_delivery", Ref("deliveries")),
];
pub(super) const REVISION: Columns = &[
    ("message", Ref("messages")),
    ("revision", U64),
    ("source", Ref("protocol_sources")),
    ("role_kind", Text),
    ("role_value", Text),
    ("candidate_index", Int),
    ("model", Text),
    ("state", Text),
    ("provider_status", Text),
    ("finish_reason", Text),
    ("first_seen_boottime_ns", U64),
    ("last_seen_boottime_ns", U64),
    ("usage_present", Bool),
    ("parts", Ref("part_sets")),
    ("derived", Bool),
    ("delivery", Ref("deliveries")),
];
pub(super) const HEADER: Columns = &[
    ("kind", Text),
    ("stage", Text),
    ("operation", Int),
    ("source_instance", Bytes),
    ("sequence_domain", U64),
    ("sequence", U64),
    ("source_boottime_ns", U64),
    ("actor_tgid", Int),
    ("actor_tid", Int),
    ("actor_birth_boottime_ns", U64),
    ("actor_thread_birth_boottime_ns", U64),
    ("actor_cgroup_id", U64),
];
pub(super) const REQUEST_RETURN: Columns = &[
    ("event_ref", Text),
    ("operation", Text),
    ("decision_id", Bytes),
    ("wait_id", U64),
    ("returned_boottime_ns", U64),
    ("return_value", Int),
    ("delivery", Ref("deliveries")),
];
pub(super) const FUTEX_SELECTION: Columns = &[
    ("event_ref", Text),
    ("selected_boottime_ns", U64),
    ("waker_tgid", Int),
    ("waker_tid", Int),
    ("waker_birth_ns", U64),
    ("waiter_tgid", Int),
    ("waiter_tid", Int),
    ("waiter_birth_ns", U64),
    ("wait_id", U64),
    ("wait_boottime_ns", U64),
    ("delivery", Ref("deliveries")),
];
pub(super) const FILE: Columns = &[
    ("path", Bytes),
    ("new_path", Bytes),
    ("device_major", Int),
    ("device_minor", Int),
    ("inode", U64),
    ("open_flags", Int),
    ("access_mask", Int),
    ("rename_flags", Int),
    ("creation_mode", Int),
    ("map_requested_prot", U64),
    ("map_effective_prot", U64),
    ("map_flags", U64),
    ("truncate_size", U64),
    ("truncate_attributes", Int),
];
pub(super) const PROCESS: Columns = &[
    ("target_executable", Bytes),
    ("target_argv_present", Bool),
    ("child_pid", Int),
    ("child_birth_boottime_ns", U64),
    ("child_parent_pid", Int),
    ("child_parent_birth_boottime_ns", U64),
    ("child_cgroup_id", U64),
    ("child_process", Ref("process_info")),
    ("exit_code", Int),
];
pub(super) const NETWORK: Columns = &[
    ("socket_cookie", U64),
    ("socket_net_namespace_inode", U64),
    ("local_ip", Text),
    ("remote_ip", Text),
    ("local_port", Int),
    ("remote_port", Int),
    ("protocol", Int),
    ("target_domain", Bytes),
];
pub(super) const DNS_SNAPSHOT: Columns = &[
    ("network_payload", Ref("network_payloads")),
    ("requested_bytes", U64),
    ("attempted_bytes", Int),
    ("send_flags", Int),
    ("data", Bytes),
    ("analysis_status", Text),
    ("qtype", Int),
    ("qclass", Int),
    ("opcode", Int),
    ("question_count", Int),
    ("reason", Text),
];
pub(super) const MOUNT_EFFECT: Columns =
    &[("file_payload", Ref("file_payloads")), ("effect", Text)];
pub(super) const HIT: Columns = &[
    ("group_id", Text),
    ("group_number", Int),
    ("rule_id", Text),
    ("rule_number", Int),
    ("source_kind", Text),
    ("source_version", Bytes),
    ("log", Bool),
    ("deny_message", Text),
];
pub(super) const REPORT: Columns = &[
    ("reference", Text),
    ("event", Ref("events")),
    ("decision_id", Bytes),
    ("hit", Ref("rule_hits")),
    ("report_type", Text),
    ("proposal_type", Text),
    ("proposal_terminate", Text),
    ("proposal_reason", Text),
    ("outcome", Text),
    ("outcome_native_error", Int),
    ("termination_requested", Text),
    ("actor_signal", Text),
    ("actor_error", Int),
    ("parent_signal", Text),
    ("parent_error", Int),
    ("failure_code", Text),
    ("failure_message", Text),
    ("original_kind", Text),
];
pub(super) const QUESTION: Columns = &[
    ("decision_id", Bytes),
    ("event", Ref("events")),
    ("hit", Ref("rule_hits")),
    ("timeout_type", Text),
    ("timeout_terminate", Text),
    ("remaining_ms", U64),
];
fn check_columns<'a>(
    conn: &Connection,
    table: &str,
    expected: impl IntoIterator<Item = &'a str>,
) -> Result<(), StoreError> {
    if !conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name=?1)",
        [table],
        |row| row.get::<_, bool>(0),
    )? {
        return Err(StoreError::Invalid);
    }
    let mut columns = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let actual = columns
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    if actual.iter().map(String::as_str).ne(expected) {
        return Err(StoreError::Invalid);
    }
    Ok(())
}

pub(super) fn initialize(conn: &Connection) -> Result<(), StoreError> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version != 0 && version != 2 {
        return Err(StoreError::Invalid);
    }
    if version == 0
        && conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_schema)", [], |row| {
            row.get::<_, bool>(0)
        })?
    {
        return Err(StoreError::Invalid);
    }
    if version == 2 {
        for (table, expected) in [
            (
                "deliveries",
                vec!["id", "kind", "cursor_runtime", "cursor_sequence"],
            ),
            ("evidence_sets", vec!["id", "original_length"]),
            (
                "evidence_segments",
                vec![
                    "set_id", "ordinal", "kind", "offset", "length", "bytes", "reason",
                ],
            ),
            (
                "protocol_session_fields",
                vec!["owner", "kind", "field", "issue"],
            ),
            (
                "protocol_session_values",
                vec!["owner", "ordinal", "kind", "bytes"],
            ),
            (
                "group_gateway_connections",
                vec![
                    "capture",
                    "source",
                    "generation",
                    "first_syn_ordinal",
                    "client_port",
                    "server_port",
                    "client",
                    "server",
                    "request",
                    "reply",
                ],
            ),
            (
                "group_gateway_socket_identity",
                vec!["capture", "client_cookie", "server_cookie"],
            ),
            (
                "group_gateway_frames",
                vec![
                    "message",
                    "ordinal",
                    "connection",
                    "from_client",
                    "stream_offset",
                    "wire",
                ],
            ),
            (
                "group_gateway_facts",
                vec!["message", "kind", "run_id", "session_key", "fact"],
            ),
            ("gateway_message_deliveries", vec!["message", "delivery"]),
            (
                "gateway_rpc_observations",
                vec!["message", "capture", "kind", "rpc_id", "method", "ok"],
            ),
            (
                "gateway_session_observations",
                vec![
                    "id",
                    "message",
                    "object_pointer",
                    "session_id",
                    "session_key",
                    "run_id",
                    "last_run_id",
                    "agent_id",
                    "request",
                ],
            ),
            (
                "gateway_history_windows",
                vec![
                    "message",
                    "request",
                    "kind",
                    "session",
                    "messages_pointer",
                    "input_cursor_pointer",
                    "output_cursor_pointer",
                    "cursor_kind",
                    "cursor_value",
                    "input_cursor_value",
                ],
            ),
            ("gateway_history_sessions", vec!["window", "session"]),
            (
                "gateway_chat_observations",
                vec![
                    "id",
                    "message",
                    "object_pointer",
                    "wrapper_pointer",
                    "history_pointer",
                    "history_index",
                    "full_window",
                    "native_id_kind",
                    "native_id_value",
                    "wrapper_id_kind",
                    "wrapper_id_value",
                    "sequence_kind",
                    "sequence_value",
                    "wrapper_sequence_kind",
                    "wrapper_sequence_value",
                    "revision_kind",
                    "revision_value",
                    "run_id",
                    "message_run_id",
                ],
            ),
            ("gateway_chat_session_fields", vec!["chat", "session"]),
            (
                "gateway_projection_gaps",
                vec!["message", "object_pointer", "gaps"],
            ),
            (
                "chat_session_entries",
                vec!["id", "delivery", "kind", "protocol", "gateway_session"],
            ),
            (
                "gateway_chat_memberships",
                vec![
                    "chat",
                    "delivery",
                    "session",
                    "handoff",
                    "message_anchor",
                    "gaps",
                ],
            ),
        ] {
            check_columns(conn, table, expected)?;
        }
        let event_fact_tables: u32 = conn.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE type='table' AND name IN
             ('event_syscalls','event_stacks','event_stack_frames','event_actor_namespace_tids','native_request_returns','protocol_session_fields','protocol_session_values','gateway_chat_memberships','chat_session_entries')",
            [],
            |row| row.get(0),
        )?;
        if event_fact_tables != 9 {
            return Err(StoreError::Invalid);
        }
        check_columns(
            conn,
            "chat_inputs",
            ["delivery", "event_kind", "payload_row"],
        )?;
        check_columns(
            conn,
            "group_gateway_messages",
            [
                "id",
                "connection",
                "from_client",
                "sender",
                "receiver",
                "reason",
                "decode_issue",
            ],
        )?;
        for (table, expected) in [
            ("process_keys", PROCESS_KEY),
            ("protocol_records", PROTOCOL),
            ("operation_reports", REPORT),
            ("file_payloads", FILE),
            ("file_mount_effects", MOUNT_EFFECT),
            ("network_dns_snapshots", DNS_SNAPSHOT),
            ("protocol_sources", SOURCE),
            ("questions", QUESTION),
        ] {
            check_columns(
                conn,
                table,
                std::iter::once("id").chain(expected.iter().map(|(name, _)| *name)),
            )?;
        }
        // A current database is validated, never passed through installation DDL to repair it.
        return Ok(());
    }
    conn.execute_batch("BEGIN IMMEDIATE;
      CREATE TABLE IF NOT EXISTS deliveries(id INTEGER PRIMARY KEY AUTOINCREMENT,kind TEXT NOT NULL,cursor_runtime BLOB,cursor_sequence BLOB,UNIQUE(cursor_runtime,cursor_sequence));
      CREATE TABLE IF NOT EXISTS chat_inputs(delivery INTEGER PRIMARY KEY REFERENCES deliveries(id),event_kind TEXT NOT NULL CHECK(event_kind IN ('observation','started','finished','delta','credential_use','rule_feedback','exchange_failed','gap','status')),payload_row INTEGER NOT NULL CHECK(payload_row>0));
      CREATE TABLE IF NOT EXISTS evidence_sets(id INTEGER PRIMARY KEY,original_length BLOB CHECK(original_length IS NULL OR length(original_length)=8));
      CREATE TABLE IF NOT EXISTS evidence_segments(set_id INTEGER NOT NULL REFERENCES evidence_sets(id),ordinal INTEGER NOT NULL,kind TEXT NOT NULL,offset BLOB NOT NULL CHECK(length(offset)=8),length BLOB,bytes BLOB,reason TEXT,PRIMARY KEY(set_id,ordinal));
      CREATE TABLE IF NOT EXISTS part_sets(id INTEGER PRIMARY KEY);
      CREATE TABLE IF NOT EXISTS events(id INTEGER PRIMARY KEY,header INTEGER NOT NULL REFERENCES event_headers(id),process INTEGER NOT NULL REFERENCES process_info(id),payload_kind TEXT NOT NULL,payload INTEGER NOT NULL);
      CREATE TABLE IF NOT EXISTS event_cgroup_lineage(header INTEGER NOT NULL REFERENCES event_headers(id),ordinal INTEGER NOT NULL CHECK(ordinal>=0 AND ordinal<32),cgroup_id BLOB NOT NULL CHECK(length(cgroup_id)=8),PRIMARY KEY(header,ordinal));
      CREATE TABLE IF NOT EXISTS event_actor_namespace_tids(
        header INTEGER NOT NULL REFERENCES event_headers(id),
        ordinal INTEGER NOT NULL CHECK(ordinal>=0 AND ordinal<33),
        tid INTEGER NOT NULL CHECK(typeof(tid)='integer' AND tid BETWEEN 1 AND 4294967295),
        PRIMARY KEY(header,ordinal)
      );
      CREATE TABLE IF NOT EXISTS event_boot_scopes(event INTEGER PRIMARY KEY REFERENCES events(id),boot_id BLOB NOT NULL CHECK(typeof(boot_id)='blob' AND length(boot_id)=16));
      CREATE TABLE IF NOT EXISTS event_syscalls(
        event INTEGER PRIMARY KEY REFERENCES events(id),
        audit_arch INTEGER NOT NULL CHECK(audit_arch BETWEEN 0 AND 4294967295),
        number INTEGER NOT NULL CHECK(number BETWEEN -2147483648 AND 2147483647),
        instruction_pointer BLOB NOT NULL CHECK(typeof(instruction_pointer)='blob' AND length(instruction_pointer)=8),
        stack_pointer BLOB NOT NULL CHECK(typeof(stack_pointer)='blob' AND length(stack_pointer)=8),
        arg0 BLOB NOT NULL CHECK(typeof(arg0)='blob' AND length(arg0)=8),
        arg1 BLOB NOT NULL CHECK(typeof(arg1)='blob' AND length(arg1)=8),
        arg2 BLOB NOT NULL CHECK(typeof(arg2)='blob' AND length(arg2)=8),
        arg3 BLOB NOT NULL CHECK(typeof(arg3)='blob' AND length(arg3)=8),
        arg4 BLOB NOT NULL CHECK(typeof(arg4)='blob' AND length(arg4)=8),
        arg5 BLOB NOT NULL CHECK(typeof(arg5)='blob' AND length(arg5)=8)
      );
      CREATE TABLE IF NOT EXISTS event_stacks(
        id INTEGER PRIMARY KEY,
        event INTEGER NOT NULL REFERENCES events(id),
        kind TEXT NOT NULL,
        buffer_full INTEGER NOT NULL CHECK(buffer_full IN (0,1)),
        result INTEGER,
        CHECK((kind='kernel' AND result IS NULL) OR (kind='user' AND typeof(result)='integer')),
        UNIQUE(event,kind)
      );
      CREATE TABLE IF NOT EXISTS event_stack_frames(
        stack INTEGER NOT NULL REFERENCES event_stacks(id),
        ordinal INTEGER NOT NULL CHECK(ordinal>=0 AND ordinal<32),
        instruction_pointer BLOB NOT NULL CHECK(typeof(instruction_pointer)='blob' AND length(instruction_pointer)=8),
        PRIMARY KEY(stack,ordinal)
      );
    ")?;
    let made = (|| {
        for (table, cols) in [
            ("process_keys", PROCESS_KEY),
            ("native_tool_executions", NATIVE_TOOL),
            ("process_info", INFO),
            ("actors", ACTOR),
            ("channels", CHANNEL),
            ("credential_ids", CREDENTIAL_ID),
            ("protocol_sources", SOURCE),
            ("protocol_records", PROTOCOL),
            ("messages", MESSAGE),
            ("message_revisions", REVISION),
            ("event_headers", HEADER),
            ("file_payloads", FILE),
            ("file_mount_effects", MOUNT_EFFECT),
            ("process_payloads", PROCESS),
            ("network_payloads", NETWORK),
            ("network_dns_snapshots", DNS_SNAPSHOT),
            ("rule_hits", HIT),
            ("operation_reports", REPORT),
            ("questions", QUESTION),
            ("native_request_returns", REQUEST_RETURN),
            ("native_futex_selections", FUTEX_SELECTION),
        ] {
            row::create(conn, table, cols)?;
        }
        conn.execute_batch("CREATE UNIQUE INDEX IF NOT EXISTS mount_effect_owner ON file_mount_effects(file_payload);
          CREATE UNIQUE INDEX IF NOT EXISTS dns_snapshot_owner ON network_dns_snapshots(network_payload);
          CREATE TABLE IF NOT EXISTS argv(info INTEGER NOT NULL,owner TEXT NOT NULL,ordinal INTEGER NOT NULL,bytes BLOB NOT NULL,PRIMARY KEY(owner,info,ordinal));
          CREATE TABLE IF NOT EXISTS namespace_tgids(info INTEGER NOT NULL REFERENCES process_info(id),ordinal INTEGER NOT NULL,value INTEGER NOT NULL,PRIMARY KEY(info,ordinal));
          CREATE TABLE IF NOT EXISTS parts(id INTEGER PRIMARY KEY,set_id INTEGER NOT NULL REFERENCES part_sets(id),ordinal INTEGER NOT NULL,part_id INTEGER NOT NULL,type TEXT NOT NULL,text_kind TEXT,text TEXT,name TEXT,content_present INTEGER NOT NULL,content_kind TEXT,content_value TEXT,reason TEXT,provider_type TEXT,media_kind TEXT,safe_label TEXT,UNIQUE(set_id,ordinal));
          CREATE TABLE IF NOT EXISTS protocol_native_ids(owner INTEGER NOT NULL REFERENCES protocol_records(id),ordinal INTEGER NOT NULL,namespace TEXT NOT NULL,field_path TEXT NOT NULL,value_kind TEXT NOT NULL,value TEXT NOT NULL,PRIMARY KEY(owner,ordinal));
          CREATE TABLE IF NOT EXISTS protocol_session_fields(owner INTEGER PRIMARY KEY REFERENCES protocol_records(id),kind TEXT NOT NULL,field TEXT NOT NULL,issue TEXT);
          CREATE TABLE IF NOT EXISTS protocol_session_values(owner INTEGER NOT NULL REFERENCES protocol_session_fields(owner),ordinal INTEGER NOT NULL,kind TEXT NOT NULL,bytes BLOB NOT NULL,PRIMARY KEY(owner,ordinal));
          CREATE INDEX IF NOT EXISTS protocol_session_value ON protocol_session_values(kind,bytes,owner);
          CREATE TABLE IF NOT EXISTS part_native_ids(owner INTEGER NOT NULL REFERENCES parts(id),ordinal INTEGER NOT NULL,namespace TEXT NOT NULL,field_path TEXT NOT NULL,value_kind TEXT NOT NULL,value TEXT NOT NULL,PRIMARY KEY(owner,ordinal));
          CREATE TABLE IF NOT EXISTS record_quality(owner INTEGER NOT NULL REFERENCES protocol_records(id),ordinal INTEGER NOT NULL,value TEXT NOT NULL,PRIMARY KEY(owner,ordinal));
          CREATE TABLE IF NOT EXISTS message_quality(owner INTEGER NOT NULL REFERENCES message_revisions(id),ordinal INTEGER NOT NULL,value TEXT NOT NULL,PRIMARY KEY(owner,ordinal));
          CREATE TABLE IF NOT EXISTS message_record_refs(owner INTEGER NOT NULL REFERENCES message_revisions(id),ordinal INTEGER NOT NULL,flow_instance BLOB NOT NULL,exchange_sequence BLOB NOT NULL,sequence BLOB NOT NULL,PRIMARY KEY(owner,ordinal));
          CREATE TABLE IF NOT EXISTS usage(owner INTEGER NOT NULL REFERENCES message_revisions(id),ordinal INTEGER NOT NULL,name TEXT NOT NULL,value BLOB NOT NULL CHECK(length(value)=8),PRIMARY KEY(owner,ordinal));
          CREATE TABLE IF NOT EXISTS deltas(id INTEGER PRIMARY KEY,delivery INTEGER NOT NULL REFERENCES deliveries(id),message_ref TEXT NOT NULL,base_revision BLOB NOT NULL,revision BLOB NOT NULL);
          CREATE TABLE IF NOT EXISTS delta_changes(owner INTEGER NOT NULL REFERENCES deltas(id),ordinal INTEGER NOT NULL,kind TEXT NOT NULL,part_id INTEGER,text TEXT,parts INTEGER REFERENCES part_sets(id),PRIMARY KEY(owner,ordinal));
          CREATE TABLE IF NOT EXISTS question_choices(owner INTEGER NOT NULL REFERENCES questions(id),ordinal INTEGER NOT NULL,type TEXT NOT NULL,terminate TEXT,PRIMARY KEY(owner,ordinal));
          CREATE TABLE IF NOT EXISTS ask_windows(id INTEGER PRIMARY KEY,question INTEGER NOT NULL UNIQUE REFERENCES questions(id),flow_instance BLOB NOT NULL,exchange_sequence BLOB NOT NULL,cutoff INTEGER NOT NULL,earlier INTEGER NOT NULL);
          CREATE TABLE IF NOT EXISTS ask_window_items(owner INTEGER NOT NULL REFERENCES ask_windows(id),ordinal INTEGER NOT NULL,message INTEGER NOT NULL REFERENCES messages(id),revision BLOB NOT NULL,PRIMARY KEY(owner,ordinal));
          CREATE TABLE IF NOT EXISTS facts(reference TEXT PRIMARY KEY,kind TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS fact_records(reference TEXT NOT NULL REFERENCES facts(reference),record_kind TEXT NOT NULL,record_id INTEGER NOT NULL,PRIMARY KEY(reference,record_kind,record_id));
          CREATE TABLE IF NOT EXISTS native_request_handoffs(
            id INTEGER PRIMARY KEY,delivery INTEGER NOT NULL REFERENCES deliveries(id),
            observation TEXT NOT NULL,request TEXT,source BLOB NOT NULL CHECK(length(source)=16),
            phase TEXT NOT NULL,bound_event TEXT,fact TEXT NOT NULL,projection BLOB,enclosing_call TEXT,
            actor_tgid INTEGER NOT NULL,actor_tid INTEGER NOT NULL,
            thread_birth BLOB NOT NULL CHECK(length(thread_birth)=8),
            observed_boottime_ns BLOB NOT NULL CHECK(length(observed_boottime_ns)=8));
          CREATE INDEX IF NOT EXISTS native_handoff_observation ON native_request_handoffs(observation);
          CREATE INDEX IF NOT EXISTS native_handoff_request ON native_request_handoffs(request);
          CREATE INDEX IF NOT EXISTS native_handoff_source_phase ON native_request_handoffs(source,phase);
          CREATE INDEX IF NOT EXISTS native_handoff_event ON native_request_handoffs(bound_event) WHERE bound_event IS NOT NULL;
          CREATE INDEX IF NOT EXISTS native_handoff_thread_time ON native_request_handoffs(source,phase,actor_tgid,actor_tid,thread_birth,observed_boottime_ns);
          CREATE TABLE IF NOT EXISTS relations(id INTEGER PRIMARY KEY,kind TEXT NOT NULL,from_ref TEXT NOT NULL REFERENCES facts(reference),to_ref TEXT NOT NULL REFERENCES facts(reference),UNIQUE(kind,from_ref,to_ref));
          CREATE TABLE IF NOT EXISTS relation_evidence(owner INTEGER NOT NULL REFERENCES relations(id),ordinal INTEGER NOT NULL,reference TEXT NOT NULL REFERENCES facts(reference),PRIMARY KEY(owner,ordinal));
          CREATE TABLE IF NOT EXISTS group_tcp_exchanges(
            id INTEGER PRIMARY KEY,capture BLOB NOT NULL UNIQUE,request INTEGER NOT NULL REFERENCES evidence_sets(id),
            response INTEGER NOT NULL REFERENCES evidence_sets(id),parent INTEGER NOT NULL REFERENCES process_keys(id),
            handoff TEXT,fact TEXT NOT NULL);
          CREATE INDEX IF NOT EXISTS group_tcp_handoff ON group_tcp_exchanges(handoff,parent);
          CREATE TABLE IF NOT EXISTS group_openclaw_receipts(
            id INTEGER PRIMARY KEY, protocol INTEGER NOT NULL REFERENCES protocol_records(id),
            call_index INTEGER NOT NULL, output_index INTEGER NOT NULL,
            tool_call TEXT NOT NULL, child_run TEXT NOT NULL, child_session TEXT NOT NULL,
            UNIQUE(protocol,output_index));
          CREATE TABLE IF NOT EXISTS group_unverified_streams(
            id INTEGER PRIMARY KEY,source BLOB NOT NULL CHECK(length(source)=16),
            generation BLOB NOT NULL CHECK(length(generation)=8),first_syn_ordinal BLOB NOT NULL CHECK(length(first_syn_ordinal)=8),
            client_port INTEGER NOT NULL,server_port INTEGER NOT NULL,from_client INTEGER NOT NULL CHECK(from_client IN(0,1)),reason TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS group_unverified_frames(
            owner INTEGER NOT NULL REFERENCES group_unverified_streams(id),ordinal INTEGER NOT NULL,
            stream_offset BLOB NOT NULL CHECK(length(stream_offset)=8),wire INTEGER NOT NULL REFERENCES evidence_sets(id),
            PRIMARY KEY(owner,ordinal));
          CREATE TABLE IF NOT EXISTS group_gateway_connections(
            capture BLOB PRIMARY KEY CHECK(length(capture)=32),source BLOB NOT NULL CHECK(length(source)=16),
            generation BLOB NOT NULL CHECK(length(generation)=8),first_syn_ordinal BLOB NOT NULL CHECK(length(first_syn_ordinal)=8),
            client_port INTEGER NOT NULL,server_port INTEGER NOT NULL,
            client INTEGER NOT NULL REFERENCES process_keys(id),server INTEGER NOT NULL REFERENCES process_keys(id),
            request INTEGER NOT NULL REFERENCES evidence_sets(id),reply INTEGER NOT NULL REFERENCES evidence_sets(id));
          CREATE INDEX IF NOT EXISTS group_gateway_server ON group_gateway_connections(server,capture);
          CREATE TABLE IF NOT EXISTS group_gateway_socket_identity(
            capture BLOB PRIMARY KEY REFERENCES group_gateway_connections(capture),
            client_cookie BLOB NOT NULL CHECK(length(client_cookie)=8 AND client_cookie<>X'0000000000000000'),
            server_cookie BLOB NOT NULL CHECK(length(server_cookie)=8 AND server_cookie<>X'0000000000000000'));
          CREATE TABLE IF NOT EXISTS group_gateway_messages(
            id INTEGER PRIMARY KEY,connection BLOB NOT NULL REFERENCES group_gateway_connections(capture),
            from_client INTEGER NOT NULL CHECK(from_client IN(0,1)),sender INTEGER REFERENCES process_keys(id),
            receiver INTEGER REFERENCES process_keys(id),reason TEXT,decode_issue TEXT CHECK(decode_issue IN ('non_text','invalid_json')),
            CHECK((reason IS NULL AND sender IS NOT NULL AND receiver IS NOT NULL)
               OR (reason IS NOT NULL AND sender IS NULL AND receiver IS NULL)));
          CREATE TABLE IF NOT EXISTS group_gateway_frames(
            message INTEGER NOT NULL REFERENCES group_gateway_messages(id),ordinal INTEGER NOT NULL,
            connection BLOB NOT NULL REFERENCES group_gateway_connections(capture),from_client INTEGER NOT NULL CHECK(from_client IN(0,1)),
            stream_offset BLOB NOT NULL CHECK(length(stream_offset)=8),wire INTEGER NOT NULL REFERENCES evidence_sets(id),
            PRIMARY KEY(message,ordinal));
          CREATE INDEX IF NOT EXISTS group_gateway_frame_position ON group_gateway_frames(connection,from_client,stream_offset);
          CREATE TABLE IF NOT EXISTS group_gateway_facts(
            message INTEGER PRIMARY KEY REFERENCES group_gateway_messages(id),kind TEXT NOT NULL,
            run_id TEXT NOT NULL,session_key TEXT NOT NULL,fact TEXT NOT NULL);
          CREATE INDEX IF NOT EXISTS group_gateway_identity ON group_gateway_facts(run_id,kind,session_key);
          CREATE TABLE IF NOT EXISTS gateway_message_deliveries(
            message INTEGER PRIMARY KEY REFERENCES group_gateway_messages(id),delivery INTEGER NOT NULL REFERENCES deliveries(id));
          CREATE TABLE IF NOT EXISTS gateway_rpc_observations(
            message INTEGER PRIMARY KEY REFERENCES group_gateway_messages(id),capture BLOB NOT NULL,
            kind TEXT NOT NULL,rpc_id TEXT,method TEXT,ok INTEGER);
          CREATE INDEX IF NOT EXISTS gateway_rpc_pair ON gateway_rpc_observations(capture,rpc_id,kind);
          CREATE TABLE IF NOT EXISTS gateway_session_observations(
            id INTEGER PRIMARY KEY,message INTEGER NOT NULL REFERENCES group_gateway_messages(id),object_pointer TEXT NOT NULL,
            session_id TEXT,session_key TEXT,run_id TEXT,last_run_id TEXT,agent_id TEXT,
            request INTEGER REFERENCES group_gateway_messages(id),UNIQUE(message,object_pointer));
          CREATE INDEX IF NOT EXISTS gateway_session_identity ON gateway_session_observations(session_id,run_id,last_run_id);
          CREATE TABLE IF NOT EXISTS gateway_history_windows(
            message INTEGER PRIMARY KEY REFERENCES group_gateway_messages(id),request INTEGER NOT NULL REFERENCES group_gateway_messages(id),
            kind TEXT NOT NULL,session INTEGER REFERENCES gateway_session_observations(id),messages_pointer TEXT,
            input_cursor_pointer TEXT,output_cursor_pointer TEXT,cursor_kind TEXT,cursor_value TEXT,input_cursor_value TEXT);
          CREATE INDEX IF NOT EXISTS gateway_window_cursor ON gateway_history_windows(cursor_kind,cursor_value);
          CREATE INDEX IF NOT EXISTS gateway_window_input_cursor ON gateway_history_windows(input_cursor_value);
          CREATE TABLE IF NOT EXISTS gateway_history_sessions(
            window INTEGER NOT NULL REFERENCES gateway_history_windows(message),session INTEGER NOT NULL REFERENCES gateway_session_observations(id),
            PRIMARY KEY(window,session));
          CREATE TABLE IF NOT EXISTS gateway_chat_observations(
            id INTEGER PRIMARY KEY,message INTEGER NOT NULL REFERENCES group_gateway_messages(id),object_pointer TEXT NOT NULL,
            wrapper_pointer TEXT,history_pointer TEXT,history_index INTEGER,full_window INTEGER NOT NULL,
            native_id_kind TEXT,native_id_value TEXT,wrapper_id_kind TEXT,wrapper_id_value TEXT,
            sequence_kind TEXT,sequence_value TEXT,wrapper_sequence_kind TEXT,wrapper_sequence_value TEXT,
            revision_kind TEXT,revision_value TEXT,run_id TEXT,message_run_id TEXT,UNIQUE(message,object_pointer));
          CREATE INDEX IF NOT EXISTS gateway_chat_run ON gateway_chat_observations(run_id,message_run_id);
          CREATE INDEX IF NOT EXISTS gateway_chat_message_run ON gateway_chat_observations(message_run_id);
          CREATE TABLE IF NOT EXISTS gateway_chat_session_fields(
            chat INTEGER NOT NULL REFERENCES gateway_chat_observations(id),session INTEGER NOT NULL REFERENCES gateway_session_observations(id),
            PRIMARY KEY(chat,session));
          CREATE INDEX IF NOT EXISTS gateway_chat_session_source ON gateway_chat_session_fields(session,chat);
          CREATE INDEX IF NOT EXISTS gateway_session_request ON gateway_session_observations(request,message);
          CREATE TABLE IF NOT EXISTS gateway_projection_gaps(
            message INTEGER NOT NULL REFERENCES group_gateway_messages(id),object_pointer TEXT NOT NULL,gaps TEXT NOT NULL,
            PRIMARY KEY(message,object_pointer));
          CREATE TABLE IF NOT EXISTS chat_session_entries(
            id INTEGER PRIMARY KEY,delivery INTEGER NOT NULL REFERENCES deliveries(id),kind TEXT NOT NULL,
            protocol INTEGER REFERENCES protocol_records(id),gateway_session INTEGER REFERENCES gateway_session_observations(id),
            CHECK((kind='model_http' AND protocol IS NOT NULL AND gateway_session IS NULL)
               OR (kind='native_gateway' AND protocol IS NULL AND gateway_session IS NOT NULL)));
          CREATE UNIQUE INDEX IF NOT EXISTS chat_session_model ON chat_session_entries(protocol) WHERE protocol IS NOT NULL;
          CREATE UNIQUE INDEX IF NOT EXISTS chat_session_native ON chat_session_entries(gateway_session) WHERE gateway_session IS NOT NULL;
          CREATE TABLE IF NOT EXISTS gateway_chat_memberships(
            chat INTEGER NOT NULL REFERENCES gateway_chat_observations(id),delivery INTEGER NOT NULL REFERENCES deliveries(id),
            session INTEGER REFERENCES chat_session_entries(id),handoff INTEGER REFERENCES gateway_session_observations(id),
            message_anchor INTEGER NOT NULL REFERENCES gateway_chat_observations(id),gaps TEXT NOT NULL,PRIMARY KEY(chat,delivery));
          CREATE INDEX IF NOT EXISTS gateway_membership_session ON gateway_chat_memberships(session,chat,delivery);
          CREATE INDEX IF NOT EXISTS process_key_lookup ON process_keys(boot_id,host_tgid,birth_boottime_ns);
          CREATE TABLE IF NOT EXISTS group_otlp_batches(
            id INTEGER PRIMARY KEY,digest BLOB NOT NULL UNIQUE,original INTEGER NOT NULL REFERENCES evidence_sets(id));
          CREATE TABLE IF NOT EXISTS group_otlp_spans(
            batch INTEGER NOT NULL REFERENCES group_otlp_batches(id),trace TEXT NOT NULL,span TEXT NOT NULL,
            actor INTEGER NOT NULL REFERENCES process_keys(id),digest BLOB NOT NULL,fact TEXT NOT NULL,
            PRIMARY KEY(actor,trace,span,digest));
          CREATE INDEX IF NOT EXISTS group_otlp_trace ON group_otlp_spans(trace,actor,span);
          CREATE TABLE IF NOT EXISTS group_unframed(id INTEGER PRIMARY KEY,boot BLOB NOT NULL,fact TEXT NOT NULL,
            body INTEGER REFERENCES evidence_sets(id),
            reason TEXT NOT NULL DEFAULT 'http_boundary_unverified');
          CREATE TABLE IF NOT EXISTS context_gaps(id INTEGER PRIMARY KEY,at_ref TEXT NOT NULL,reason TEXT NOT NULL,UNIQUE(at_ref,reason));
          CREATE TABLE IF NOT EXISTS source_gaps(id INTEGER PRIMARY KEY,delivery INTEGER NOT NULL REFERENCES deliveries(id),kind TEXT NOT NULL,flow_instance BLOB,exchange_sequence BLOB,start_runtime BLOB,start_sequence BLOB,end_runtime BLOB,end_sequence BLOB,reason TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS credential_facts(id INTEGER PRIMARY KEY,delivery INTEGER NOT NULL REFERENCES deliveries(id),flow_instance BLOB NOT NULL,exchange_sequence BLOB NOT NULL,sequence BLOB NOT NULL,revision BLOB NOT NULL,actor INTEGER NOT NULL REFERENCES actors(id),binding BLOB,destination_origin TEXT NOT NULL,state TEXT NOT NULL,auth_field TEXT,reason TEXT,observed_boottime_ns BLOB NOT NULL);
          CREATE TABLE IF NOT EXISTS exchange_failures(id INTEGER PRIMARY KEY,delivery INTEGER NOT NULL REFERENCES deliveries(id),flow_instance BLOB NOT NULL,exchange_sequence BLOB NOT NULL,http_status INTEGER,reason TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS flow_status(id INTEGER PRIMARY KEY,delivery INTEGER NOT NULL REFERENCES deliveries(id),intake_state TEXT NOT NULL,mode TEXT NOT NULL,active_exchanges BLOB NOT NULL,lost_deliveries BLOB NOT NULL,parse_failures BLOB NOT NULL);
          CREATE TABLE IF NOT EXISTS status_profiles(owner INTEGER NOT NULL REFERENCES flow_status(id),ordinal INTEGER NOT NULL,profile TEXT NOT NULL,PRIMARY KEY(owner,ordinal));
          CREATE TABLE IF NOT EXISTS feedback(id INTEGER PRIMARY KEY,delivery INTEGER NOT NULL REFERENCES deliveries(id),denial_ref TEXT NOT NULL,call_ref TEXT NOT NULL,state TEXT NOT NULL,original_flow BLOB,original_exchange BLOB,original_sequence BLOB,evidence INTEGER REFERENCES evidence_sets(id),failure_code TEXT);
          CREATE TABLE IF NOT EXISTS feedback_bindings(denial_ref TEXT PRIMARY KEY,call_ref TEXT NOT NULL UNIQUE,last_fact INTEGER REFERENCES feedback(id),failure TEXT);
          CREATE UNIQUE INDEX IF NOT EXISTS protocol_identity ON protocol_records(flow_instance,exchange_sequence,sequence);
          CREATE INDEX IF NOT EXISTS chat_observation_delivery ON chat_inputs(payload_row,delivery) WHERE event_kind='observation';
          CREATE UNIQUE INDEX IF NOT EXISTS message_identity ON messages(flow_instance,exchange_sequence,sequence);
          CREATE UNIQUE INDEX IF NOT EXISTS message_revision ON message_revisions(message,revision);
          CREATE UNIQUE INDEX IF NOT EXISTS question_identity ON questions(decision_id);
          CREATE UNIQUE INDEX IF NOT EXISTS completion_identity ON operation_reports(reference);
          CREATE INDEX IF NOT EXISTS message_exchange_order ON messages(flow_instance,exchange_sequence,origin,first_delivery);
          CREATE INDEX IF NOT EXISTS record_ref_identity ON message_record_refs(flow_instance,exchange_sequence,sequence);
          CREATE INDEX IF NOT EXISTS protocol_native_value ON protocol_native_ids(namespace,value_kind,value);
          CREATE INDEX IF NOT EXISTS protocol_request_trace ON protocol_sources(origin,request_traceparent) WHERE side='client_request';
          CREATE INDEX IF NOT EXISTS native_call_value ON part_native_ids(namespace,value_kind,value);
          CREATE INDEX IF NOT EXISTS native_tool_event ON native_tool_executions(event_ref);
          CREATE INDEX IF NOT EXISTS attribution_observed_time ON message_revisions(last_seen_boottime_ns) WHERE state='complete' AND derived=0;
          CREATE INDEX IF NOT EXISTS relation_source ON relations(from_ref);
          CREATE INDEX IF NOT EXISTS relation_target ON relations(to_ref);
          PRAGMA user_version=2;")?;
        Ok::<_, StoreError>(())
    })();
    match made {
        Ok(()) => {
            conn.execute_batch("COMMIT")?;
            Ok(())
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(conn: &Connection) -> Result<Vec<(String, String, Option<String>)>, StoreError> {
        Ok(conn
            .prepare("SELECT type,name,sql FROM sqlite_schema ORDER BY type,name")?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .collect::<Result<Vec<_>, _>>()?)
    }

    #[test]
    fn current_session_layout_rejects_missing_tables_or_columns_without_repair()
    -> Result<(), StoreError> {
        for damage in [
            "DROP TABLE gateway_history_sessions",
            "ALTER TABLE gateway_rpc_observations DROP COLUMN method",
            "DROP TABLE group_gateway_frames",
        ] {
            let conn = Connection::open_in_memory()?;
            initialize(&conn)?;
            conn.execute_batch(damage)?;
            let before = layout(&conn)?;
            assert!(
                matches!(initialize(&conn), Err(StoreError::Invalid)),
                "{damage}"
            );
            assert_eq!(layout(&conn)?, before, "rejection must not repair {damage}");
        }
        Ok(())
    }

    #[test]
    fn version_zero_nonempty_database_is_not_a_new_install() -> Result<(), StoreError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("CREATE TABLE gateway_history_windows(obsolete TEXT)")?;
        let before = layout(&conn)?;
        assert!(matches!(initialize(&conn), Err(StoreError::Invalid)));
        assert_eq!(layout(&conn)?, before);
        assert_eq!(
            conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))?,
            0
        );
        Ok(())
    }
}
