//! Root-owned cgroup socket marking, packet DNAT and actual established socket admissions.
use crate::{
    Shared,
    docker::Binding,
    loader::asset,
    upstream::{Connector, Network},
};
use as_types::{
    dispatch_event::ProcessInfo,
    driver::{
        TLS_INGRESS_BPF_FILE_NAME, TLS_INGRESS_LOADER_FILE_NAME, TlsIngressInput,
        TlsIngressSession, TlsIngressStatus,
    },
    flow::{ActorFacts, AdmittedConnection, ChannelRef, FlowScope, IngressFacts},
    main_ports::ModuleError,
    process_tree::ProcessKey,
    workload::Workload,
};
use async_io::Async;
use rustix::{
    fs::{Mode, OFlags, ResolveFlags, openat2},
    net::{
        self, RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, ReturnFlags,
        SendAncillaryBuffer, SendAncillaryMessage, SendFlags,
    },
};
use std::{
    collections::HashMap,
    fs::File,
    io::{IoSlice, IoSliceMut},
    mem::MaybeUninit,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream},
    os::{
        fd::{AsFd, AsRawFd, OwnedFd},
        unix::{fs::MetadataExt, net::UnixDatagram, process::ExitStatusExt},
    },
    path::Path,
    process::Stdio,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    process::{Child, Command},
    sync::{mpsc, oneshot, watch},
};

struct Ingress {
    status: Arc<Mutex<TlsIngressStatus>>,
    stop: watch::Sender<bool>,
    done: watch::Receiver<Option<Result<(), ModuleError>>>,
}
type Lifetime = tokio_util::task::task_tracker::TaskTrackerToken;
const BINDING_BYTES: usize = 96;
const PACKET_BYTES: usize = 8 + BINDING_BYTES + 8;
impl Drop for Ingress {
    fn drop(&mut self) {
        self.stop.send_replace(true);
    }
}
#[async_trait::async_trait]
impl TlsIngressSession for Ingress {
    fn status(&self) -> Result<TlsIngressStatus, ModuleError> {
        self.status
            .lock()
            .map(|s| *s)
            .map_err(|_| ModuleError::Unavailable)
    }
    async fn stop_accepting(&self) -> Result<TlsIngressStatus, ModuleError> {
        self.stop.send_replace(true);
        let mut done = self.done.clone();
        loop {
            if let Some(result) = *done.borrow_and_update() {
                result?;
                return self.status();
            }
            done.changed().await.map_err(|_| ModuleError::Unavailable)?;
        }
    }
}
fn plan(
    input: &TlsIngressInput,
    subjects: Option<&[ProcessKey]>,
    cgroup_id: u64,
) -> Result<Vec<u8>, ModuleError> {
    if input.boot_id == [0; 16]
        || input.max_connections == 0
        || input.max_connections > 4096
        || input.destinations.is_empty()
        || input.destinations.len() > 4096
        || subjects.is_some_and(|s| s.is_empty() || s.len() > 4096)
    {
        return Err(ModuleError::InvalidInput);
    }
    let all = subjects.is_none();
    let subjects = subjects.unwrap_or(&[]);
    let mut out = Vec::new();
    for n in [
        u32::from(all),
        subjects.len() as u32,
        input.destinations.len() as u32,
        input.max_connections,
    ] {
        out.extend(n.to_ne_bytes());
    }
    out.extend(cgroup_id.to_ne_bytes());
    for key in subjects {
        if key.boot_id != input.boot_id || key.host_tgid == 0 || key.birth_boottime_ns == 0 {
            return Err(ModuleError::InvalidInput);
        }
        out.extend(key.host_tgid.to_ne_bytes());
        out.extend(0u32.to_ne_bytes());
        out.extend(key.birth_boottime_ns.to_ne_bytes());
    }
    for &addr in &input.destinations {
        encode_endpoint(&mut out, addr)?;
    }
    Ok(out)
}

fn encode_endpoint(out: &mut Vec<u8>, endpoint: SocketAddr) -> Result<(), ModuleError> {
    if endpoint.port() == 0 || endpoint.ip().is_unspecified() || endpoint.ip().is_multicast() {
        return Err(ModuleError::InvalidInput);
    }
    match endpoint {
        SocketAddr::V4(addr) => {
            out.extend(4u32.to_ne_bytes());
            out.extend(addr.ip().octets());
            out.extend([0; 12]);
        }
        SocketAddr::V6(addr) => {
            if addr.scope_id() != 0
                || addr.ip().is_unicast_link_local()
                || addr.ip().to_ipv4_mapped().is_some()
            {
                return Err(ModuleError::Unsupported);
            }
            out.extend(6u32.to_ne_bytes());
            out.extend(addr.ip().octets());
        }
    }
    out.extend(u32::from(endpoint.port().to_be()).to_ne_bytes());
    Ok(())
}

struct Packet {
    kind: u32,
    binding: [u8; BINDING_BYTES],
    fd: Option<OwnedFd>,
    accepted_cookie: u64,
}
fn receive(io: &UnixDatagram, pid: u32) -> std::io::Result<Packet> {
    let mut payload = [0u8; PACKET_BYTES];
    let mut control = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1), ScmCredentials(1))];
    let mut ancillary = RecvAncillaryBuffer::new(&mut control);
    let msg = net::recvmsg(
        io,
        &mut [IoSliceMut::new(&mut payload)],
        &mut ancillary,
        RecvFlags::CMSG_CLOEXEC,
    )
    .map_err(std::io::Error::from)?;
    let mut trusted = false;
    let mut fd = None;
    let mut extra = false;
    for item in ancillary.drain() {
        match item {
            RecvAncillaryMessage::ScmCredentials(c) => {
                trusted = c.pid.as_raw_pid() as u32 == pid && c.uid.as_raw() == 0;
            }
            RecvAncillaryMessage::ScmRights(rights) => {
                for incoming in rights {
                    if fd.is_some() {
                        extra = true;
                    }
                    fd = Some(incoming);
                }
            }
            _ => {
                extra = true;
            }
        }
    }
    if !trusted
        || extra
        || msg.bytes != PACKET_BYTES
        || msg
            .flags
            .intersects(ReturnFlags::TRUNC | ReturnFlags::CTRUNC)
        || payload[4..8] != [0; 4]
    {
        return Err(std::io::ErrorKind::PermissionDenied.into());
    }
    let mut binding = [0; BINDING_BYTES];
    binding.copy_from_slice(&payload[8..8 + BINDING_BYTES]);
    Ok(Packet {
        kind: u32::from_ne_bytes(
            payload[..4]
                .try_into()
                .map_err(|_| std::io::ErrorKind::InvalidData)?,
        ),
        binding,
        fd,
        accepted_cookie: u64::from_ne_bytes(
            payload[8 + BINDING_BYTES..]
                .try_into()
                .map_err(|_| std::io::ErrorKind::InvalidData)?,
        ),
    })
}
fn facts(
    bytes: &[u8; BINDING_BYTES],
    source: [u8; 16],
    input: &TlsIngressInput,
    scope: &Scope,
    generation: u64,
) -> Result<IngressFacts, ModuleError> {
    let read64 = |o| {
        let mut value = [0; 8];
        value.copy_from_slice(&bytes[o..o + 8]);
        u64::from_ne_bytes(value)
    };
    let read32 = |o| {
        let mut value = [0; 4];
        value.copy_from_slice(&bytes[o..o + 4]);
        u32::from_ne_bytes(value)
    };
    let key = ProcessKey {
        boot_id: input.boot_id,
        host_tgid: read32(40),
        birth_boottime_ns: read64(8),
    };
    let cookie = read64(0);
    let port = read32(88);
    let ip = match read32(48) {
        4 if bytes[56..68] == [0; 12] && bytes[72..84] == [0; 12] => {
            IpAddr::V4(Ipv4Addr::new(bytes[68], bytes[69], bytes[70], bytes[71]))
        }
        6 => {
            let mut address = [0; 16];
            address.copy_from_slice(&bytes[68..84]);
            IpAddr::V6(Ipv6Addr::from(address))
        }
        _ => return Err(ModuleError::InvalidInput),
    };
    if cookie == 0
        || key.host_tgid == 0
        || key.birth_boottime_ns == 0
        || read32(44) == 0
        || read32(84) == 0
        || read32(84) > 65535
        || read32(92) != 0
        || port == 0
        || port > 65535
        || scope.subjects.as_ref().is_some_and(|s| !s.contains(&key))
        || !input
            .destinations
            .iter()
            .any(|a| a.ip() == ip && a.port() == port as u16)
    {
        return Err(ModuleError::Unauthenticated);
    }
    let workload = if let Some(binding) = &scope.binding {
        if !binding.matches(read64(32), read64(24)) {
            return Err(ModuleError::Unauthenticated);
        }
        Some(Workload::Container(binding.instance.clone()))
    } else {
        None
    };
    Ok(IngressFacts {
        connection_initiator: ActorFacts {
            process: Some(key),
            info: ProcessInfo {
                workload,
                ..ProcessInfo::default()
            },
            tid: Some(read32(44)),
            thread_birth_boottime_ns: Some(read64(16)),
            observed_boottime_ns: read64(24),
            cgroup_id: Some(read64(32)),
        },
        channel: ChannelRef {
            kind: as_types::flow::ChannelKind::Socket,
            source_instance: source,
            object_id: cookie,
            generation: 1,
        },
        original_remote_ip: ip,
        original_remote_port: port as u16,
        plan_generation: generation,
        config_version: input.config_version,
    })
}
struct Scope {
    subjects: Option<Vec<ProcessKey>>,
    binding: Option<Arc<Binding>>,
}

struct Assets {
    program: File,
    object: File,
}

/// Start the actual scopes only after the Flow consumer is ready.
pub(crate) async fn start(
    directory: &Path,
    source: [u8; 16],
    input: TlsIngressInput,
    output: mpsc::Sender<AdmittedConnection>,
    owner: std::sync::Weak<Shared>,
    lifetime: Lifetime,
) -> Result<Arc<dyn TlsIngressSession>, ModuleError> {
    let (native_stop, active, bindings) = {
        let owner = owner.upgrade().ok_or(ModuleError::Unavailable)?;
        (
            owner.admission_stop.subscribe(),
            owner.tls_active.clone(),
            owner.containers.subscribe(),
        )
    };
    if active
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err(ModuleError::Busy);
    }
    struct Active(Arc<AtomicBool>);
    impl Drop for Active {
        fn drop(&mut self) {
            self.0.store(false, Ordering::Release);
        }
    }
    let guard = Active(active);
    let host = match &input.scope {
        FlowScope::AllProtected => Some(Scope { subjects: None, binding: None }),
        FlowScope::Processes(processes) => Some(Scope { subjects: Some(processes.clone()), binding: None }),
        FlowScope::Workloads(items) if !items.is_empty() && items.iter().all(|item| {
            matches!(item, Workload::Container(c) if crate::docker::local_instance(c))
        }) => None,
        FlowScope::Workloads(_) => return Err(ModuleError::Unsupported),
    };
    plan(&input, host.as_ref().and_then(|s| s.subjects.as_deref()), 0)?;
    if *native_stop.borrow() {
        return Err(ModuleError::Unavailable);
    }
    let directory = directory.to_path_buf();
    let startup = lifetime.clone();
    let assets = tokio::task::spawn_blocking(move || {
        let _startup = startup;
        let root = File::from(
            openat2(
                rustix::fs::CWD,
                &directory,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
                Mode::empty(),
                ResolveFlags::NO_SYMLINKS,
            )
            .map_err(|_| ModuleError::Unavailable)?,
        );
        let meta = root.metadata().map_err(|_| ModuleError::Unavailable)?;
        if meta.uid() != 0 || meta.mode() & 0o022 != 0 {
            return Err(ModuleError::Unauthenticated);
        }
        Ok(Arc::new(Assets {
            program: asset(&root, TLS_INGRESS_LOADER_FILE_NAME)?,
            object: asset(&root, TLS_INGRESS_BPF_FILE_NAME)?,
        }))
    })
    .await
    .map_err(|_| ModuleError::Unavailable)??;
    let status = Arc::new(Mutex::new(TlsIngressStatus {
        generation: 0,
        accepting: false,
        admitted: 0,
        rejected: 0,
    }));
    let (stop, stopping) = watch::channel(false);
    let (done, finished) = watch::channel(None);
    let (ready, waiting) = oneshot::channel();
    let session = Arc::new(Ingress {
        status: status.clone(),
        stop,
        done: finished,
    });
    tokio::spawn(async move {
        let supervisor_lifetime = lifetime.clone();
        let _resources = (guard, lifetime);
        let result = Supervisor {
            assets,
            input: Arc::new(input),
            source,
            output,
            status,
            host,
            bindings,
            stop: stopping,
            native_stop,
            ready: Some(ready),
            active: HashMap::new(),
            lifetime: supervisor_lifetime,
        }
        .run()
        .await;
        if let Some(owner) = owner.upgrade() {
            owner.record_tls_exit(result);
        }
        let _ = done.send(Some(result));
    });
    match waiting.await {
        Ok(()) => Ok(session),
        Err(_) => match session.stop_accepting().await {
            Err(error) => Err(error),
            Ok(_) => Err(ModuleError::Unavailable),
        },
    }
}

struct Supervisor {
    assets: Arc<Assets>,
    input: Arc<TlsIngressInput>,
    source: [u8; 16],
    output: mpsc::Sender<AdmittedConnection>,
    status: Arc<Mutex<TlsIngressStatus>>,
    host: Option<Scope>,
    bindings: watch::Receiver<crate::docker::Bindings>,
    stop: watch::Receiver<bool>,
    native_stop: watch::Receiver<bool>,
    ready: Option<oneshot::Sender<()>>,
    active: HashMap<(u64, u64), Arc<Ingress>>,
    lifetime: Lifetime,
}
impl Supervisor {
    async fn add(&mut self, scope: Scope) -> Result<(), ModuleError> {
        let generation = self
            .status
            .lock()
            .map_err(|_| ModuleError::Unavailable)?
            .generation
            .checked_add(1)
            .ok_or(ModuleError::Unavailable)?;
        let id = scope.binding.as_ref().map_or((0, 0), |binding| {
            (binding.cgroup_id, binding.init.birth_boottime_ns)
        });
        let entry = self.start_one(scope, generation).await?;
        self.active.insert(id, entry);
        self.status
            .lock()
            .map_err(|_| ModuleError::Unavailable)?
            .generation = generation;
        Ok(())
    }

    async fn reconcile(&mut self) -> Result<(), ModuleError> {
        let snapshot = self.bindings.borrow_and_update().clone();
        let selected = |binding: &Binding| {
            match &self.input.scope {
            FlowScope::AllProtected => true,
            FlowScope::Workloads(items) => items.iter().any(|item| {
                matches!(item, Workload::Container(instance) if *instance == binding.instance)
            }),
            FlowScope::Processes(_) => false,
        }
        };
        let obsolete: Vec<_> = self
            .active
            .keys()
            .copied()
            .filter(|id| {
                id.0 != 0
                    && !snapshot.get(&id.0).is_some_and(|binding| {
                        selected(binding)
                            && binding.init.birth_boottime_ns == id.1
                            && binding.live()
                    })
            })
            .collect();
        let new: Vec<_> = snapshot
            .iter()
            .filter(|(id, binding)| {
                !self
                    .active
                    .contains_key(&(**id, binding.init.birth_boottime_ns))
                    && selected(binding)
                    && binding.live()
            })
            .map(|(_, binding)| binding.clone())
            .collect();
        for id in obsolete {
            if let Some(ingress) = self.active.remove(&id) {
                ingress.stop_accepting().await?;
            }
        }
        for entry in self.active.values() {
            if let Some(done) = *entry.done.borrow() {
                done?;
                return Err(ModuleError::Unavailable);
            }
        }
        for binding in new {
            if *self.stop.borrow() || *self.native_stop.borrow() {
                break;
            }
            let result = self
                .add(Scope {
                    subjects: None,
                    binding: Some(binding.clone()),
                })
                .await;
            if binding.live() {
                result?;
            }
        }
        let mut status = self.status.lock().map_err(|_| ModuleError::Unavailable)?;
        status.accepting = !self.active.is_empty();
        Ok(())
    }

    async fn run(mut self) -> Result<(), ModuleError> {
        let result = self.run_until_stop().await;
        self.ready.take();
        if let Ok(mut status) = self.status.lock() {
            status.accepting = false;
        }
        for entry in self.active.values() {
            entry.stop.send_replace(true);
        }
        let mut first_error = result.err();
        for (_, entry) in self.active.drain() {
            if let Err(error) = entry.stop_accepting().await {
                first_error.get_or_insert(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    async fn run_until_stop(&mut self) -> Result<(), ModuleError> {
        if let Some(host) = self.host.take() {
            self.add(host).await?;
        }
        self.reconcile().await?;
        if self
            .ready
            .take()
            .is_none_or(|ready| ready.send(()).is_err())
        {
            return Ok(());
        }
        let mut poll = tokio::time::interval(Duration::from_millis(250));
        poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! { biased;
                _ = self.stop.wait_for(|stop| *stop) => return Ok(()),
                _ = self.native_stop.wait_for(|stop| *stop) => return Ok(()),
                _ = self.output.closed() => return Err(ModuleError::Unavailable),
                changed = self.bindings.changed() => changed.map_err(|_| ModuleError::Stopped)?,
                _ = poll.tick() => {},
            }
            self.reconcile().await?;
        }
    }

    async fn start_one(&self, scope: Scope, generation: u64) -> Result<Arc<Ingress>, ModuleError> {
        let assets = self.assets.clone();
        let source = self.source;
        let input = self.input.clone();
        let output = self.output.clone();
        let status = self.status.clone();
        let native_stop = self.native_stop.clone();
        let lifetime = self.lifetime.clone();
        let (cg, ns, network) = if let Some(binding) = &scope.binding {
            let ns = binding
                .net
                .try_clone()
                .map_err(|_| ModuleError::Unavailable)?;
            let network = Network::start(
                ns.try_clone().map_err(|_| ModuleError::Unavailable)?,
                input.max_connections as usize,
                lifetime.clone(),
            )
            .await
            .map_err(|_| ModuleError::Unavailable)?;
            (
                binding
                    .cgroup
                    .try_clone()
                    .map_err(|_| ModuleError::Unavailable)?,
                ns,
                Some(network),
            )
        } else {
            (
                File::open("/sys/fs/cgroup").map_err(|_| ModuleError::Unavailable)?,
                File::open("/proc/self/ns/net").map_err(|_| ModuleError::Unavailable)?,
                None,
            )
        };
        let bytes = plan(
            &input,
            scope.subjects.as_deref(),
            scope.binding.as_ref().map_or(0, |b| b.cgroup_id),
        )?;
        let (parent_fd, child_fd) = net::socketpair(
            net::AddressFamily::UNIX,
            net::SocketType::SEQPACKET,
            net::SocketFlags::CLOEXEC,
            None,
        )
        .map_err(|_| ModuleError::Unavailable)?;
        let parent = UnixDatagram::from(parent_fd);
        let child_socket = UnixDatagram::from(child_fd);
        net::sockopt::set_socket_passcred(&parent, true).map_err(|_| ModuleError::Unavailable)?;
        net::sockopt::set_socket_passcred(&child_socket, true)
            .map_err(|_| ModuleError::Unavailable)?;
        parent
            .set_nonblocking(true)
            .map_err(|_| ModuleError::Unavailable)?;
        let io = Arc::new(Async::new(parent).map_err(|_| ModuleError::Unavailable)?);
        let child_fd: OwnedFd = child_socket.into();
        let child = Command::new(format!("/proc/self/fd/{}", assets.program.as_raw_fd()))
            .arg(format!(
                "/proc/{}/fd/{}",
                std::process::id(),
                assets.object.as_raw_fd()
            ))
            .stdin(Stdio::from(child_fd))
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .env_clear()
            .current_dir("/")
            .kill_on_drop(false)
            .spawn()
            .map_err(|_| ModuleError::Unavailable)?;
        let pid = child.id().ok_or(ModuleError::Unavailable)?;
        let (stop, stopping) = watch::channel(false);
        let (done, finished) = watch::channel(None);
        let (ready, waiting) = oneshot::channel();
        let session = Arc::new(Ingress {
            status: status.clone(),
            stop,
            done: finished,
        });
        tokio::spawn(async move {
            let _resources = (assets, lifetime);
            let mut diagnostic = None;
            let result = Runner {
                child,
                pid,
                io,
                plan: bytes,
                source,
                input,
                output,
                status,
                stop: stopping,
                native_stop,
                ready,
                cg,
                ns,
                network,
                scope,
                generation,
            }
            .run(&mut diagnostic)
            .await;
            let _ = done.send(Some(result));
            if let Some(diagnostic) = diagnostic {
                as_types::diagnostics::record_stderr(diagnostic).await;
            }
        });
        match waiting.await.map_err(|_| ModuleError::Unavailable)? {
            Ok(()) => Ok(session),
            Err(e) => {
                let _ = session.stop_accepting().await;
                Err(e)
            }
        }
    }
}

struct Runner {
    child: Child,
    pid: u32,
    io: Arc<Async<UnixDatagram>>,
    plan: Vec<u8>,
    source: [u8; 16],
    input: Arc<TlsIngressInput>,
    output: mpsc::Sender<AdmittedConnection>,
    status: Arc<Mutex<TlsIngressStatus>>,
    stop: watch::Receiver<bool>,
    native_stop: watch::Receiver<bool>,
    ready: oneshot::Sender<Result<(), ModuleError>>,
    cg: File,
    ns: File,
    scope: Scope,
    network: Option<Arc<Network>>,
    generation: u64,
}
impl Runner {
    async fn run(self, diagnostic: &mut Option<String>) -> Result<(), ModuleError> {
        let Self {
            mut child,
            pid,
            io,
            plan,
            source,
            input,
            output,
            status,
            mut stop,
            mut native_stop,
            ready,
            cg,
            ns,
            scope,
            network,
            generation,
        } = self;
        let first = tokio::select! {biased;
            _=stop.wait_for(|stop| *stop)=>Err(ModuleError::Stopped),
            _=native_stop.wait_for(|stop| *stop)=>Err(ModuleError::Stopped),
            _=output.closed()=>Err(ModuleError::Unavailable),
            _=child.wait()=>Err(ModuleError::Unavailable),
            first=async {
                io.write_with(|p|send_plan(p,&plan,&cg,&ns)).await.map_err(|_|ModuleError::Unavailable)?;
                io.read_with(|p|receive(p,pid)).await.map_err(|_|ModuleError::Unavailable)
            }=>first,
        };
        if let Err(error) = first.and_then(|packet| match packet {
            Packet {
                kind: 1, fd: None, ..
            } => Ok(()),
            _ => Err(ModuleError::Unavailable),
        }) {
            let _ = ready.send(Err(error));
            let _ = io.write_with(|p| p.send(b"S")).await;
            let _ = child.wait().await;
            return Err(error);
        }
        if ready.send(Ok(())).is_err() {
            stop = watch::channel(true).1;
        }
        let mut failure = None;
        loop {
            if *stop.borrow() || *native_stop.borrow() {
                break;
            }
            tokio::select! {biased;
                _=stop.changed()=>break,
                _=native_stop.changed()=>break,
                _=output.closed()=>{failure=Some(ModuleError::Unavailable);break;},
                exited=child.wait()=>{let _=exited;failure=Some(ModuleError::Unavailable);break;},
                packet=io.read_with(|p|receive(p,pid))=>{
                    let packet=match packet{Ok(p)=>p,Err(_)=>{failure=Some(ModuleError::Unauthenticated);break;}};
                    if packet.kind!=2{failure=Some(ModuleError::InvalidInput);break;}
                    let result=(||{
                        let facts=facts(&packet.binding,source,&input,&scope,generation)?;
                        let fd=packet.fd.ok_or(ModuleError::InvalidInput)?;
                        if packet.accepted_cookie==0 || net::sockopt::socket_cookie(&fd).map_err(|_|ModuleError::Unavailable)?!=packet.accepted_cookie{return Err(ModuleError::Unauthenticated)}
                        let stream=TcpStream::from(fd);
                        if stream.local_addr().map_err(|_|ModuleError::Unavailable)?.is_ipv4()!=facts.original_remote_ip.is_ipv4(){return Err(ModuleError::Unauthenticated)}
                        stream.set_nonblocking(true).map_err(|_|ModuleError::Unavailable)?;
                        let stream=tokio::net::TcpStream::from_std(stream).map_err(|_|ModuleError::Unavailable)?;
                        let peer = SocketAddr::new(facts.original_remote_ip, facts.original_remote_port);
                        output.try_send(AdmittedConnection{stream:Box::pin(stream),facts,
                            upstream: Arc::new(Connector { peer, network: network.clone() })
                        }).map_err(|_|ModuleError::Busy)
                    })();
                    let mut state=status.lock().map_err(|_|ModuleError::Unavailable)?;
                    if result.is_ok(){state.admitted=state.admitted.saturating_add(1);}else{state.rejected=state.rejected.saturating_add(1);}
                }
            }
        }
        let _ = io.write_with(|p| p.send(b"S")).await;
        let exited = child.wait().await.map_err(|error| {
            *diagnostic = Some(format!(
                "AS_TLS_INGRESS_HELPER_WAIT_FAILED errno={:?}",
                error.raw_os_error()
            ));
            ModuleError::Unavailable
        })?;
        *diagnostic = Some(format!(
            "AS_TLS_INGRESS_HELPER_EXIT code={:?} signal={:?}",
            exited.code(),
            exited.signal()
        ));
        if let Some(e) = failure {
            return Err(e);
        }
        if !exited.success() {
            return Err(ModuleError::Unavailable);
        }
        Ok(())
    }
}
fn send_plan(io: &UnixDatagram, plan: &[u8], cg: &File, ns: &File) -> std::io::Result<usize> {
    let mut buffer = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(2))];
    let mut ancillary = SendAncillaryBuffer::new(&mut buffer);
    let handles = [cg.as_fd(), ns.as_fd()];
    if !ancillary.push(SendAncillaryMessage::ScmRights(&handles)) {
        return Err(std::io::ErrorKind::InvalidData.into());
    }
    net::sendmsg(
        io,
        &[IoSlice::new(plan)],
        &mut ancillary,
        SendFlags::NOSIGNAL,
    )
    .map_err(std::io::Error::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_plan_never_accepts_unsupported_or_reused_identity() {
        let boot = [1; 16];
        let mut input = TlsIngressInput {
            boot_id: boot,
            scope: FlowScope::AllProtected,
            destinations: vec!["192.0.2.1:443".parse().expect("endpoint")],
            config_version: as_types::configuration::ConfigVersion::from_digest([2; 32]),
            max_connections: 8,
        };
        let mut subjects = [ProcessKey {
            boot_id: boot,
            host_tgid: 123,
            birth_boottime_ns: 55,
        }];
        assert!(plan(&input, Some(&subjects), 0).is_ok());
        subjects[0].boot_id = [9; 16];
        assert!(plan(&input, Some(&subjects), 0).is_err());
        input.destinations = vec!["[::1]:443".parse().expect("ipv6")];
        let bytes = plan(&input, None, 0).expect("native IPv6 plan");
        assert_eq!(&bytes[24..28], &6u32.to_ne_bytes());
        assert_eq!(&bytes[28..44], &Ipv6Addr::LOCALHOST.octets());
        input.destinations = vec!["[fe80::1%2]:443".parse().expect("scoped ipv6")];
        assert!(matches!(
            plan(&input, None, 0),
            Err(ModuleError::Unsupported)
        ));
    }
}
