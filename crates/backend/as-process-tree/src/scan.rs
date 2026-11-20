use crate::{
    Error, Field, FieldState, ProcessTree, ScanReceipt, TreePhase, model::Origin, store::Candidate,
};
use as_types::{dispatch_event::ProcessInfo, process_source::*, process_tree::*};
use std::{
    collections::{BTreeMap, HashSet},
    fs::File,
    io::Read,
    os::{fd::AsRawFd, unix::ffi::OsStrExt},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
pub(crate) fn now_ns() -> u64 {
    let t = rustix::time::clock_gettime(rustix::time::ClockId::Boottime);
    (t.tv_sec as u64)
        .saturating_mul(1_000_000_000)
        .saturating_add(t.tv_nsec as u64)
}
pub(crate) fn boot_id() -> Result<[u8; 16], Error> {
    let text = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?;
    let hex = text.trim().replace('-', "");
    if hex.len() != 32 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::Coverage);
    }
    let mut id = [0; 16];
    for (i, b) in id.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).map_err(|_| Error::Coverage)?;
    }
    Ok(id)
}
fn coverage(namespace: u64) -> Result<(), Error> {
    let root = File::open("/proc")?;
    if rustix::fs::fstatfs(&root)
        .map_err(std::io::Error::from)?
        .f_type as u64
        != 0x9fa0
    {
        return Err(Error::Coverage);
    }
    let link = std::fs::read_link("/proc/self/ns/pid")?;
    let s = link.to_str().ok_or(Error::Coverage)?;
    let ns = s
        .strip_prefix("pid:[")
        .and_then(|v| v.strip_suffix(']'))
        .and_then(|v| v.parse::<u64>().ok())
        .ok_or(Error::Coverage)?;
    if ns != namespace {
        return Err(Error::Coverage);
    }
    validate_mountinfo(&std::fs::read("/proc/self/mountinfo")?)
}
pub(crate) fn validate_mountinfo(bytes: &[u8]) -> Result<(), Error> {
    fn fields(line: &[u8]) -> impl Iterator<Item = &[u8]> {
        line.split(|b| b.is_ascii_whitespace())
            .filter(|v| !v.is_empty())
    }
    let line = bytes
        .split(|b| *b == b'\n')
        .find(|line| fields(line).nth(4) == Some(b"/proc".as_slice()))
        .ok_or(Error::Coverage)?;
    if fields(line)
        .flat_map(|v| v.split(|b| *b == b','))
        .any(|v| v.starts_with(b"hidepid=") && v != b"hidepid=0" && v != b"hidepid=off")
    {
        return Err(Error::Coverage);
    }
    Ok(())
}
impl ProcessTree {
    pub(crate) async fn scan(
        self: &Arc<Self>,
        target: RefreshTarget,
    ) -> Result<ScanReceipt, Error> {
        let source = self
            .source
            .lock()
            .map_err(|_| Error::Unavailable)?
            .clone()
            .ok_or(Error::Unavailable)?;
        let before = source.facts_barrier().await?;
        let gap_serial = self
            .state
            .read()
            .map_err(|_| Error::Unavailable)?
            .gap_serial;
        let namespace = before.host_pid_namespace_inode;
        tokio::task::spawn_blocking(move || coverage(namespace))
            .await
            .map_err(|_| Error::Unavailable)??;
        let wanted = match target {
            RefreshTarget::System => None,
            RefreshTarget::Process { key } => {
                if key.boot_id != self.boot {
                    return Err(Error::Invalid);
                }
                Some(key)
            }
        };
        let pids = if let Some(key) = wanted {
            vec![key.host_tgid]
        } else {
            tokio::task::spawn_blocking(|| -> Result<Vec<u32>, Error> {
                let mut pids = Vec::new();
                for item in std::fs::read_dir("/proc")? {
                    let item = item?;
                    if let Some(pid) = item
                        .file_name()
                        .to_str()
                        .and_then(|v| v.parse::<u32>().ok())
                        && pid != 0
                    {
                        pids.push(pid)
                    }
                }
                pids.sort_unstable();
                Ok(pids)
            })
            .await
            .map_err(|_| Error::Unavailable)??
        };
        let _list_charge = self
            .budget
            .take(pids.len().saturating_mul(64).saturating_add(1024))?;
        let mut seen = HashSet::new();
        for chunk in pids.chunks(64) {
            let bindings = source.inspect(chunk).await?;
            for inspection in bindings {
                let ProcessInspection::Bound {
                    binding: before_read,
                } = inspection
                else {
                    if matches!(inspection, ProcessInspection::Inaccessible { .. }) {
                        return Err(Error::Coverage);
                    }
                    continue;
                };
                let key = self.key(before_read.host_tgid, before_read.birth_boottime_ns);
                if wanted.is_some_and(|k| k != key) {
                    continue;
                }
                let pid = before_read.host_tgid;
                let budget = self.budget.clone();
                let read = tokio::task::spawn_blocking(move || read_process(pid, budget))
                    .await
                    .map_err(|_| Error::Unavailable)?;
                let Some(ReadProcess {
                    facts,
                    leader,
                    states,
                    _charge: _candidate_charge,
                }) = (match read {
                    Ok(v) => Some(v),
                    Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => None,
                    Err(e) => return Err(e),
                })
                else {
                    continue;
                };
                let final_read = source.inspect(&[pid]).await?;
                let Some(ProcessInspection::Bound {
                    binding: after_read,
                }) = final_read.first()
                else {
                    continue;
                };
                if after_read.birth_boottime_ns != before_read.birth_boottime_ns
                    || after_read.exec_cookie != before_read.exec_cookie
                {
                    continue;
                }
                let mut facts = facts;
                facts.parent_pid = Some(after_read.parent_pid);
                facts.parent_birth_boottime_ns = Some(after_read.parent_birth_boottime_ns);
                facts.kind = Some(after_read.kind);
                facts.exec_cookie = Some(after_read.exec_cookie);
                facts.workload = after_read
                    .cgroup_id
                    .and_then(|group| source.workload(group, after_read.window.end_boottime_ns));
                let window = SampleWindow {
                    begin_boottime_ns: before_read.window.begin_boottime_ns,
                    end_boottime_ns: after_read.window.end_boottime_ns,
                };
                let mut state = self.state.write().map_err(|_| Error::Unavailable)?;
                state.collect(
                    Duration::from_millis(self.settings.retired_retention_ms.get()),
                    false,
                );
                let origin = if state.phase == TreePhase::Building {
                    Origin::StartupScan
                } else {
                    Origin::Refresh
                };
                state.put(
                    Candidate {
                        key,
                        facts,
                        cgroup: after_read.cgroup_id,
                        leader,
                        window,
                        origin,
                        executed: false,
                        states,
                    },
                    &self.budget,
                )?;
                seen.insert(key);
            }
        }
        // Recheck absent old instances by exact native identity, never infer exit from one proc miss.
        let missing = {
            let s = self.state.read().map_err(|_| Error::Unavailable)?;
            s.nodes
                .values()
                .filter(|n| {
                    n.record.lifecycle == Lifecycle::Present
                        && !seen.contains(&n.record.key)
                        && wanted.is_none_or(|key| key == n.record.key)
                })
                .map(|n| n.record.key)
                .collect::<Vec<_>>()
        };
        let mut confirmed_gone = Vec::new();
        for keys in missing.chunks(64) {
            let ids = keys.iter().map(|k| k.host_tgid).collect::<Vec<_>>();
            let actual = source.inspect(&ids).await?;
            for (key, found) in keys.iter().zip(actual) {
                if !matches!(found,ProcessInspection::Bound{ref binding} if binding.birth_boottime_ns==key.birth_boottime_ns)
                {
                    if matches!(found, ProcessInspection::Inaccessible { .. }) {
                        return Err(Error::Coverage);
                    }
                    confirmed_gone.push(*key);
                }
            }
        }
        let after = source.facts_barrier().await?;
        if before.tracking_epoch != after.tracking_epoch
            || before.loss_epoch != after.loss_epoch
            || before.backend_rejected != after.backend_rejected
        {
            eprintln!(
                "AS_PROCESS_TREE_SCAN_CONFLICT stage=source_fence before={before:?} after={after:?}"
            );
            return Err(Error::Conflict);
        }
        let mut state = self.state.write().map_err(|_| Error::Unavailable)?;
        if state.phase == TreePhase::Closed {
            return Err(Error::Stopped);
        }
        if state.gap_serial != gap_serial
            || state.initial_expected.is_some_and(|expected| {
                expected
                    != (
                        after.tracking_epoch,
                        after.loss_epoch,
                        after.backend_rejected,
                    )
                    || state.gap
            })
        {
            let actual = (state.gap_serial, state.initial_expected, state.gap);
            drop(state);
            eprintln!(
                "AS_PROCESS_TREE_SCAN_CONFLICT stage=tree_state started_gap_serial={gap_serial} actual={actual:?} after={after:?}"
            );
            return Err(Error::Conflict);
        }
        for key in confirmed_gone {
            state.retire(key, None, None);
        }
        if wanted.is_none() {
            state.phase = TreePhase::Ready;
            state.initial_expected = None;
        }
        state.marker = after.marker;
        state.loss = after.loss_epoch;
        state.rejected = after.backend_rejected;
        Ok(ScanReceipt {
            status: self.status_locked(&state),
            scanned: seen.len(),
            window: SampleWindow {
                begin_boottime_ns: before.cutoff_boottime_ns,
                end_boottime_ns: after.cutoff_boottime_ns,
            },
        })
    }
}
fn read_limited(path: PathBuf, limit: usize) -> Result<(Option<Vec<u8>>, FieldState), Error> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            return Ok((None, FieldState::Inaccessible));
        }
        Err(e) => return Err(e.into()),
    };
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        Ok((None, FieldState::Truncated))
    } else {
        Ok((Some(bytes), FieldState::Known))
    }
}
struct ReadProcess {
    facts: ProcessInfo,
    leader: Option<u8>,
    states: BTreeMap<Field, FieldState>,
    _charge: Arc<crate::model::Charge>,
}
fn read_process(pid: u32, budget: Arc<crate::model::Budget>) -> Result<ReadProcess, Error> {
    let limit = (budget.max / 32).clamp(4096, 1024 * 1024);
    let temporary = budget.take(limit.saturating_mul(4).saturating_add(8192))?;
    let directory = File::open(format!("/proc/{pid}"))?;
    if !directory.metadata()?.is_dir() {
        return Err(Error::Coverage);
    }
    let base = PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd()));
    let mut facts = ProcessInfo::default();
    let mut leader = None;
    let mut states = BTreeMap::new();
    let (bytes, state) = read_limited(base.join("comm"), limit)?;
    if let Some(bytes) = bytes {
        facts.name = Some(bytes.strip_suffix(b"\n").unwrap_or(&bytes).to_vec());
    } else {
        states.insert(Field::Name, state);
    }
    for (field, name, tag) in [
        (&mut facts.executable, "exe", Field::Executable),
        (&mut facts.cwd, "cwd", Field::Cwd),
    ] {
        match std::fs::read_link(base.join(name)) {
            Ok(path) => *field = Some(path.as_os_str().as_bytes().to_vec()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                states.insert(tag, FieldState::Missing);
            }
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                states.insert(tag, FieldState::Inaccessible);
            }
            Err(e) => return Err(e.into()),
        }
    }
    for (field, name, tag) in [
        (&mut facts.pid_namespace_inode, "pid", Field::PidNamespace),
        (&mut facts.net_namespace_inode, "net", Field::NetNamespace),
        (
            &mut facts.user_namespace_inode,
            "user",
            Field::UserNamespace,
        ),
        (
            &mut facts.mount_namespace_inode,
            "mnt",
            Field::MountNamespace,
        ),
    ] {
        match std::fs::read_link(base.join("ns").join(name)) {
            Ok(link) => {
                if link.as_os_str().is_empty() {
                    states.insert(tag, FieldState::Missing);
                    continue;
                }
                let text = link.to_str().ok_or(Error::Invalid)?;
                let number = text
                    .strip_prefix(name)
                    .and_then(|v| v.strip_prefix(":["))
                    .and_then(|v| v.strip_suffix(']'))
                    .ok_or(Error::Invalid)?;
                *field = Some(number.parse().map_err(|_| Error::Invalid)?);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                states.insert(tag, FieldState::Missing);
            }
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                states.insert(tag, FieldState::Inaccessible);
            }
            Err(e) => return Err(e.into()),
        }
    }
    let (bytes, state) = read_limited(base.join("cmdline"), limit)?;
    if let Some(bytes) = bytes {
        match parse_cmdline(&bytes, limit / 32) {
            Ok(argv) => facts.argv = Some(argv),
            Err(reason) => {
                states.insert(Field::Argv, reason);
            }
        }
    } else {
        states.insert(Field::Argv, state);
    }
    let (bytes, state) = read_limited(base.join("status"), limit)?;
    if let Some(bytes) = bytes {
        leader = parse_status(&bytes, &mut facts)?;
    } else {
        for field in [
            Field::Uid,
            Field::Euid,
            Field::Suid,
            Field::Fsuid,
            Field::Gid,
            Field::Egid,
            Field::Sgid,
            Field::Fsgid,
            Field::NamespaceTgids,
            Field::LeaderState,
        ] {
            states.insert(field, state);
        }
    }
    drop(temporary);
    let charge = budget.take(crate::store::facts_bytes(&facts).saturating_add(2048))?;
    Ok(ReadProcess {
        facts,
        leader,
        states,
        _charge: charge,
    })
}

pub(crate) fn parse_cmdline(
    bytes: &[u8],
    max_arguments: usize,
) -> Result<Vec<Vec<u8>>, FieldState> {
    // A successful empty read is an acquired empty value, not a failed read.
    // Its procfs provenance does not prove that an original operation had argc=0.
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    if bytes.last() != Some(&0) || bytes.iter().filter(|b| **b == 0).count() > max_arguments {
        return Err(FieldState::Truncated);
    }
    Ok(bytes[..bytes.len() - 1]
        .split(|b| *b == 0)
        .map(<[u8]>::to_vec)
        .collect())
}

pub(crate) fn parse_status(bytes: &[u8], facts: &mut ProcessInfo) -> Result<Option<u8>, Error> {
    let mut leader = None;
    for line in bytes.split(|b| *b == b'\n') {
        if let Some(rest) = line.strip_prefix(b"Uid:") {
            let [real, effective, saved, filesystem] = ascii_ids(rest)?;
            facts.uid = Some(real);
            facts.euid = Some(effective);
            facts.suid = Some(saved);
            facts.fsuid = Some(filesystem);
        }
        if let Some(rest) = line.strip_prefix(b"Gid:") {
            let [real, effective, saved, filesystem] = ascii_ids(rest)?;
            facts.gid = Some(real);
            facts.egid = Some(effective);
            facts.sgid = Some(saved);
            facts.fsgid = Some(filesystem);
        }
        if let Some(rest) = line.strip_prefix(b"NStgid:") {
            let text = std::str::from_utf8(rest).map_err(|_| Error::Invalid)?;
            let ids = text
                .split_whitespace()
                .map(|part| part.parse::<u32>().map_err(|_| Error::Invalid))
                .collect::<Result<Vec<_>, _>>()?;
            if ids.is_empty() || ids.contains(&0) {
                return Err(Error::Invalid);
            }
            facts.namespace_tgids = Some(ids);
        }
        if let Some(rest) = line.strip_prefix(b"State:") {
            leader = rest.iter().find(|b| !b.is_ascii_whitespace()).copied();
        }
    }
    Ok(leader)
}
fn ascii_ids(bytes: &[u8]) -> Result<[u32; 4], Error> {
    let text = std::str::from_utf8(bytes).map_err(|_| Error::Invalid)?;
    let mut parts = text.split_whitespace();
    let mut ids = [0; 4];
    for id in &mut ids {
        *id = parts
            .next()
            .ok_or(Error::Invalid)?
            .parse()
            .map_err(|_| Error::Invalid)?;
    }
    if parts.next().is_some() {
        return Err(Error::Invalid);
    }
    Ok(ids)
}
