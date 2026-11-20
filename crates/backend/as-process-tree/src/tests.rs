use super::*;
use std::sync::atomic::AtomicUsize;
fn budget() -> Arc<model::Budget> {
    Arc::new(model::Budget {
        used: AtomicUsize::new(0),
        max: 1024 * 1024,
    })
}
fn key(pid: u32, birth: u64) -> ProcessKey {
    ProcessKey {
        boot_id: [1; 16],
        host_tgid: pid,
        birth_boottime_ns: birth,
    }
}
pub(crate) fn sample(pid: u32, birth: u64, time: u64, mut facts: ProcessInfo) -> Candidate {
    facts.kind.get_or_insert(ProcessKind::User);
    Candidate {
        key: key(pid, birth),
        facts,
        cgroup: Some(1),
        leader: Some(b'S'),
        window: SampleWindow {
            begin_boottime_ns: time,
            end_boottime_ns: time,
        },
        origin: Origin::Refresh,
        executed: false,
        states: Default::default(),
    }
}
#[test]
fn first_view_frozen_pid_reuse_is_distinct_and_held_query_stays_charged() {
    let budget = budget();
    let mut s = State::new();
    let a = key(5, 10);
    s.put(
        sample(
            5,
            10,
            20,
            ProcessInfo {
                name: Some(b"initial".to_vec()),
                ..Default::default()
            },
        ),
        &budget,
    )
    .expect("first");
    let initial = s.nodes[&a].record.initial.clone();
    s.put(
        sample(
            5,
            10,
            30,
            ProcessInfo {
                name: Some(b"latest".to_vec()),
                ..Default::default()
            },
        ),
        &budget,
    )
    .expect("update");
    assert_eq!(initial.process.name.as_deref(), Some(b"initial".as_slice()));
    assert_eq!(
        s.nodes[&a].record.latest.process.name.as_deref(),
        Some(b"latest".as_slice())
    );
    s.put(sample(5, 40, 45, ProcessInfo::default()), &budget)
        .expect("new same PID");
    s.retire(
        a,
        Some(SampleWindow {
            begin_boottime_ns: 50,
            end_boottime_ns: 50,
        }),
        Some(0),
    );
    assert_eq!(s.pids.get(&5), Some(&key(5, 40)));
    s.closed_before = 100;
    s.collect(Duration::ZERO, true);
    assert!(!s.nodes.contains_key(&a));
    s.nodes.clear();
    assert!(
        budget.used.load(Ordering::Acquire) > 0,
        "held initial remains charged"
    );
    drop(initial);
    assert_eq!(budget.used.load(Ordering::Acquire), 0);
}
#[test]
fn parent_change_cannot_keep_an_old_parents_executable() {
    let b = budget();
    let mut s = State::new();
    let k = key(7, 10);
    s.put(
        sample(
            7,
            10,
            20,
            ProcessInfo {
                parent_pid: Some(1),
                parent_birth_boottime_ns: Some(2),
                parent_executable: Some(b"old-parent".to_vec()),
                ..Default::default()
            },
        ),
        &b,
    )
    .expect("old parent");
    s.put(
        sample(
            7,
            10,
            30,
            ProcessInfo {
                parent_pid: Some(3),
                parent_birth_boottime_ns: Some(4),
                ..Default::default()
            },
        ),
        &b,
    )
    .expect("reparent");
    assert!(
        s.nodes[&k]
            .record
            .latest
            .process
            .parent_executable
            .is_none()
    );
    s.put(
        sample(
            7,
            10,
            25,
            ProcessInfo {
                parent_pid: Some(1),
                parent_birth_boottime_ns: Some(2),
                parent_executable: Some(b"old-parent".to_vec()),
                ..Default::default()
            },
        ),
        &b,
    )
    .expect("late old sample");
    assert_eq!(s.nodes[&k].record.latest.process.parent_pid, Some(3));
    assert!(
        s.nodes[&k]
            .record
            .latest
            .process
            .parent_executable
            .is_none()
    );
}
#[test]
fn exec_invalidation_blocks_late_old_arguments_and_scan_cannot_erase_exit() {
    let b = budget();
    let mut s = State::new();
    let k = key(7, 10);
    s.put(
        sample(
            7,
            10,
            20,
            ProcessInfo {
                argv: Some(vec![b"old-args".to_vec()]),
                executable: Some(b"old".to_vec()),
                ..Default::default()
            },
        ),
        &b,
    )
    .expect("old image");
    let mut c = sample(
        7,
        10,
        40,
        ProcessInfo {
            executable: Some(b"new".to_vec()),
            ..Default::default()
        },
    );
    c.executed = true;
    s.put(c, &b).expect("exec observed");
    s.put(
        sample(
            7,
            10,
            30,
            ProcessInfo {
                argv: Some(vec![b"old-args".to_vec()]),
                ..Default::default()
            },
        ),
        &b,
    )
    .expect("late scan");
    assert!(s.nodes[&k].record.latest.process.argv.is_none());
    s.retire(
        k,
        Some(SampleWindow {
            begin_boottime_ns: 50,
            end_boottime_ns: 50,
        }),
        Some(9),
    );
    s.retire(k, None, None);
    assert_eq!(s.nodes[&k].record.lifecycle, Lifecycle::ExitObserved);
    assert_eq!(s.nodes[&k].record.exit_code, Some(9));
}
#[test]
fn resource_failure_is_explicit_and_does_not_insert_partial_node() {
    let b = Arc::new(model::Budget {
        used: AtomicUsize::new(0),
        max: 64,
    });
    let mut s = State::new();
    assert!(matches!(
        s.put(sample(5, 10, 20, ProcessInfo::default()), &b),
        Err(Error::ResourceLimit)
    ));
    assert!(s.nodes.is_empty() && s.pids.is_empty() && s.ordered.is_empty());
    assert_eq!(b.used.load(Ordering::Acquire), 0);
}
#[test]
fn proc_status_keeps_arbitrary_comm_bytes_outside_ascii_numeric_parser() {
    let mut facts = ProcessInfo::default();
    let state = crate::scan::parse_status(
        b"Name:\tas-tree-\xff\nState:\tS (sleeping)\nUid:\t1\t2\t5\t6\nGid:\t3\t4\t7\t8\nNStgid:\t91\t4\t1\n",
        &mut facts,
    )
    .expect("kernel permits nonUTF8 comm");
    assert_eq!(state, Some(b'S'));
    assert_eq!(
        (facts.uid, facts.euid, facts.gid, facts.egid),
        (Some(1), Some(2), Some(3), Some(4))
    );
    assert_eq!(
        (facts.suid, facts.fsuid, facts.sgid, facts.fsgid),
        (Some(5), Some(6), Some(7), Some(8))
    );
    assert_eq!(facts.namespace_tgids, Some(vec![91, 4, 1]));
}

#[test]
fn an_empty_proc_read_is_an_acquired_empty_detail_value() {
    assert_eq!(crate::scan::parse_cmdline(b"", 8), Ok(Vec::new()));
    assert_eq!(crate::scan::parse_cmdline(b"\0", 8), Ok(vec![Vec::new()]));
    assert_eq!(
        crate::scan::parse_cmdline(b"program\0\0last\0", 8),
        Ok(vec![b"program".to_vec(), Vec::new(), b"last".to_vec()])
    );
    assert_eq!(
        crate::scan::parse_cmdline(b"partial", 8),
        Err(FieldState::Truncated)
    );
}
#[test]
fn memory_pressure_reclaims_retired_nodes_before_refusing_new_live_node() {
    let b = Arc::new(model::Budget {
        used: AtomicUsize::new(0),
        max: 8000,
    });
    let mut s = State::new();
    s.put(sample(5, 10, 20, ProcessInfo::default()), &b)
        .expect("first live record");
    s.retire(key(5, 10), None, None);
    s.put(sample(6, 30, 40, ProcessInfo::default()), &b)
        .expect("retired data is reclaimable");
    assert!(!s.nodes.contains_key(&key(5, 10)));
    assert!(s.nodes.contains_key(&key(6, 30)));
}
pub(crate) fn settings() -> ProcessTreeSettings {
    ProcessTreeSettings {
        memory_budget_bytes: std::num::NonZeroUsize::new(16 * 1024 * 1024).expect("budget"),
        retired_retention_ms: std::num::NonZeroU64::new(300000).expect("retention"),
        reconcile_interval_ms: std::num::NonZeroU64::new(30000).expect("period"),
        query_page_size: std::num::NonZeroUsize::new(32).expect("page"),
    }
}
#[tokio::test]
async fn dropping_close_waiter_still_finishes_tree_retirement() {
    let tree = ProcessTree::prepare(settings()).expect("tree");
    let close = tree.close();
    drop(close);
    tokio::task::yield_now().await;
    assert_eq!(tree.status().expect("status").phase, TreePhase::Closed);
}
struct LostBeforeScan;
#[async_trait::async_trait]
impl ProcessFactsPort for LostBeforeScan {
    async fn inspect(&self, pids: &[u32]) -> Result<Vec<ProcessInspection>, ModuleError> {
        Ok(pids
            .iter()
            .map(|pid| ProcessInspection::Gone { host_tgid: *pid })
            .collect())
    }
    async fn facts_barrier(&self) -> Result<ProcessFactsBarrier, ModuleError> {
        let path = std::fs::read_link("/proc/self/ns/pid").expect("actual namespace");
        let text = path.to_str().expect("ascii namespace");
        let ns = text
            .strip_prefix("pid:[")
            .and_then(|v| v.strip_suffix(']'))
            .expect("namespace shape")
            .parse()
            .expect("inode");
        Ok(ProcessFactsBarrier {
            host_pid_namespace_inode: ns,
            tracking_epoch: 1,
            marker: 1,
            loss_epoch: 1,
            backend_rejected: 0,
            cutoff_boottime_ns: crate::scan::now_ns(),
            native_hit_loss: 0,
            backend_hit_rejected: 0,
        })
    }
}
#[tokio::test]
async fn initial_image_does_not_hide_a_gap_between_start_and_first_scan_barrier() {
    let tree = ProcessTree::prepare(settings()).expect("tree");
    let port = Arc::new(LostBeforeScan);
    let marker = port.facts_barrier().await.expect("simulated source");
    let start = ProcessTrackingStart {
        host_pid_namespace_inode: marker.host_pid_namespace_inode,
        tracking_epoch: 1,
        begin_marker: 0,
        loss_epoch: 0,
        backend_rejected: 0,
        cutoff_boottime_ns: marker.cutoff_boottime_ns,
    };
    assert!(matches!(
        tree.initialize(start, port).await,
        Err(Error::Conflict)
    ));
    assert_ne!(tree.status().expect("status").phase, TreePhase::Ready);
    tree.close().await.expect("retired");
}
#[test]
fn gap_reconciliation_cannot_reactivate_unconfirmed_old_parent_fields() {
    let b = budget();
    let mut s = State::new();
    let k = key(7, 10);
    s.put(
        sample(
            7,
            10,
            20,
            ProcessInfo {
                parent_pid: Some(1),
                parent_birth_boottime_ns: Some(2),
                parent_executable: Some(b"before-gap".to_vec()),
                ..Default::default()
            },
        ),
        &b,
    )
    .expect("captured old parent path");
    s.uncertain();
    s.put(
        sample(
            7,
            10,
            40,
            ProcessInfo {
                parent_pid: Some(1),
                parent_birth_boottime_ns: Some(2),
                ..Default::default()
            },
        ),
        &b,
    )
    .expect("scan confirms identity but not parent path");
    s.phase = TreePhase::Ready;
    let node = &s.nodes[&k];
    assert!(node.record.latest.process.parent_executable.is_none());
    assert_eq!(
        node.record.initial.process.parent_executable.as_deref(),
        Some(b"before-gap".as_slice())
    );
    assert_ne!(
        node.record.latest.stamps[&Field::ParentExecutable].evidence_epoch,
        s.gap_serial
    );
}
#[test]
fn retired_identity_is_pinned_by_scan_and_inflight_events_until_fence() {
    let b = budget();
    let mut s = State::new();
    let k = key(7, 10);
    s.put(sample(7, 10, 20, ProcessInfo::default()), &b)
        .expect("node");
    s.retire(
        k,
        Some(SampleWindow {
            begin_boottime_ns: 30,
            end_boottime_ns: 30,
        }),
        None,
    );
    s.collect(Duration::ZERO, true);
    assert!(
        s.nodes.contains_key(&k),
        "exit before source fence remains a tombstone"
    );
    s.closed_before = 40;
    s.scan_active = true;
    s.collect(Duration::ZERO, true);
    assert!(s.nodes.contains_key(&k));
    s.put(sample(7, 10, 25, ProcessInfo::default()), &b)
        .expect("late scan cannot resurrect");
    assert_eq!(s.nodes[&k].record.lifecycle, Lifecycle::ExitObserved);
    s.scan_active = false;
    s.pins.insert(k, 1);
    s.collect(Duration::ZERO, true);
    assert!(s.nodes.contains_key(&k));
    s.pins.remove(&k);
    s.collect(Duration::ZERO, true);
    assert!(!s.nodes.contains_key(&k));
}
#[test]
fn an_interval_sample_can_supply_its_new_image_and_parent_after_invalidation() {
    let b = budget();
    let mut s = State::new();
    let k = key(7, 10);
    let mut old = sample(
        7,
        10,
        100,
        ProcessInfo {
            executable: Some(b"old".to_vec()),
            parent_pid: Some(1),
            parent_birth_boottime_ns: Some(2),
            ..Default::default()
        },
    );
    old.window.end_boottime_ns = 110;
    old.facts.exec_cookie = Some(1);
    s.put(old, &b).expect("old image");
    let mut new = sample(
        7,
        10,
        200,
        ProcessInfo {
            executable: Some(b"new".to_vec()),
            argv: Some(vec![b"new-argument".to_vec()]),
            parent_pid: Some(3),
            parent_birth_boottime_ns: Some(4),
            parent_executable: Some(b"new-parent".to_vec()),
            ..Default::default()
        },
    );
    new.window.end_boottime_ns = 210;
    new.facts.exec_cookie = Some(2);
    s.put(new, &b).expect("new interval sample");
    let latest = &s.nodes[&k].record.latest;
    assert_eq!(
        latest.process.executable.as_deref(),
        Some(b"new".as_slice())
    );
    assert_eq!(latest.process.exec_cookie, Some(2));
    assert_eq!(
        latest.process.parent_executable.as_deref(),
        Some(b"new-parent".as_slice())
    );
    assert_eq!(
        latest.process.argv.as_ref().expect("new arguments")[0],
        b"new-argument"
    );
}
#[tokio::test]
async fn public_queries_follow_exact_relations_and_bind_live_page_cursors() {
    let tree = ProcessTree::prepare(settings()).expect("tree");
    {
        let mut s = tree.state.write().expect("state");
        for (pid, birth, parent, parent_birth) in
            [(10, 1, 0, 0), (11, 2, 10, 1), (12, 3, 11, 2), (13, 4, 0, 0)]
        {
            let mut c = sample(
                pid,
                birth,
                10,
                ProcessInfo {
                    parent_pid: Some(parent),
                    parent_birth_boottime_ns: Some(parent_birth),
                    ..Default::default()
                },
            );
            c.key = tree.key(pid, birth);
            s.put(c, &tree.budget).expect("fixture node");
        }
        s.phase = TreePhase::Ready;
    }
    let page = || PageRequest {
        limit: std::num::NonZeroUsize::new(8).expect("page"),
        cursor: None,
    };
    let root = tree.key(10, 1);
    let child = tree.key(11, 2);
    let grandchild = tree.key(12, 3);
    let QueryReply::Page(children) = tree
        .query(ProcessQuery::Children {
            key: root,
            page: page(),
        })
        .await
        .expect("children")
    else {
        panic!("page")
    };
    assert_eq!(
        children.records.iter().map(|r| r.key).collect::<Vec<_>>(),
        vec![child]
    );
    let QueryReply::Ancestors {
        records,
        incomplete,
    } = tree
        .query(ProcessQuery::Ancestors {
            key: grandchild,
            limit: std::num::NonZeroUsize::new(8).expect("depth"),
        })
        .await
        .expect("ancestors")
    else {
        panic!("path")
    };
    assert!(!incomplete);
    assert_eq!(
        records.iter().map(|r| r.key).collect::<Vec<_>>(),
        vec![child, root]
    );
    let QueryReply::Page(subtree) = tree
        .query(ProcessQuery::Subtree {
            key: root,
            page: page(),
        })
        .await
        .expect("subtree")
    else {
        panic!("page")
    };
    assert_eq!(
        subtree.records.iter().map(|r| r.key).collect::<Vec<_>>(),
        vec![child, grandchild]
    );
    let QueryReply::Page(first) = tree
        .query(ProcessQuery::List {
            filter: ProcessFilter::default(),
            page: PageRequest {
                limit: std::num::NonZeroUsize::MIN,
                cursor: None,
            },
        })
        .await
        .expect("first page")
    else {
        panic!("page")
    };
    let cursor = first.next_cursor.expect("continuation");
    let QueryReply::Page(next) = tree
        .query(ProcessQuery::List {
            filter: ProcessFilter::default(),
            page: PageRequest {
                limit: std::num::NonZeroUsize::MIN,
                cursor: Some(cursor.clone()),
            },
        })
        .await
        .expect("next page")
    else {
        panic!("page")
    };
    assert_ne!(first.records[0].key, next.records[0].key);
    assert!(matches!(
        tree.query(ProcessQuery::List {
            filter: ProcessFilter {
                name: Some(b"other".to_vec()),
                ..Default::default()
            },
            page: PageRequest {
                limit: std::num::NonZeroUsize::MIN,
                cursor: Some(cursor)
            }
        })
        .await,
        Err(Error::CursorExpired)
    ));
    tree.close().await.expect("closed");
}
#[tokio::test]
async fn a_busy_periodic_refresh_does_not_invent_a_source_gap() {
    let mut settings = settings();
    settings.reconcile_interval_ms = std::num::NonZeroU64::MIN;
    let tree = ProcessTree::prepare(settings).expect("tree");
    tree.state.write().expect("state").phase = TreePhase::Ready;
    let scan = tree.scan.clone().lock_owned().await;
    let (stop, watch) = tokio::sync::watch::channel(false);
    let owner = tree.clone();
    let periodic = tokio::spawn(async move {
        owner.run_reconciliation(watch).await;
    });
    tokio::time::sleep(Duration::from_millis(5)).await;
    stop.send_replace(true);
    periodic.await.expect("periodic stopped");
    drop(scan);
    let status = tree.status().expect("status");
    assert_eq!(status.phase, TreePhase::Ready);
    assert!(!status.historical_gap);
    assert_eq!(status.evidence_epoch, 0);
    tree.close().await.expect("closed");
}
#[tokio::test]
async fn initialize_cannot_replace_an_already_bound_source() {
    let tree = ProcessTree::prepare(settings()).expect("tree");
    let source: Arc<dyn ProcessFactsPort> = Arc::new(LostBeforeScan);
    *tree.source.lock().expect("source") = Some(source.clone());
    tree.state.write().expect("state").epoch = 7;
    let marker = source.facts_barrier().await.expect("fixture marker");
    let start = ProcessTrackingStart {
        host_pid_namespace_inode: marker.host_pid_namespace_inode,
        tracking_epoch: 1,
        begin_marker: 0,
        loss_epoch: 0,
        backend_rejected: 0,
        cutoff_boottime_ns: marker.cutoff_boottime_ns,
    };
    assert!(matches!(
        tree.initialize(start, Arc::new(LostBeforeScan)).await,
        Err(Error::Conflict)
    ));
    assert_eq!(tree.status().expect("status").tracking_epoch, 7);
    assert!(Arc::ptr_eq(
        tree.source.lock().expect("source").as_ref().expect("bound"),
        &source
    ));
    tree.close().await.expect("closed");
}
#[test]
fn proc_coverage_parses_only_ascii_fields_not_arbitrary_mount_path_bytes() {
    let input=b"1 0 1:1 / /other-\xff rw - tmpfs tmpfs rw\n2 0 0:1 / /proc rw - proc proc rw,hidepid=off\n";
    crate::scan::validate_mountinfo(input).expect("unrelated path need not be UTF8");
    assert!(matches!(
        crate::scan::validate_mountinfo(b"2 0 0:1 / /proc rw - proc proc rw,hidepid=2\n"),
        Err(Error::Coverage)
    ));
}

fn process_event(
    pid: u32,
    birth: u64,
    time: u64,
    stage: as_types::dispatch_event::EventStage,
    operation: u32,
    process: ProcessInfo,
    payload: as_types::dispatch_event::ProcessData,
) -> EventData {
    thread_event(
        (pid, birth, pid, birth),
        time,
        stage,
        operation,
        process,
        payload,
    )
}
fn thread_event(
    identity: (u32, u64, u32, u64),
    time: u64,
    stage: as_types::dispatch_event::EventStage,
    operation: u32,
    process: ProcessInfo,
    payload: as_types::dispatch_event::ProcessData,
) -> EventData {
    let (pid, birth, tid, thread_birth) = identity;
    use as_kernel_common::event_frame::*;
    use zerocopy::IntoBytes;
    let header = EventFrameHeader {
        version: EVENT_FRAME_VERSION.into(),
        header_len: (EVENT_FRAME_HEADER_LEN as u16).into(),
        record_len: ((EVENT_FRAME_HEADER_LEN + 16) as u32).into(),
        kind: 1.into(),
        stage: (stage as u16).into(),
        operation: operation.into(),
        source_instance: [8; 16],
        sequence_domain: 1.into(),
        sequence: time.into(),
        source_boottime_ns: time.into(),
        actor_tgid: pid.into(),
        actor_tid: tid.into(),
        actor_birth_boottime_ns: birth.into(),
        actor_cgroup_id: 44.into(),
        available: (AVAILABLE_SOURCE_TIME
            | AVAILABLE_ACTOR_TGID
            | AVAILABLE_ACTOR_TID
            | AVAILABLE_ACTOR_BIRTH
            | AVAILABLE_ACTOR_CGROUP)
            .into(),
        common_process_len: 16.into(),
    };
    let mut bytes = header.as_bytes().to_vec();
    bytes.extend(25u16.to_le_bytes());
    bytes.extend(0u16.to_le_bytes());
    bytes.extend(8u32.to_le_bytes());
    bytes.extend(thread_birth.to_le_bytes());
    let mut event = as_types::dispatch_event::decode(&bytes).expect("valid source header");
    event.process = process;
    event.payload = EventPayload::Process(payload);
    event
}

#[tokio::test]
async fn callbacks_preserve_initial_latest_child_identity_and_real_exit() {
    use as_types::dispatch_event::{EventStage, ProcessData};
    let tree = ProcessTree::prepare(settings()).expect("tree");
    let before = process_event(
        10,
        100,
        10,
        EventStage::ControlledRequest,
        1,
        ProcessInfo {
            name: Some(b"creator".to_vec()),
            executable: Some(b"old".to_vec()),
            argv: Some(vec![b"old-arg".to_vec()]),
            kind: Some(ProcessKind::User),
            exec_cookie: Some(1),
            ..Default::default()
        },
        ProcessData {
            target_executable: Some(b"new".to_vec()),
            target_argv: Some(vec![b"target-arg".to_vec()]),
            ..Default::default()
        },
    );
    tree.observe(&before).expect("pre-exec");
    assert_eq!(
        tree.get_cached_process(tree.key(10, 100))
            .expect("actor")
            .record
            .latest
            .process
            .executable
            .as_deref(),
        Some(b"old".as_slice())
    );
    let executed = process_event(
        10,
        100,
        20,
        EventStage::Observation,
        1,
        ProcessInfo {
            argv: Some(vec![b"new-arg".to_vec()]),
            cwd: Some(b"/work".to_vec()),
            kind: Some(ProcessKind::User),
            exec_cookie: Some(2),
            pid_namespace_inode: Some(71),
            net_namespace_inode: Some(72),
            user_namespace_inode: Some(73),
            namespace_tgids: Some(vec![10, 1]),
            suid: Some(5),
            fsuid: Some(6),
            sgid: Some(7),
            fsgid: Some(8),
            ..Default::default()
        },
        ProcessData {
            target_executable: Some(b"new".to_vec()),
            ..Default::default()
        },
    );
    tree.observe(&executed).expect("post-exec");
    let actor = tree
        .get_cached_process(tree.key(10, 100))
        .expect("actor")
        .record;
    assert_eq!(
        actor.initial.process.executable.as_deref(),
        Some(b"old".as_slice())
    );
    assert_eq!(
        actor.latest.process.executable.as_deref(),
        Some(b"new".as_slice())
    );
    assert_eq!(actor.latest.process.argv, Some(vec![b"new-arg".to_vec()]));
    assert_eq!(actor.latest.process.exec_cookie, Some(2));
    assert_eq!(actor.latest.process.net_namespace_inode, Some(72));
    assert_eq!(actor.latest.process.namespace_tgids, Some(vec![10, 1]));
    assert_eq!(actor.latest.process.fsuid, Some(6));
    let mut creator_facts = actor.latest.process.clone();
    creator_facts.argv = None;
    let fork = process_event(
        10,
        100,
        30,
        EventStage::Observation,
        2,
        creator_facts,
        ProcessData {
            child_pid: Some(20),
            child_birth_boottime_ns: Some(200),
            child_parent_pid: Some(10),
            child_parent_birth_boottime_ns: Some(100),
            child_cgroup_id: Some(99),
            child_process: Some(Box::new(ProcessInfo {
                name: Some(b"child".to_vec()),
                executable: Some(b"new".to_vec()),
                kind: Some(ProcessKind::User),
                exec_cookie: Some(2),
                net_namespace_inode: Some(88),
                namespace_tgids: Some(vec![20, 2]),
                ..Default::default()
            })),
            ..Default::default()
        },
    );
    tree.observe(&fork).expect("child fact");
    let child = tree
        .get_cached_process(tree.key(20, 200))
        .expect("child")
        .record;
    assert_eq!(child.creator, Some(tree.key(10, 100)));
    assert_eq!(child.initial.cgroup_id, Some(99));
    assert_eq!(
        child.initial.process.name.as_deref(),
        Some(b"child".as_slice())
    );
    assert_eq!(child.initial.process.net_namespace_inode, Some(88));
    assert!(
        child.initial.process.argv.is_none(),
        "creator argv is not child argv"
    );
    assert_eq!(
        child.initial.process.parent_argv,
        Some(vec![b"new-arg".to_vec()])
    );
    assert_eq!(
        child.initial.stamps[&Field::ParentArgv]
            .window
            .end_boottime_ns,
        20
    );
    let exited = process_event(
        20,
        200,
        40,
        EventStage::Observation,
        3,
        ProcessInfo::default(),
        ProcessData {
            exit_code: Some(7 << 8),
            ..Default::default()
        },
    );
    tree.observe(&exited).expect("exit fact");
    let child = tree
        .get_cached_process(tree.key(20, 200))
        .expect("retained child")
        .record;
    assert_eq!(child.exit_code, Some(7 << 8));
    assert_eq!(child.lifecycle, Lifecycle::ExitObserved);
    assert_eq!(child.initial.process.net_namespace_inode, Some(88));
    tree.close().await.expect("closed");
}

#[tokio::test]
async fn cached_thread_attributes_require_the_same_tid_and_task_lifetime() {
    use as_types::dispatch_event::{EventStage, ProcessData};
    let tree = ProcessTree::prepare(settings()).expect("tree");
    tree.state.write().expect("state").phase = TreePhase::Ready;
    let source = thread_event(
        (10, 100, 11, 110),
        10,
        EventStage::ControlledRequest,
        1,
        ProcessInfo {
            uid: Some(1200),
            cwd: Some(b"/worker".to_vec()),
            net_namespace_inode: Some(71),
            argv: Some(vec![b"shared-mm".to_vec()]),
            ..Default::default()
        },
        ProcessData::default(),
    );
    tree.observe(&source).expect("worker sample");
    let same = thread_event(
        (10, 100, 11, 110),
        20,
        EventStage::ControlledRequest,
        1,
        ProcessInfo::default(),
        ProcessData::default(),
    );
    let same = FastFacts::prepare(&*tree, &same).expect("same task");
    assert_eq!(same.uid, Some(1200));
    for (tid, birth, time) in [(10, 100, 30), (11, 120, 40)] {
        let event = thread_event(
            (10, 100, tid, birth),
            time,
            EventStage::ControlledRequest,
            1,
            ProcessInfo::default(),
            ProcessData::default(),
        );
        let filled = FastFacts::prepare(&*tree, &event).expect("different task");
        assert!(filled.uid.is_none());
        assert!(filled.cwd.is_none());
        assert!(filled.net_namespace_inode.is_none());
        assert_eq!(filled.argv, Some(vec![b"shared-mm".to_vec()]));
    }
    let retained = tree
        .get_cached_process(tree.key(10, 100))
        .expect("retained source");
    assert_eq!(retained.record.latest.process.uid, Some(1200));
    assert!(matches!(
        retained.record.latest.stamps[&Field::Uid].origin,
        Origin::DriverEvent {
            actor_tid: Some(11),
            actor_thread_birth_boottime_ns: Some(110),
            ..
        }
    ));
    tree.close().await.expect("closed");
}
#[tokio::test]
async fn rejected_refresh_inputs_do_not_invalidate_existing_facts() {
    let tree = ProcessTree::prepare(settings()).expect("tree");
    tree.state.write().expect("state").phase = TreePhase::Ready;
    let wrong = ProcessKey {
        boot_id: [0; 16],
        host_tgid: 1,
        birth_boottime_ns: 1,
    };
    assert!(matches!(
        tree.refresh(RefreshTarget::Process { key: wrong }).await,
        Err(Error::Invalid)
    ));
    assert!(matches!(
        tree.refresh(RefreshTarget::System).await,
        Err(Error::Unavailable)
    ));
    *tree.source.lock().expect("source") = Some(Arc::new(LostBeforeScan));
    let zero_pid = ProcessKey {
        boot_id: tree.boot,
        host_tgid: 0,
        birth_boottime_ns: 1,
    };
    assert!(matches!(
        tree.refresh(RefreshTarget::Process { key: zero_pid }).await,
        Err(Error::Invalid)
    ));
    let status = tree.status().expect("status");
    assert_eq!(status.phase, TreePhase::Ready);
    assert!(!status.historical_gap);
    tree.close().await.expect("closed");
}

// Software identity tokens exercise targeted proc reads; they do not claim kernel provenance.
struct DetailProbe {
    cgroup: Option<u64>,
    allow_enumeration: bool,
    inspections: AtomicUsize,
    barriers: AtomicUsize,
    fail: std::sync::atomic::AtomicBool,
    block_once: std::sync::atomic::AtomicBool,
    entered: tokio::sync::Notify,
    resume: tokio::sync::Semaphore,
}
impl DetailProbe {
    fn new(fail: bool) -> Arc<Self> {
        Arc::new(Self {
            cgroup: Some(1),
            allow_enumeration: false,
            inspections: AtomicUsize::new(0),
            barriers: AtomicUsize::new(0),
            fail: std::sync::atomic::AtomicBool::new(fail),
            block_once: std::sync::atomic::AtomicBool::new(false),
            entered: tokio::sync::Notify::new(),
            resume: tokio::sync::Semaphore::new(0),
        })
    }
}
#[async_trait::async_trait]
impl ProcessFactsPort for DetailProbe {
    async fn inspect(&self, pids: &[u32]) -> Result<Vec<ProcessInspection>, ModuleError> {
        if !self.allow_enumeration {
            assert_eq!(pids, &[std::process::id()], "only the requested instance");
        }
        self.inspections.fetch_add(1, Ordering::AcqRel);
        let time = crate::scan::now_ns();
        Ok(pids
            .iter()
            .map(|pid| {
                if *pid != std::process::id() {
                    ProcessInspection::Gone { host_tgid: *pid }
                } else {
                    ProcessInspection::Bound {
                        binding: ProcessBinding {
                            host_tgid: std::process::id(),
                            birth_boottime_ns: 1,
                            parent_pid: 0,
                            parent_birth_boottime_ns: 0,
                            kind: ProcessKind::User,
                            exec_cookie: 1,
                            cgroup_id: self.cgroup,
                            window: SampleWindow {
                                begin_boottime_ns: time,
                                end_boottime_ns: time,
                            },
                        },
                    }
                }
            })
            .collect())
    }
    async fn facts_barrier(&self) -> Result<ProcessFactsBarrier, ModuleError> {
        self.barriers.fetch_add(1, Ordering::AcqRel);
        if self.block_once.swap(false, Ordering::AcqRel) {
            self.entered.notify_one();
            self.resume.acquire().await.expect("resume").forget();
        }
        tokio::task::yield_now().await;
        if self.fail.load(Ordering::Acquire) {
            return Err(ModuleError::Unavailable);
        }
        LostBeforeScan.facts_barrier().await
    }
}
fn detail_fixture(source: &Arc<DetailProbe>) -> (Arc<ProcessTree>, ProcessKey) {
    let tree = ProcessTree::prepare(settings()).expect("tree");
    let key = tree.key(std::process::id(), 1);
    *tree.source.lock().expect("source") = Some(source.clone());
    {
        let mut state = tree.state.write().expect("state");
        let mut candidate = sample(
            key.host_tgid,
            1,
            0,
            ProcessInfo {
                exec_cookie: Some(1),
                ..Default::default()
            },
        );
        candidate.key = key;
        candidate.cgroup = source.cgroup;
        candidate.origin = Origin::DriverEvent {
            event: EventKey {
                source_instance: [7; 16],
                sequence_domain: 1,
                sequence: 1,
            },
            actor_tid: Some(key.host_tgid),
            actor_thread_birth_boottime_ns: Some(1),
        };
        state.put(candidate, &tree.budget).expect("node");
        state.phase = TreePhase::Ready;
    }
    (tree, key)
}
#[tokio::test]
async fn system_scan_negative_fields_do_not_repeat_source_io_on_detail_get() {
    let mut source = DetailProbe::new(false);
    let probe = Arc::get_mut(&mut source).expect("exclusive fixture");
    probe.cgroup = None;
    probe.allow_enumeration = true;
    let (tree, key) = detail_fixture(&source);
    tree.refresh(RefreshTarget::System)
        .await
        .expect("full proc image");
    let details = tree.get_cached_process(key).expect("sampled node");
    assert!(details.record.latest.cgroup_id.is_none());
    let before = (
        source.barriers.load(Ordering::Acquire),
        source.inspections.load(Ordering::Acquire),
    );
    tree.get_process(key).await.expect("negative result cached");
    tree.query(ProcessQuery::Get { key })
        .await
        .expect("same cached detail");
    assert_eq!(
        (
            source.barriers.load(Ordering::Acquire),
            source.inspections.load(Ordering::Acquire)
        ),
        before
    );
    tree.close().await.expect("closed");
}
#[tokio::test]
async fn detail_reads_merge_concurrent_attempts_and_keep_cache_hits_io_free() {
    let source = DetailProbe::new(false);
    let (tree, key) = detail_fixture(&source);
    assert!(
        tree.get_cached_process(key)
            .expect("cached")
            .record
            .latest
            .process
            .mount_namespace_inode
            .is_none()
    );
    assert_eq!(source.barriers.load(Ordering::Acquire), 0);
    let (a, b, c) = tokio::join!(
        tree.get_process(key),
        tree.get_process(key),
        tree.query(ProcessQuery::Get { key })
    );
    let a = a.expect("details");
    let b = b.expect("same details");
    assert!(matches!(c.expect("query"), QueryReply::Details(_)));
    assert!(Arc::ptr_eq(&a.record.latest, &b.record.latest));
    let actual_mount = std::fs::read_link("/proc/self/ns/mnt").expect("mount oracle");
    let actual_mount = actual_mount
        .to_str()
        .expect("namespace ASCII")
        .strip_prefix("mnt:[")
        .and_then(|v| v.strip_suffix(']'))
        .expect("mount namespace shape")
        .parse::<u64>()
        .expect("inode");
    assert_eq!(
        a.record.latest.process.mount_namespace_inode,
        Some(actual_mount)
    );
    assert!(a.record.initial.process.mount_namespace_inode.is_none());
    assert_eq!(source.barriers.load(Ordering::Acquire), 2);
    assert_eq!(source.inspections.load(Ordering::Acquire), 2);
    {
        let mut state = tree.state.write().expect("state");
        let node = state.nodes.get_mut(&key).expect("node");
        node.detail_attempt = None;
        assert!(
            !node.needs_details(),
            "Workload and parent descriptions have no targeted producer"
        );
    }
    let cached = tree.get_process(key).await.expect("cache hit");
    assert!(Arc::ptr_eq(&a.record.latest, &cached.record.latest));
    assert_eq!(source.barriers.load(Ordering::Acquire), 2);
    // Kernel threads have no user image or argument vector to obtain.
    {
        let mut state = tree.state.write().expect("state");
        let mut facts = cached.record.latest.process.clone();
        facts.kind = Some(ProcessKind::Kernel);
        facts.executable = None;
        facts.argv = None;
        facts.cwd = None;
        let mut candidate = sample(key.host_tgid, 1, crate::scan::now_ns(), facts);
        candidate.key = key;
        candidate.executed = true;
        state.put(candidate, &tree.budget).expect("kernel fixture");
    }
    tree.get_process(key).await.expect("kernel cache");
    assert_eq!(source.barriers.load(Ordering::Acquire), 2);
    tree.close().await.expect("closed");
}
#[tokio::test]
async fn failed_detail_attempt_survives_caller_cancel_and_source_gaps() {
    let source = DetailProbe::new(true);
    source.block_once.store(true, Ordering::Release);
    let (tree, key) = detail_fixture(&source);
    let reader = tree.clone();
    let pending = tokio::spawn(async move { reader.get_process(key).await });
    source.entered.notified().await;
    pending.abort();
    assert!(pending.await.expect_err("caller canceled").is_cancelled());
    source.resume.add_permits(1);
    let (a, b) = tokio::join!(tree.get_process(key), tree.get_process(key));
    assert!(
        a.expect("retained details")
            .record
            .latest
            .process
            .name
            .is_none()
    );
    b.expect("shared failure cache");
    assert_eq!(source.barriers.load(Ordering::Acquire), 1);
    assert_eq!(tree.status().expect("status").evidence_epoch, 1);
    tree.source_gap();
    tree.get_process(key)
        .await
        .expect("source gaps do not retry detail I/O");
    assert_eq!(source.barriers.load(Ordering::Acquire), 1);
    assert!(tree.refresh(RefreshTarget::Process { key }).await.is_err());
    tree.get_process(key)
        .await
        .expect("explicit failed refresh cached");
    assert_eq!(source.barriers.load(Ordering::Acquire), 2);
    {
        let mut state = tree.state.write().expect("state");
        let mut candidate = sample(
            key.host_tgid,
            1,
            crate::scan::now_ns(),
            ProcessInfo::default(),
        );
        candidate.key = key;
        candidate.executed = true;
        state
            .put(candidate, &tree.budget)
            .expect("new successful exec fact");
    }
    tree.get_process(key).await.expect("new exec attempt");
    tree.get_process(key)
        .await
        .expect("failed exec attempt cached");
    assert_eq!(source.barriers.load(Ordering::Acquire), 3);
    tree.state.write().expect("state").retire(key, None, None);
    tree.get_process(key).await.expect("retained exited node");
    assert!(matches!(
        tree.get_process(tree.key(key.host_tgid, 2)).await,
        Err(Error::NotFound)
    ));
    assert_eq!(source.barriers.load(Ordering::Acquire), 3);
    assert_eq!(source.inspections.load(Ordering::Acquire), 0);
    tree.close().await.expect("closed");
}
#[tokio::test]
async fn alternating_failed_nodes_do_not_reopen_each_others_detail_attempts() {
    let source = DetailProbe::new(true);
    let (tree, a) = detail_fixture(&source);
    // A second software node: its barrier fails before any OS identity lookup.
    let b = tree.key(if a.host_tgid == 10 { 20 } else { 10 }, 1);
    {
        let mut state = tree.state.write().expect("state");
        let mut candidate = sample(b.host_tgid, 1, 0, ProcessInfo::default());
        candidate.key = b;
        state.put(candidate, &tree.budget).expect("second node");
    }
    for key in [a, b, a, b, a, b] {
        tree.get_process(key)
            .await
            .expect("retained failure details");
    }
    assert_eq!(source.barriers.load(Ordering::Acquire), 2);
    assert_eq!(source.inspections.load(Ordering::Acquire), 0);
    tree.source_gap();
    tree.get_process(a).await.expect("A remains attempted");
    tree.get_process(b).await.expect("B remains attempted");
    assert_eq!(source.barriers.load(Ordering::Acquire), 2);
    assert!(
        tree.refresh(RefreshTarget::Process { key: b })
            .await
            .is_err()
    );
    tree.get_process(a)
        .await
        .expect("A still cached after B refresh");
    tree.get_process(b).await.expect("B refresh failure cached");
    assert_eq!(source.barriers.load(Ordering::Acquire), 3);
    tree.close().await.expect("closed");
}
#[tokio::test]
async fn an_inflight_failed_detail_read_cannot_complete_a_new_execs_attempt() {
    let source = DetailProbe::new(true);
    source.block_once.store(true, Ordering::Release);
    let (tree, key) = detail_fixture(&source);
    let reader = tree.clone();
    let pending = tokio::spawn(async move { reader.get_process(key).await });
    source.entered.notified().await;
    {
        let mut state = tree.state.write().expect("state");
        let mut candidate = sample(
            key.host_tgid,
            1,
            crate::scan::now_ns(),
            ProcessInfo {
                exec_cookie: Some(2),
                ..Default::default()
            },
        );
        candidate.key = key;
        candidate.executed = true;
        state
            .put(candidate, &tree.budget)
            .expect("exec while read is pending");
        let mut older_read = sample(
            key.host_tgid,
            1,
            0,
            ProcessInfo {
                exec_cookie: Some(1),
                ..Default::default()
            },
        );
        older_read.key = key;
        state
            .put(older_read, &tree.budget)
            .expect("late old-image proc read");
        assert!(
            state.nodes[&key].detail_attempt.is_none(),
            "old-image scan cannot complete the new exec's attempt"
        );
    }
    source.resume.add_permits(1);
    pending.await.expect("reader").expect("retained details");
    assert!(
        tree.state.read().expect("state").nodes[&key]
            .detail_attempt
            .is_none()
    );
    tree.get_process(key).await.expect("new image attempt");
    tree.get_process(key)
        .await
        .expect("new image failure cached");
    assert_eq!(source.barriers.load(Ordering::Acquire), 2);
    assert_eq!(
        tree.get_cached_process(key)
            .expect("record")
            .record
            .latest
            .process
            .exec_cookie,
        Some(2)
    );
    tree.close().await.expect("closed");
}
#[tokio::test]
async fn empty_proc_argv_stays_in_details_but_does_not_supply_original_operation_argc() {
    let tree = ProcessTree::prepare(settings()).expect("tree");
    let parent = tree.key(10, 100);
    let child = tree.key(20, 200);
    {
        let mut state = tree.state.write().expect("state");
        for (key, time, parent_key) in [(parent, 10, None), (child, 20, Some(parent))] {
            let mut candidate = sample(
                key.host_tgid,
                key.birth_boottime_ns,
                time,
                ProcessInfo {
                    argv: Some(Vec::new()),
                    parent_pid: parent_key.map(|p| p.host_tgid),
                    parent_birth_boottime_ns: parent_key.map(|p| p.birth_boottime_ns),
                    ..Default::default()
                },
            );
            candidate.key = key;
            state.put(candidate, &tree.budget).expect("proc sample");
        }
        state.phase = TreePhase::Ready;
    }
    let event = process_event(
        20,
        200,
        30,
        as_types::dispatch_event::EventStage::ControlledRequest,
        1,
        ProcessInfo::default(),
        Default::default(),
    );
    let supplement = FastFacts::prepare(&*tree, &event).expect("cache supplement");
    assert!(supplement.argv.is_none());
    assert!(supplement.parent_argv.is_none());
    let details = tree.get_cached_process(child).expect("details");
    assert_eq!(details.record.latest.process.argv, Some(Vec::new()));
    assert_eq!(details.record.latest.process.parent_argv, Some(Vec::new()));
    tree.close().await.expect("closed");
}
