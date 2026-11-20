//! Passive runtime source resources owned by the authenticated native session.
pub(crate) mod capture;
mod metadata;
pub(crate) mod wire;

use super::Shared;
use as_types::{
    main_ports::ModuleError,
    process_source::{ProcessBinding, ProcessInspection, ProcessKind},
};
use std::{
    collections::HashMap,
    fs::File,
    os::unix::fs::MetadataExt,
    sync::{Arc, Mutex},
    time::Instant,
};

/// Preparing or completed metadata checks of live exec instances; never target contexts.
#[derive(Default)]
pub(crate) struct Resources {
    preparations: Mutex<HashMap<u32, (u64, u64)>>,
}

enum PreparationOutcome {
    NoProbeSymbols,
    Installed,
}

impl Resources {
    pub(crate) fn forget(&self, pid: u32, birth: u64) {
        if let Ok(mut entries) = self.preparations.lock()
            && entries.get(&pid).is_some_and(|value| value.0 == birth)
        {
            entries.remove(&pid);
        }
    }
    pub(crate) fn observe(&self, shared: &Arc<Shared>, records: &[ProcessInspection]) {
        for record in records {
            let ProcessInspection::Bound { binding } = record else {
                continue;
            };
            if binding.kind != ProcessKind::User {
                continue;
            }
            let Ok(mut entries) = self.preparations.lock() else {
                return;
            };
            let identity = (binding.birth_boottime_ns, binding.exec_cookie);
            if entries.get(&binding.host_tgid) == Some(&identity) {
                continue;
            }
            entries.insert(binding.host_tgid, identity);
            drop(entries);
            let admission = match shared.retain_admission() {
                Ok(admission) => admission,
                Err(error) => {
                    self.preparation_completed(binding, &Err(error));
                    continue;
                }
            };
            let owner = shared.clone();
            let binding = binding.clone();
            shared.work.spawn(async move {
                let _admission = admission;
                let started = Instant::now();
                tracing::debug!(pid=binding.host_tgid,birth=binding.birth_boottime_ns,exec_cookie=binding.exec_cookie,phase="metadata_started","runtime source preparation");
                let result = prepare(&owner, &binding).await;
                owner.runtime_observation.preparation_completed(&binding, &result);
                match result {
                    Ok(PreparationOutcome::Installed) => tracing::debug!(pid=binding.host_tgid,birth=binding.birth_boottime_ns,exec_cookie=binding.exec_cookie,phase="installed",elapsed_ms=started.elapsed().as_millis(),"runtime source preparation"),
                    Ok(PreparationOutcome::NoProbeSymbols) => {},
                    Err(error) => tracing::warn!(pid=binding.host_tgid,birth=binding.birth_boottime_ns,exec_cookie=binding.exec_cookie,phase="gap",elapsed_ms=started.elapsed().as_millis(),%error,"runtime source preparation"),
                }
            });
        }
    }
    fn preparation_completed(
        &self,
        binding: &ProcessBinding,
        result: &Result<PreparationOutcome, ModuleError>,
    ) {
        if result.is_ok() {
            return;
        }
        if let Ok(mut entries) = self.preparations.lock()
            && entries.get(&binding.host_tgid)
                == Some(&(binding.birth_boottime_ns, binding.exec_cookie))
        {
            entries.remove(&binding.host_tgid);
        }
    }
    pub(crate) fn inspect_later(&self, shared: &Arc<Shared>, pid: u32) {
        let Ok(admission) = shared.retain_admission() else {
            return;
        };
        let owner = shared.clone();
        shared.work.spawn(async move {
            let _admission = admission;
            match owner.inspect(&[pid]).await {
                Ok(records) => owner.runtime_observation.observe(&owner, &records),
                Err(error) => tracing::warn!(pid,%error,"runtime exec inspection gap"),
            }
        });
    }
}

fn value(meta: &metadata::Metadata, name: &str) -> Option<u64> {
    match name {
        "UV_REQ_RESULT" => meta.uv.get("uv_fs_s.result").copied().map(u64::from),
        "UV_REQ_WORK" => meta.uv.get("uv_fs_s.work_req").copied().map(u64::from),
        other => {
            let symbol = other
                .strip_prefix("V8_")
                .map(|name| format!("v8dbg_{name}"));
            meta.runtime
                .get(symbol.as_deref().unwrap_or(other))
                .copied()
        }
    }
}

fn plan(
    binding: &ProcessBinding,
    file: &File,
    meta: &metadata::Metadata,
) -> Result<Vec<u8>, ModuleError> {
    let identity = file.metadata().map_err(|_| ModuleError::Unavailable)?;
    let names: Vec<_> = include_str!("../../../../drivers/linux/as_runtime_fields.h")
        .lines()
        .filter_map(|line| line.strip_prefix("AS_RUNTIME_FIELD(")?.strip_suffix(')'))
        .collect();
    let mut available = vec![0_u64; names.len().div_ceil(64)];
    let mut fields = vec![0_u64; names.len()];
    for (index, name) in names.iter().enumerate() {
        if let Some(value) = value(meta, name) {
            available[index / 64] |= 1_u64 << (index % 64);
            fields[index] = value;
        }
    }
    let mut output = Vec::new();
    output.extend(binding.host_tgid.to_le_bytes());
    output.extend(meta.machine.to_le_bytes());
    let present = meta
        .probes
        .iter()
        .enumerate()
        .fold(0_u16, |mask, (index, value)| {
            mask | u16::from(value.is_some()) << index
        });
    output.extend(present.to_le_bytes());
    for value in [
        binding.birth_boottime_ns,
        binding.exec_cookie,
        identity.ino(),
    ] {
        output.extend(value.to_le_bytes());
    }
    output.extend(rustix::fs::major(identity.dev()).to_le_bytes());
    output.extend(rustix::fs::minor(identity.dev()).to_le_bytes());
    for value in meta.probes {
        output.extend(value.unwrap_or(0).to_le_bytes());
    }
    output.extend((fields.len() as u32).to_le_bytes());
    output.extend(0_u32.to_le_bytes());
    for value in available.into_iter().chain(fields) {
        output.extend(value.to_le_bytes());
    }
    Ok(output)
}

async fn prepare(
    shared: &Arc<Shared>,
    binding: &ProcessBinding,
) -> Result<PreparationOutcome, ModuleError> {
    let pid = binding.host_tgid;
    let prepared = shared
        .work
        .spawn_blocking(move || {
            let mut file =
                File::open(format!("/proc/{pid}/exe")).map_err(|_| ModuleError::Unavailable)?;
            let meta = metadata::read(&mut file)?;
            Ok::<_, ModuleError>(meta.map(|meta| (file, meta)))
        })
        .await
        .map_err(|_| ModuleError::Unavailable)??;
    let inspected = shared.inspect(&[pid]).await?;
    if !inspected.iter().any(|record|matches!(record,ProcessInspection::Bound{binding:current}if current.birth_boottime_ns==binding.birth_boottime_ns&&current.exec_cookie==binding.exec_cookie)) {
        return Err(ModuleError::Conflict);
    }
    let Some((file, meta)) = prepared else {
        tracing::debug!(
            pid,
            "runtime source has no defined libuv probe symbols; nothing installed"
        );
        return Ok(PreparationOutcome::NoProbeSymbols);
    };
    tracing::debug!(
        pid = binding.host_tgid,
        birth = binding.birth_boottime_ns,
        exec_cookie = binding.exec_cookie,
        phase = "metadata_ready",
        probes = meta.probes.iter().filter(|probe| probe.is_some()).count(),
        "runtime source preparation"
    );
    for gap in &meta.gaps {
        tracing::warn!(
            pid,
            birth = binding.birth_boottime_ns,
            exec_cookie = binding.exec_cookie,
            gap,
            "runtime metadata incomplete; available native probes retained"
        );
    }
    let input = plan(binding, &file, &meta)?;
    if !shared.command(17, input).await?.is_empty() {
        return Err(ModuleError::InvalidInput);
    }
    Ok(PreparationOutcome::Installed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use as_types::process_source::SampleWindow;

    #[test]
    fn completed_no_probe_check_survives_queries_and_old_completion_cannot_erase_new_exec() {
        let resources = Resources::default();
        let mut binding = ProcessBinding {
            host_tgid: 40,
            birth_boottime_ns: 100,
            parent_pid: 1,
            parent_birth_boottime_ns: 1,
            kind: ProcessKind::User,
            exec_cookie: 2,
            cgroup_id: None,
            window: SampleWindow {
                begin_boottime_ns: 100,
                end_boottime_ns: 101,
            },
        };
        resources
            .preparations
            .lock()
            .expect("lock")
            .insert(40, (100, 2));
        resources.preparation_completed(&binding, &Ok(PreparationOutcome::NoProbeSymbols));
        assert_eq!(
            resources.preparations.lock().expect("lock").get(&40),
            Some(&(100, 2))
        );

        resources
            .preparations
            .lock()
            .expect("lock")
            .insert(40, (100, 3));
        resources.preparation_completed(&binding, &Err(ModuleError::Unavailable));
        assert_eq!(
            resources.preparations.lock().expect("lock").get(&40),
            Some(&(100, 3))
        );

        binding.exec_cookie = 3;
        resources.preparation_completed(&binding, &Err(ModuleError::Unavailable));
        assert!(
            !resources
                .preparations
                .lock()
                .expect("lock")
                .contains_key(&40)
        );
        resources
            .preparations
            .lock()
            .expect("lock")
            .insert(40, (200, 1));
        resources.forget(40, 100);
        assert_eq!(
            resources.preparations.lock().expect("lock").get(&40),
            Some(&(200, 1))
        );
        resources.forget(40, 200);
        assert!(resources.preparations.lock().expect("lock").is_empty());
    }
}
