//! Read-only discovery and live-object proof for the default local Docker runtime.
use crate::Shared;
use as_types::{
    dispatch_event::EventData,
    main_ports::ModuleError,
    process_source::{ProcessBinding, ProcessInspection},
    workload::{ContainerInstance, Workload},
};
use bytes::Bytes;
use http_body_util::{BodyExt, Empty, Limited};
use hyper::{Request, client::conn::http1};
use hyper_util::rt::TokioIo;
use rustix::{
    event::{PollFd, PollFlags, poll},
    fs::{self, Mode, OFlags, ResolveFlags},
    process::{Pid, PidfdFlags, pidfd_open},
};
use serde::{Deserialize, de::DeserializeOwned};
use std::{
    collections::HashMap,
    fs::File,
    io::Read,
    os::{
        fd::OwnedFd,
        unix::fs::{FileTypeExt, MetadataExt},
    },
    path::{Component, Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};
use tokio::{io::AsyncReadExt, net::UnixStream, process::Command, sync::watch};

const DOCKER_SOCKET: &str = "/run/docker.sock";
const RUNC_ROOT: &str = "/run/docker/runtime-runc/moby";
const CGROUP_ROOT: &str = "/sys/fs/cgroup";
const CGROUP2_MAGIC: u64 = 0x6367_7270;
const MAX_DOCUMENT: usize = 1024 * 1024;

pub(crate) type Bindings = Arc<HashMap<u64, Arc<Binding>>>;

pub(crate) struct Binding {
    pub instance: ContainerInstance,
    pub cgroup: File,
    pub net: File,
    pub cgroup_id: u64,
    pub init: ProcessBinding,
    pidfd: OwnedFd,
}

impl Binding {
    pub(crate) fn live(&self) -> bool {
        live(&self.pidfd)
    }

    pub(crate) fn matches(&self, group: u64, observed: u64) -> bool {
        self.cgroup_id == group && observed >= self.init.window.end_boottime_ns && self.live()
    }
}

pub(crate) fn enrich(bindings: &Bindings, event: &mut EventData) {
    if let Some(workload) = event_workload(bindings, event) {
        event.process.workload = Some(workload);
    }
}

pub(crate) fn event_workload(bindings: &Bindings, event: &EventData) -> Option<Workload> {
    if event.process.workload.is_some() {
        return None;
    }
    let header = event.header();
    workload(
        bindings,
        header.actor_cgroup_id?,
        header.source_boottime_ns?,
    )
}

pub(crate) fn workload(bindings: &Bindings, group: u64, observed: u64) -> Option<Workload> {
    let binding = bindings.get(&group)?;
    binding
        .matches(group, observed)
        .then(|| Workload::Container(binding.instance.clone()))
}

pub(crate) fn outside_containers(
    bindings: &Bindings,
    instances: &[ContainerInstance],
    lineage: &[u64],
    observed: u64,
) -> bool {
    if lineage.is_empty() || instances.is_empty() {
        return false;
    }
    // A complete one-item chain is the real hierarchy root. A default Docker
    // container uses a child cgroup even when its selected object has retired.
    if lineage.len() == 1 && instances.iter().all(local_instance) {
        return true;
    }
    instances.iter().all(|instance| {
        bindings.values().any(|binding| {
            binding.instance == *instance
                && !lineage.contains(&binding.cgroup_id)
                && binding.matches(binding.cgroup_id, observed)
        })
    })
}

pub(crate) fn start(
    owner: Arc<Shared>,
    initial: tokio::sync::oneshot::Sender<()>,
    lifetime: tokio_util::task::task_tracker::TaskTrackerToken,
) {
    let stop = owner.admission_stop.subscribe();
    tokio::task::spawn_blocking(move || {
        // A native file or connect hook may wait for the fast callback pump.
        // It must remain independent of the thread performing discovery I/O.
        if let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            runtime.block_on(reconcile(owner, initial, stop, lifetime));
        }
    });
}

async fn reconcile(
    owner: Arc<Shared>,
    initial: tokio::sync::oneshot::Sender<()>,
    mut stop: watch::Receiver<bool>,
    _lifetime: tokio_util::task::task_tracker::TaskTrackerToken,
) {
    let mut initial = Some(initial);
    let mut tick = tokio::time::interval(Duration::from_secs(2));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! { biased;
            _ = stop.wait_for(|stop| *stop) => break,
            _ = tick.tick() => {},
        }
        let before = owner.container_snapshot();
        // The scan owns any started runtime child through its actual wait result.
        let next = scan(&owner, &before, &mut stop).await;
        if *stop.borrow() {
            break;
        }
        if let Ok(next) = next {
            let same = before.len() == next.len()
                && next
                    .iter()
                    .all(|(key, b)| before.get(key).is_some_and(|a| Arc::ptr_eq(a, b)));
            if !same {
                owner.containers.send_replace(Arc::new(next));
            }
        } else if matches!(
            next,
            Err(ModuleError::Conflict | ModuleError::Unauthenticated)
        ) {
            if !before.is_empty() {
                owner.containers.send_replace(Arc::default());
            }
        } else {
            let retired: Vec<_> = before
                .iter()
                .filter_map(|(&id, item)| (!item.live()).then_some(id))
                .collect();
            if !retired.is_empty() {
                let mut remaining = before;
                let bindings = Arc::make_mut(&mut remaining);
                for id in retired {
                    bindings.remove(&id);
                }
                owner.containers.send_replace(remaining);
            }
        }
        if let Some(ready) = initial.take() {
            let _ = ready.send(());
        }
    }
    owner.containers.send_replace(Arc::default());
}

#[derive(Deserialize)]
struct Listing {
    #[serde(rename = "Id")]
    id: String,
}
#[derive(Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
struct Inspect {
    id: String,
    state: State,
    network_settings: Sandbox,
}
#[derive(Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
struct State {
    pid: u32,
    started_at: String,
    running: bool,
    restarting: bool,
    paused: bool,
}
#[derive(Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
struct Sandbox {
    sandbox_key: PathBuf,
}
#[derive(Deserialize)]
struct OciState {
    id: String,
    pid: u32,
    status: String,
    bundle: PathBuf,
}
#[derive(Deserialize)]
struct Spec {
    linux: Linux,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Linux {
    cgroups_path: String,
}

async fn scan(
    owner: &Arc<Shared>,
    before: &Bindings,
    stop: &mut watch::Receiver<bool>,
) -> Result<HashMap<u64, Arc<Binding>>, ModuleError> {
    let running = get::<Vec<Listing>>("/containers/json", stop).await?;
    if running.len() > 4096 {
        return Err(ModuleError::Busy);
    }
    let live_before: HashMap<_, _> = before
        .values()
        .filter(|binding| binding.live())
        .map(|binding| (binding.instance.id.as_str(), binding))
        .collect();
    let mut next = HashMap::new();
    for item in running {
        if *stop.borrow() {
            return Err(ModuleError::Stopped);
        }
        let id = item.id;
        if !full_id(&id) {
            return Err(ModuleError::InvalidInput);
        }
        let binding = match live_before.get(id.as_str()) {
            Some(binding) => (*binding).clone(),
            None => match bind(owner, &id, stop).await {
                Ok(binding) => Arc::new(binding),
                Err(error) => {
                    tracing::debug!(container_id = %id, %error, "Docker instance remains unbound");
                    continue;
                }
            },
        };
        if let Some(old) = next.insert(binding.cgroup_id, binding.clone())
            && old.instance != binding.instance
        {
            return Err(ModuleError::Conflict);
        }
    }
    Ok(next)
}

async fn bind(
    owner: &Arc<Shared>,
    id: &str,
    stop: &mut watch::Receiver<bool>,
) -> Result<Binding, ModuleError> {
    let resource = format!("/containers/{id}/json");
    let inspect = get::<Inspect>(&resource, stop).await?;
    if inspect.id != id
        || !inspect.state.running
        || inspect.state.restarting
        || inspect.state.paused
    {
        return Err(ModuleError::Unavailable);
    }
    let pid = inspect.state.pid;
    let pidfd = pidfd_open(
        Pid::from_raw(pid.try_into().map_err(|_| ModuleError::InvalidInput)?)
            .filter(|_| pid > 1)
            .ok_or(ModuleError::InvalidInput)?,
        PidfdFlags::empty(),
    )
    .map_err(|_| ModuleError::Unavailable)?;
    let oci = runc_state(id, stop).await?;
    if oci.id != id || oci.pid != pid || oci.status != "running" || !live(&pidfd) {
        return Err(ModuleError::Unauthenticated);
    }

    // The bundle is located by the trusted runtime, and the file must remain host-owned.
    let bundle = std::fs::canonicalize(oci.bundle).map_err(|_| ModuleError::Unavailable)?;
    let bundle_name = Path::new(id);
    let roots = [
        Path::new("/run/docker/containerd/daemon/io.containerd.runtime.v2.task/moby"),
        Path::new("/run/containerd/io.containerd.runtime.v2.task/moby"),
    ];
    if !roots.iter().any(|root| bundle == root.join(bundle_name)) {
        return Err(ModuleError::Unsupported);
    }
    let bundle = open_absolute(&bundle, OFlags::RDONLY | OFlags::DIRECTORY)?;
    let dir = bundle.metadata().map_err(|_| ModuleError::Unavailable)?;
    if dir.uid() != 0 || dir.mode() & 0o077 != 0 {
        return Err(ModuleError::Unauthenticated);
    }
    let spec_file = fs::openat2(
        &bundle,
        "config.json",
        OFlags::RDONLY | OFlags::CLOEXEC,
        Mode::empty(),
        ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS,
    )
    .map(File::from)
    .map_err(|_| ModuleError::Unavailable)?;
    let meta = spec_file.metadata().map_err(|_| ModuleError::Unavailable)?;
    if !meta.is_file() || meta.uid() != 0 || meta.len() > MAX_DOCUMENT as u64 {
        return Err(ModuleError::Unauthenticated);
    }
    let spec: Spec = serde_json::from_reader(std::io::BufReader::new(
        spec_file.take(MAX_DOCUMENT as u64 + 1),
    ))
    .map_err(|_| ModuleError::InvalidInput)?;
    let relative = cgroup_path(&spec.linux.cgroups_path, id)?;
    let cgroup = open_absolute(
        &Path::new(CGROUP_ROOT).join(relative),
        OFlags::RDONLY | OFlags::DIRECTORY,
    )?;
    if fs::fstatfs(&cgroup)
        .map_err(|_| ModuleError::Unavailable)?
        .f_type as u64
        != CGROUP2_MAGIC
    {
        return Err(ModuleError::Unsupported);
    }
    let cgroup_id = cgroup
        .metadata()
        .map_err(|_| ModuleError::Unavailable)?
        .ino();

    let sandbox = std::fs::canonicalize(&inspect.network_settings.sandbox_key)
        .map_err(|_| ModuleError::Unavailable)?;
    if sandbox.parent() != Some(Path::new("/run/docker/netns")) {
        return Err(ModuleError::Unsupported);
    }
    let sandbox = open_absolute(&sandbox, OFlags::RDONLY)?;
    let net = File::open(format!("/proc/{pid}/ns/net")).map_err(|_| ModuleError::Unavailable)?;
    let host = File::open("/proc/self/ns/net").map_err(|_| ModuleError::Unavailable)?;
    if object(&sandbox)? != object(&net)? || object(&host)? == object(&net)? {
        return Err(ModuleError::Unsupported);
    }

    let final_inspect = get::<Inspect>(&resource, stop).await?;
    let init = match owner.inspect(&[pid]).await?.pop() {
        Some(ProcessInspection::Bound { binding })
            if binding.host_tgid == pid && binding.cgroup_id == Some(cgroup_id) =>
        {
            binding
        }
        _ => return Err(ModuleError::Unauthenticated),
    };
    if final_inspect != inspect || !live(&pidfd) {
        return Err(ModuleError::Unauthenticated);
    }
    // Network namespace may change while the PID stays alive.
    let latest = File::open(format!("/proc/{pid}/ns/net")).map_err(|_| ModuleError::Unavailable)?;
    if object(&latest)? != object(&net)? || !live(&pidfd) {
        return Err(ModuleError::Unauthenticated);
    }
    Ok(Binding {
        instance: ContainerInstance {
            runtime: "docker".into(),
            namespace: None,
            id: id.into(),
        },
        cgroup,
        net,
        cgroup_id,
        init,
        pidfd,
    })
}

pub(crate) fn local_instance(instance: &ContainerInstance) -> bool {
    instance.runtime == "docker" && instance.namespace.is_none() && full_id(&instance.id)
}

fn full_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn cgroup_path(value: &str, id: &str) -> Result<PathBuf, ModuleError> {
    if let Some((slice, unit)) = value.split_once(':') {
        let suffix = format!("docker:{id}");
        if unit != suffix || slice.contains('/') {
            return Err(ModuleError::Unsupported);
        }
        let slice = slice
            .strip_suffix(".slice")
            .ok_or(ModuleError::InvalidInput)?;
        let mut path = PathBuf::new();
        if slice != "-" {
            let mut prefix = String::new();
            for component in slice.split('-') {
                if component.is_empty() || component == "." || component == ".." {
                    return Err(ModuleError::InvalidInput);
                }
                if !prefix.is_empty() {
                    prefix.push('-');
                }
                prefix.push_str(component);
                path.push(format!("{prefix}.slice"));
            }
        }
        path.push(format!("docker-{id}.scope"));
        return Ok(path);
    }
    let relative = Path::new(value)
        .strip_prefix("/")
        .map_err(|_| ModuleError::Unsupported)?;
    if !relative
        .components()
        .all(|c| matches!(c, Component::Normal(_)))
        || relative.file_name() != Some(std::ffi::OsStr::new(id))
    {
        return Err(ModuleError::Unsupported);
    }
    Ok(relative.to_path_buf())
}

fn open_absolute(path: &Path, flags: OFlags) -> Result<File, ModuleError> {
    fs::openat2(
        fs::CWD,
        path,
        flags | OFlags::CLOEXEC,
        Mode::empty(),
        ResolveFlags::NO_SYMLINKS,
    )
    .map(File::from)
    .map_err(|_| ModuleError::Unavailable)
}

fn object(file: &File) -> Result<(u64, u64), ModuleError> {
    let metadata = file.metadata().map_err(|_| ModuleError::Unavailable)?;
    Ok((metadata.dev(), metadata.ino()))
}

fn live(pidfd: &OwnedFd) -> bool {
    let mut fds = [PollFd::new(pidfd, PollFlags::IN)];
    poll(
        &mut fds,
        Some(&rustix::event::Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        }),
    ) == Ok(0)
}

async fn runc_state(id: &str, stop: &mut watch::Receiver<bool>) -> Result<OciState, ModuleError> {
    if *stop.borrow() {
        return Err(ModuleError::Stopped);
    }
    let mut child = Command::new("/usr/bin/runc")
        .args(["--root", RUNC_ROOT, "state", id])
        .env_clear()
        .current_dir("/")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::piped())
        .kill_on_drop(false)
        .spawn()
        .map_err(|_| ModuleError::Unavailable)?;
    let mut output = Vec::new();
    let result = tokio::select! {biased;
        _=stop.wait_for(|stop| *stop)=>Err(ModuleError::Stopped),
        result=async {
            child.stdout.as_mut().ok_or(ModuleError::Unavailable)?
                .take(MAX_DOCUMENT as u64 + 1)
                .read_to_end(&mut output).await.map_err(|_|ModuleError::Unavailable)?;
            if output.len()>MAX_DOCUMENT {
                return Err(ModuleError::Unavailable);
            }
            if !child.wait().await.map_err(|_|ModuleError::Unavailable)?.success() {
                return Err(ModuleError::Unavailable);
            }
            serde_json::from_slice(&output).map_err(|_|ModuleError::InvalidInput)
        }=>result,
    };
    if result.is_err() {
        if let Err(error) = child.start_kill() {
            tracing::debug!(%error, "Docker runtime state child cancellation failed; awaiting its actual exit");
        }
        child.wait().await.map_err(|_| ModuleError::Unavailable)?;
    }
    result
}

async fn get<T: DeserializeOwned>(
    path: &str,
    stop: &mut watch::Receiver<bool>,
) -> Result<T, ModuleError> {
    tokio::select! {biased;
        _=stop.wait_for(|stop| *stop)=>Err(ModuleError::Stopped),
        result=async {
        let socket =
            std::fs::symlink_metadata(DOCKER_SOCKET).map_err(|_| ModuleError::Unavailable)?;
        let run = std::fs::symlink_metadata("/run").map_err(|_| ModuleError::Unavailable)?;
        if !socket.file_type().is_socket()
            || socket.uid() != 0
            || socket.mode() & 0o002 != 0
            || !run.is_dir()
            || run.uid() != 0
            || run.mode() & 0o022 != 0
        {
            return Err(ModuleError::Unauthenticated);
        }
        let stream = UnixStream::connect(DOCKER_SOCKET)
            .await
            .map_err(|_| ModuleError::Unavailable)?;
        let cred = stream
            .peer_cred()
            .map_err(|_| ModuleError::Unauthenticated)?;
        let pid = cred
            .pid()
            .filter(|pid| *pid > 0)
            .ok_or(ModuleError::Unauthenticated)?;
        let actual =
            File::open(format!("/proc/{pid}/exe")).map_err(|_| ModuleError::Unauthenticated)?;
        // In the stock systemd unit the authenticated peer is the listener creator.
        let program = if pid == 1 {
            "/usr/lib/systemd/systemd"
        } else {
            "/usr/bin/dockerd"
        };
        let expected = File::open(program).map_err(|_| ModuleError::Unauthenticated)?;
        let current_socket =
            std::fs::symlink_metadata(DOCKER_SOCKET).map_err(|_| ModuleError::Unavailable)?;
        if cred.uid() != 0
            || object(&actual)? != object(&expected)?
            || (socket.dev(), socket.ino()) != (current_socket.dev(), current_socket.ino())
        {
            return Err(ModuleError::Unauthenticated);
        }
        let (mut sender, connection) = http1::handshake(TokioIo::new(stream))
            .await
            .map_err(|_| ModuleError::Unavailable)?;
        let (_, response) = tokio::try_join!(
            async { connection.await.map_err(|_| ModuleError::Unavailable) },
            async move {
                let req = Request::get(path)
                    .header("host", "localhost")
                    .header("connection", "close")
                    .body(Empty::<Bytes>::new())
                    .map_err(|_| ModuleError::InvalidInput)?;
                let response = sender
                    .send_request(req)
                    .await
                    .map_err(|_| ModuleError::Unavailable)?;
                if response.status() != hyper::StatusCode::OK {
                    return Err(ModuleError::Unavailable);
                }
                Limited::new(response.into_body(), MAX_DOCUMENT)
                    .collect()
                    .await
                    .map(|body| body.to_bytes())
                    .map_err(|_| ModuleError::InvalidInput)
            }
        )?;
        serde_json::from_slice(&response).map_err(|_| ModuleError::InvalidInput)
        }=>result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hierarchy_root_excludes_retired_local_docker_without_guessing_other_domains() {
        let bindings = Bindings::default();
        let local = ContainerInstance {
            runtime: "docker".into(),
            namespace: None,
            id: "a".repeat(64),
        };
        let selected = std::slice::from_ref(&local);
        assert!(outside_containers(&bindings, selected, &[42], 100));
        assert!(!outside_containers(&bindings, selected, &[42, 99], 100));
        assert!(!outside_containers(&bindings, selected, &[], 100));
        let foreign = ContainerInstance {
            runtime: "other".into(),
            ..local
        };
        assert!(!outside_containers(&bindings, &[foreign], &[42], 100));
    }
}
