//! Real temporary databases only; no QA history or security decision is modified.
use super::*;
use crate::{context::FactReference, database::BusinessLog, execution_feedback::tests::Fixture};
use as_types::{
    configuration::LogStorageSettings,
    context::{ConfigPaths, ResolvedPaths},
    flow::*,
    log_storage::LogStorageBudget,
    main_ports::{SecurityLogEntry, SecurityLogSink},
    security_log::FileCloseAuditRecord,
};
use std::{num::NonZeroU64, path::Path, sync::Arc};

#[test]
fn backfill_reopen_uses_both_key_columns_and_preserves_existing_generations()
-> Result<(), StoreError> {
    use rusqlite::StatementStatus;
    fn run(rows: i64) -> Result<(i32, i32), StoreError> {
        let mut conn = Connection::open_in_memory()?;
        crate::database::schema::initialize(&conn)?;
        initialize(&conn)?;
        begin_write(&conn)?;
        {
            let tx = conn.transaction()?;
            for id in 1..=rows {
                tx.execute("INSERT INTO messages(id) VALUES(?1)", [id])?;
            }
            tx.commit()?;
        }
        let current = backfill("messages");
        let previous = current.replace("r.object=+p.id", "r.object=p.id");
        let plan = |sql: &str| -> Result<Vec<String>, StoreError> {
            let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?;
            Ok(stmt
                .query_map([], |r| r.get(3))?
                .collect::<Result<_, _>>()?)
        };
        let before = plan(&previous)?;
        let after = plan(&current)?;
        println!(
            "RETENTION_BACKFILL_PLAN sqlite={} rows={rows} before={before:?} after={after:?}",
            rusqlite::version()
        );
        assert!(
            before
                .iter()
                .any(|p| p.contains("SEARCH r") && p.contains("kind=?") && !p.contains("object=?"))
        );
        assert!(
            after
                .iter()
                .any(|p| p.contains("SEARCH r") && p.contains("kind=?") && p.contains("object=?"))
        );
        let measure = |sql: &str| -> Result<i32, StoreError> {
            let mut s = conn.prepare(sql)?;
            assert_eq!(s.execute([])?, 0);
            Ok(s.get_status(StatementStatus::VmStep))
        };
        let old_steps = measure(&previous)?;
        let new_steps = measure(&current)?;
        println!("RETENTION_BACKFILL_STEPS rows={rows} before={old_steps} after={new_steps}");
        // This is a plan/work regression, without a wall-clock timeout or performance gate.
        assert!(old_steps > new_steps * 8);
        conn.execute("INSERT INTO messages(id) VALUES(?1)", [i64::MAX])?;
        let original:Vec<(i64,i64,String)>=conn.prepare("SELECT object,generation,typeof(object) FROM retention_records WHERE kind='messages' ORDER BY object")?.query_map([],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?.collect::<Result<_,_>>()?;
        initialize(&conn)?;
        let reopened:Vec<(i64,i64,String)>=conn.prepare("SELECT object,generation,typeof(object) FROM retention_records WHERE kind='messages' ORDER BY object")?.query_map([],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?.collect::<Result<_,_>>()?;
        assert_eq!(original, reopened);
        assert_eq!(
            reopened.len(),
            usize::try_from(rows + 1).map_err(|_| StoreError::Invalid)?
        );
        assert!(reopened.iter().all(|(_, _, kind)| kind == "integer"));
        assert_eq!(reopened.last().map(|r| r.0), Some(i64::MAX));
        // Table presence alone must not hide a missing item from an interrupted backfill.
        conn.execute(
            "DELETE FROM retention_records WHERE kind='messages' AND object=?1",
            [1],
        )?;
        initialize(&conn)?;
        let repaired:i64=conn.query_row("SELECT count(*) FROM retention_records WHERE kind='messages' AND object=1 AND typeof(object)='integer'",[],|r|r.get(0))?;
        assert_eq!(repaired, 1);
        let still_original: i64 = conn.query_row(
            "SELECT generation FROM retention_records WHERE kind='messages' AND object=?1",
            [i64::MAX],
            |r| r.get(0),
        )?;
        assert_eq!(Some(still_original), original.last().map(|r| r.1));
        Ok((old_steps, new_steps))
    }
    let small = run(512)?;
    let large = run(1024)?;
    assert!(
        large.0 > small.0 * 3,
        "the previous same-kind scan was quadratic"
    );
    assert!(
        large.1 < small.1 * 3,
        "the fixed lookup does not scan the whole kind per root"
    );
    Ok(())
}

fn paths(root: &Path) -> ConfigPaths {
    ConfigPaths::from_configuration(ResolvedPaths {
        backend_executable: root.join("bin/as-service"),
        self_protection_root: root.to_owned(),
        configuration_file: root.join("config/as.yaml"),
        rules_file: root.join("config/rules.yaml"),
        driver_assets_directory: root.join("driver"),
        middleware_endpoint: root.join("run/middleware.sock"),
        data_directory: root.join("data"),
        credential_store_file: root.join("data/credentials/store"),
        ca_public_file: root.join("data/credentials/ca.crt"),
        security_log_file: root.join("log/security.sqlite"),
        middleware_database_file: root.join("data/middleware/business.sqlite"),
    })
}
fn budget(bytes: u64) -> Arc<LogStorageBudget> {
    LogStorageBudget::new(LogStorageSettings {
        total_bytes: NonZeroU64::new(bytes).expect("positive test budget"),
    })
}
async fn observation(
    log: &BusinessLog,
    sequence: u64,
    size: usize,
) -> Result<ProtocolObservation, StoreError> {
    let mut value = Fixture::new().protocol;
    value.id.sequence = sequence;
    value.body = EvidenceBody::from_bytes(vec![(sequence % 251) as u8; size]);
    value.native_ids.clear();
    log.record_chat(Arc::new(ChatEnvelope {
        cursor: ChatCursor {
            service_instance: [1; 16],
            sequence,
        },
        event: Arc::new(ChatEvent::Observation(Arc::new(value.clone()))),
    }))
    .await?;
    Ok(value)
}
fn foreign_keys(conn: &Connection) -> Result<(), StoreError> {
    let broken: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_foreign_key_check)",
        [],
        |r| r.get(0),
    )?;
    assert!(
        !broken,
        "retirement must retain all surviving relational references"
    );
    Ok(())
}

#[tokio::test]
async fn shared_budget_rolls_both_real_files_and_reopens_latest_original()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    let paths = paths(dir.path());
    let limit = 2 * 1024 * 1024;
    let budget = budget(limit);
    let log = BusinessLog::initialize(&paths, budget.clone()).await?;
    let audit = as_security_log::SecurityLog::open(&paths, budget.clone()).await?;
    let (send, receive) = tokio::sync::mpsc::channel(1);
    let audited = tokio::spawn(Box::new(audit).run(receive));
    // Real original rows and their unfinished business claim. This opaque value
    // has the same sole Drop contract as the runtime prepare/Transaction field.
    let mut feedback = Fixture::new();
    feedback.protocol.id.exchange.flow_instance = [8; 16];
    feedback.message.id.exchange = feedback.protocol.id.exchange;
    feedback.message.record_refs = vec![feedback.protocol.id];
    feedback.call.message = feedback.message.id;
    for (sequence, event) in [
        (
            1,
            ChatEvent::Observation(Arc::new(feedback.protocol.clone())),
        ),
        (2, ChatEvent::Finished(Arc::new(feedback.message.clone()))),
    ] {
        log.record_chat(Arc::new(ChatEnvelope {
            cursor: ChatCursor {
                service_instance: [8; 16],
                sequence,
            },
            event: Arc::new(event),
        }))
        .await?;
    }
    log.record_up(Arc::new(as_main::messages::protocol::Up::OperationResult(
        Arc::new(feedback.completion.clone()),
    )))?
    .await?;
    let event = FactReference::Event(as_types::process_tree::EventKey::from(
        feedback.completion.event.as_ref(),
    ));
    let part = FactReference::Part(feedback.call);
    let request = RuleFeedbackRequest {
        denial: as_types::process_tree::EventKey::from(feedback.completion.event.as_ref()),
        call: feedback.call,
    };
    assert!(matches!(
        log.claim_feedback(request).await?,
        crate::database::query::FeedbackClaim::New
    ));
    let retention_guard: Option<Box<dyn Send + Sync>> =
        Some(Box::new(log.retain_facts(&[event.clone(), part.clone()])?));
    let mut newest = None;
    for sequence in 1..=24 {
        send.send(SecurityLogEntry::FileClose(
            FileCloseAuditRecord::FileWriteClose {
                reporter_tid: 7,
                path: Some(vec![b'x'; 64 * 1024]),
                path_error: None,
                device_major: 0,
                device_minor: 0,
                inode: sequence,
            },
        ))
        .await?;
        newest = Some(observation(&log, sequence, 128 * 1024).await?);
    }
    drop(send);
    audited.await??;
    assert!(log.activity(event.clone()).await.is_ok());
    assert!(log.activity(part.clone()).await.is_ok());
    assert!(matches!(
        log.claim_feedback(request).await?,
        crate::database::query::FeedbackClaim::Unconfirmed
    ));
    drop(retention_guard);
    while log.activity(event.clone()).await.is_ok() || log.activity(part.clone()).await.is_ok() {
        assert!(
            log.inner.retire_oldest()?,
            "the retired runtime custody permits old originals to roll"
        );
    }
    assert!(matches!(
        log.activity(event).await,
        Err(StoreError::NotProvided)
    ));
    assert!(matches!(
        log.activity(part).await,
        Err(StoreError::NotProvided)
    ));
    log.call(|db| {
        assert_eq!(
            db.query_row("SELECT count(*) FROM feedback_bindings", [], |r| r
                .get::<_, i64>(0))?,
            0
        );
        Ok(())
    })?
    .await?;
    let audit_rows = {
        let conn = Connection::open_with_flags(
            paths.security_log_file(),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        let count: i64 = conn.query_row("SELECT count(*) FROM rule_logs", [], |r| r.get(0))?;
        let original: String = conn.query_row(
            "SELECT record FROM rule_logs ORDER BY id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )?;
        let SecurityLogEntry::FileClose(FileCloseAuditRecord::FileWriteClose {
            inode, path, ..
        }) = serde_json::from_str(&original)?
        else {
            panic!("actual audit variant")
        };
        assert_eq!(inode, 24);
        assert_eq!(path, Some(vec![b'x'; 64 * 1024]));
        assert!(count < 24 && count > 0);
        count
    };
    let usage = budget.usage()?;
    println!("AS_LOG_STORAGE_TEMPORARY_FILES {usage:?} audit_rows={audit_rows}");
    assert!(
        usage.business_bytes + usage.security_bytes <= limit,
        "{usage:?}"
    );
    assert!(
        usage.security_bytes > 0,
        "closed audit file still counts in the total"
    );
    let expected = newest.expect("actual last input");
    let reference = FactReference::Protocol(expected.id);
    let retained = log.activity(reference.clone()).await?;
    assert!(
        retained
            .source_records
            .iter()
            .any(|r| matches!(r,crate::context::SourceRecord::Protocol(p) if **p==expected))
    );
    let counts = log
        .call(|db| {
            foreign_keys(db)?;
            Ok(
                db.query_row("SELECT count(*) FROM protocol_records", [], |r| {
                    r.get::<_, i64>(0)
                })?,
            )
        })?
        .await?;
    assert!(
        counts < 24 && counts > 0,
        "oldest local units were actually retired"
    );
    assert_eq!(log.write_failure_count(), 0);
    log.close().await?;
    drop(log);
    let reopened = BusinessLog::initialize(&paths, budget::default_budget()).await?;
    assert!(reopened.activity(reference).await.is_ok());
    reopened.close().await?;
    Ok(())
}
mod budget {
    pub(super) fn default_budget() -> std::sync::Arc<as_types::log_storage::LogStorageBudget> {
        as_types::log_storage::LogStorageBudget::new(Default::default())
    }
}

#[tokio::test]
async fn live_reference_keeps_its_original_closure_then_retires_without_dangling_parts()
-> Result<(), StoreError> {
    let dir = tempfile::tempdir()?;
    let log = BusinessLog::open_path(&dir.path().join("business.sqlite"))?;
    let f = Fixture::new();
    log.record_chat(Arc::new(ChatEnvelope {
        cursor: ChatCursor {
            service_instance: [2; 16],
            sequence: 1,
        },
        event: Arc::new(ChatEvent::Observation(Arc::new(f.protocol.clone()))),
    }))
    .await?;
    log.record_chat(Arc::new(ChatEnvelope {
        cursor: ChatCursor {
            service_instance: [2; 16],
            sequence: 2,
        },
        event: Arc::new(ChatEvent::Finished(Arc::new(f.message.clone()))),
    }))
    .await?;
    let pin = log.retain_facts(&[FactReference::Part(f.call)])?;
    log.record_chat(Arc::new(ChatEnvelope {
        cursor: ChatCursor {
            service_instance: [2; 16],
            sequence: 3,
        },
        event: Arc::new(ChatEvent::Delta {
            id: f.message.id,
            base_revision: f.message.revision,
            revision: 2,
            changes: vec![ChatDelta::PutPart {
                part: ChatPart::Text {
                    id: 1,
                    kind: TextKind::Plain,
                    text: "retained delta".into(),
                },
            }],
        }),
    }))
    .await?;
    let second_pin = log.retain_facts(&[FactReference::Part(f.call)])?;
    drop(pin);
    observation(&log, 50, 8192).await?;
    observation(&log, 51, 8192).await?;
    log.call(|db| truncate_wal(db))?.await?;
    assert!(log.inner.retire_oldest()?);
    assert!(
        log.activity(FactReference::Part(f.call)).await.is_ok(),
        "another guard for the same reference remains live"
    );
    drop(second_pin);
    assert!(log.inner.retire_oldest()?);
    assert!(matches!(
        log.activity(FactReference::Part(f.call)).await,
        Err(StoreError::NotProvided)
    ));
    log.call(|db| foreign_keys(db))?.await?;
    log.close().await
}

#[tokio::test]
async fn gateway_retention_uses_capture_key_across_vacuum_and_reopen() -> Result<(), StoreError> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("business.sqlite");
    let log = BusinessLog::open_path(&path)?;
    let capture = [42u8; 32];
    log.write_call(move|db|{
        let tx=db.transaction()?;
        tx.execute("INSERT INTO process_keys(boot_id,host_tgid,birth_boottime_ns) VALUES(?1,1,?2)",params![[7u8;16].as_slice(),0u64.to_be_bytes().as_slice()])?;
        let actor=tx.last_insert_rowid();
        let body=crate::database::parts::write_evidence(&tx,&EvidenceBody::from_bytes(b"original handshake".to_vec()))?;
        tx.execute("INSERT INTO group_gateway_connections(capture,source,generation,first_syn_ordinal,client_port,server_port,client,server,request,reply) VALUES(?1,?2,?3,?3,1000,18789,?4,?4,?5,?5)",params![capture.as_slice(),[9u8;16].as_slice(),1u64.to_be_bytes().as_slice(),actor,body])?;
        tx.commit()?;Ok(())
    })?.await?;
    observation(&log, 60, 32).await?;
    log.call(|db| {
        db.execute_batch("VACUUM")?;
        Ok(())
    })?
    .await?;
    log.close().await?;
    drop(log);
    let log = BusinessLog::open_path(&path)?;
    while log
        .call(|db| {
            Ok(
                db.query_row("SELECT count(*) FROM group_gateway_connections", [], |r| {
                    r.get::<_, i64>(0)
                })?,
            )
        })?
        .await?
        != 0
    {
        assert!(log.inner.retire_oldest()?);
    }
    log.call(|db| foreign_keys(db))?.await?;
    log.close().await
}

#[tokio::test]
async fn checkpoint_backpressure_preserves_commit_and_failure_accounting() -> Result<(), StoreError>
{
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("business.sqlite");
    let budget = budget(1024 * 1024);
    let log = BusinessLog::open_budget(&path, budget.clone())?;
    observation(&log, 70, 32).await?;
    let reader = Connection::open(&path)?;
    reader.execute_batch("BEGIN; SELECT * FROM protocol_records;")?;
    let original = observation(&log, 71, 768 * 1024).await?;
    assert!(budget.usage()?.deferred_maintenance > 0);
    assert_eq!(
        log.write_failure_count(),
        0,
        "checkpoint busy is maintenance, not failed original SQL"
    );
    assert!(
        log.activity(FactReference::Protocol(original.id))
            .await
            .is_ok()
    );
    reader.execute_batch("ROLLBACK")?;
    drop(reader);
    observation(&log, 72, 32).await?;
    let usage = budget.usage()?;
    assert!(usage.business_bytes <= usage.total_limit_bytes, "{usage:?}");
    log.call(|db| foreign_keys(db))?.await?;
    log.close().await
}
