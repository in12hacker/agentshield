//! Linux backend/native adapter. One authenticated device connection, one receive pump, real receipts.
#![forbid(unsafe_code)]
#![cfg(target_os = "linux")]
mod completion;
mod dns;
mod docker;
mod loader;
mod operation;
mod runtime_observation;
mod tls_ingress;
mod upstream;
mod wire;

use as_types::{
    context::TargetPlatform,
    driver::*,
    driver_control::*,
    driver_image::DriverInitParts,
    driver_tools::{DriverProcessTarget, DriverTools, KillProcessOutcome},
    driver_wire::{DriverFrame, FrameClass, HEADER_BYTES},
    main_ports::{ChoiceReason, Decision, DispatchInput, ModuleError, RuleChoice},
    process_source::{
        ProcessFactsBarrier, ProcessFactsPort, ProcessInspection, ProcessTrackingStart,
    },
};
use async_io::Async;
use loader::Loader;
use operation::Original;
use std::{
    collections::{HashMap, VecDeque},
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt},
    sync::{Arc, Mutex, OnceLock, Weak},
};
use tokio::sync::{
    Mutex as AsyncMutex, Notify, OwnedSemaphorePermit, Semaphore, mpsc, oneshot, watch,
};
use tokio_util::task::TaskTracker;

type ReplySender = oneshot::Sender<Result<DriverFrame, ModuleError>>;
type FactsReceipt = watch::Receiver<Option<Result<ProcessFactsBarrier, ModuleError>>>;
enum Critical {
    Required(Box<DriverEvent>),
    Retired(u64),
    Facts {
        marker: ProcessFactsBarrier,
        processed: oneshot::Sender<()>,
    },
}
struct GroupSink {
    events: mpsc::Sender<NativeGroupTcp>,
    full: u64,
    closed: u64,
    first_rejection: Option<&'static str>,
}
impl GroupSink {
    fn offer(&mut self, event: NativeGroupTcp) -> Option<&'static str> {
        let first = self.full == 0 && self.closed == 0;
        let (reason, count) = match self.events.try_send(event) {
            Ok(()) => return None,
            Err(mpsc::error::TrySendError::Full(_)) => ("full", &mut self.full),
            Err(mpsc::error::TrySendError::Closed(_)) => ("closed", &mut self.closed),
        };
        *count = count.saturating_add(1);
        if first {
            self.first_rejection = Some(reason);
        }
        first.then_some(reason)
    }
    fn retire(self) -> Option<String> {
        if self.full != 0 || self.closed != 0 {
            let first = self.first_rejection.map_or_else(String::new, |reason| {
                format!("AS_GROUP_ADAPTER_REJECTED reason={reason}\n")
            });
            Some(format!(
                "{first}AS_GROUP_ADAPTER_LOSS full={} closed={}",
                self.full, self.closed
            ))
        } else {
            None
        }
    }
}
struct Ipc {
    available: bool,
    retiring: bool,
    next: u64,
    replies: HashMap<u64, (u16, ReplySender)>,
    originals: HashMap<u64, Arc<Original>>,
    controlled: Option<mpsc::Sender<DispatchInput>>,
    control_permit: Option<mpsc::OwnedPermit<DispatchInput>>,
    events: Option<mpsc::Sender<DriverEvent>>,
    group: Option<GroupSink>,
    // At most the original group's final diagnostic, drained by the normal shutdown owner.
    group_diagnostic: Option<String>,
    generations: HashMap<u64, OwnedSemaphorePermit>,
    critical: VecDeque<Critical>,
    facts_active: bool,
    facts_expected: Option<(u64, u64)>,
    facts_markers: HashMap<(u64, u64), FactsReceipt>,
    unconfirmed: Vec<OwnedSemaphorePermit>,
    // First failed TLS runner result, reported after all native resources retire.
    first_tls_exit_error: Option<ModuleError>,
}
pub(crate) struct Shared {
    host_boot_id: [u8; 16],
    runtime_observation: runtime_observation::Resources,
    io: Mutex<Option<Arc<Async<File>>>>,
    assets: OnceLock<std::path::PathBuf>,
    tls_active: Arc<std::sync::atomic::AtomicBool>,
    ipc: Mutex<Ipc>,
    info: OnceLock<DriverInfo>,
    bulk: AsyncMutex<()>,
    stop: watch::Sender<bool>,
    work: TaskTracker,
    rejected: watch::Sender<u64>,
    hit_rejected: std::sync::atomic::AtomicU64,
    generation_budget: OnceLock<Arc<Semaphore>>,
    facts_serial: AsyncMutex<()>,
    facts_changed: Notify,
    admission_stop: watch::Sender<bool>,
    containers: watch::Sender<docker::Bindings>,
}
impl Shared {
    fn new(io: Async<File>, host_boot_id: [u8; 16]) -> Arc<Self> {
        let (stop, _) = watch::channel(false);
        let (admission_stop, _) = watch::channel(false);
        let (containers, _) = watch::channel(docker::Bindings::default());
        let (rejected, _) = watch::channel(0);
        Arc::new(Self {
            host_boot_id,
            runtime_observation: runtime_observation::Resources::default(),
            io: Mutex::new(Some(Arc::new(io))),
            assets: OnceLock::new(),
            tls_active: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            ipc: Mutex::new(Ipc {
                available: true,
                retiring: false,
                next: 1,
                replies: HashMap::new(),
                originals: HashMap::new(),
                controlled: None,
                control_permit: None,
                events: None,
                group: None,
                group_diagnostic: None,
                generations: HashMap::new(),
                critical: VecDeque::new(),
                facts_active: false,
                facts_expected: None,
                facts_markers: HashMap::new(),
                unconfirmed: Vec::new(),
                first_tls_exit_error: None,
            }),
            info: OnceLock::new(),
            bulk: AsyncMutex::new(()),
            stop,
            work: TaskTracker::new(),
            rejected,
            hit_rejected: std::sync::atomic::AtomicU64::new(0),
            generation_budget: OnceLock::new(),
            facts_serial: AsyncMutex::new(()),
            facts_changed: Notify::new(),
            admission_stop,
            containers,
        })
    }
    // Capture work ownership under the same state lock used to begin retirement.
    // The token covers asynchronous startup and the resulting resource lifetime.
    fn retain_admission(
        &self,
    ) -> Result<tokio_util::task::task_tracker::TaskTrackerToken, ModuleError> {
        let state = self.ipc.lock().map_err(|_| ModuleError::Unavailable)?;
        if !state.available {
            return Err(ModuleError::Unavailable);
        }
        if state.retiring {
            return Err(ModuleError::Stopped);
        }
        Ok(self.work.token())
    }
    fn record_tls_exit(&self, result: Result<(), ModuleError>) {
        if let Err(error) = result
            && let Ok(mut state) = self.ipc.lock()
        {
            state.first_tls_exit_error.get_or_insert(error);
        }
    }
    fn tls_cleanup_result(&self) -> Result<(), ModuleError> {
        let state = self.ipc.lock().map_err(|_| ModuleError::Unavailable)?;
        state.first_tls_exit_error.map_or(Ok(()), Err)
    }
    fn file(&self) -> Result<Arc<Async<File>>, ModuleError> {
        self.io
            .lock()
            .map_err(|_| ModuleError::Unavailable)?
            .clone()
            .ok_or(ModuleError::Unavailable)
    }
    pub(crate) fn capabilities(&self) -> Result<&DriverCapabilities, ModuleError> {
        Ok(&self
            .info
            .get()
            .ok_or(ModuleError::Unavailable)?
            .capabilities)
    }
    fn container_snapshot(&self) -> docker::Bindings {
        self.containers.borrow().clone()
    }
    fn reject(&self) {
        self.rejected
            .send_modify(|count| *count = count.saturating_add(1));
        let rejected_callbacks = *self.rejected.borrow();
        tracing::error!(
            rejected_callbacks,
            "backend driver callback queue rejected a real native event"
        );
    }
    fn fail(&self) {
        let (replies, originals, events, critical) = {
            // Poison is never a recovery-to-running path: only take ownership for failure cleanup.
            let mut state = match self.ipc.lock() {
                Ok(state) => state,
                Err(poison) => poison.into_inner(),
            };
            state.available = false;
            state.controlled.take();
            state.control_permit.take();
            if let Some(diagnostic) = state.group.take().and_then(GroupSink::retire) {
                state.group_diagnostic = Some(diagnostic);
            }
            (
                std::mem::take(&mut state.replies),
                std::mem::take(&mut state.originals),
                state.events.take(),
                std::mem::take(&mut state.critical),
            )
        };
        for (_, (_, waiting)) in replies {
            let _ = waiting.send(Err(ModuleError::Unavailable));
        }
        for (_, operation) in originals {
            operation.complete(Err(ModuleError::Unavailable));
        }
        drop(critical);
        if let Some(events) = events
            && events.try_send(DriverEvent::TransportUnavailable).is_err()
        {
            self.reject();
        }
        self.stop.send_replace(true);
        self.admission_stop.send_replace(true);
        self.facts_changed.notify_one();
        if let Some(budget) = self.generation_budget.get() {
            budget.close();
        }
        let file = match self.io.lock() {
            Ok(mut io) => io.take(),
            Err(poison) => poison.into_inner().take(),
        };
        drop(file);
    }
    async fn send_frame(self: &Arc<Self>, frame: DriverFrame) -> Result<(), ModuleError> {
        let bytes: Arc<[u8]> = frame.encode()?.into();
        let mut stopped = self.stop.subscribe();
        loop {
            if *stopped.borrow() {
                return Err(ModuleError::Unavailable);
            }
            let io = self.file()?;
            let writing = io.clone();
            let frame = bytes.clone();
            let owner = Arc::downgrade(self);
            // Native write may allocate/parse. It is one owned atomic frame syscall,
            // not a blocking closure on the single callback/message runtime.
            let result = tokio::task::spawn_blocking(move || {
                let owner = owner
                    .upgrade()
                    .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::BrokenPipe))?;
                if !owner
                    .ipc
                    .lock()
                    .map_err(|_| std::io::Error::from(std::io::ErrorKind::BrokenPipe))?
                    .available
                {
                    return Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe));
                }
                let mut file = writing.get_ref();
                file.write(&frame)
            })
            .await
            .map_err(|_| ModuleError::Unavailable)?;
            match result {
                Ok(count) if count == bytes.len() => return Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    let stop = async {
                        if !*stopped.borrow() {
                            let _ = stopped.changed().await;
                        }
                    };
                    tokio::select! {biased;_ = stop=>return Err(ModuleError::Unavailable),ready=io.writable()=>ready.map_err(|_|ModuleError::Unavailable)?}
                }
                _ => return Err(ModuleError::Unavailable),
            }
        }
    }
    async fn exchange(
        self: &Arc<Self>,
        kind: u16,
        body: Vec<u8>,
    ) -> Result<DriverFrame, ModuleError> {
        let bulk = matches!(kind, 2 | 3);
        let max = if let Some(info) = self.info.get() {
            info.capabilities.max_frame_bytes as usize
        } else {
            wire::MAX_FRAME
        };
        let total = body.len() as u64;
        if total
            > self
                .info
                .get()
                .map_or(wire::MAX_TRANSFER, |v| v.capabilities.max_transfer_bytes)
        {
            return Err(ModuleError::Busy);
        }
        if !bulk
            && body.len()
                > max
                    .checked_sub(HEADER_BYTES)
                    .ok_or(ModuleError::InvalidInput)?
        {
            return Err(ModuleError::InvalidInput);
        }
        let (waiting, mut received) = oneshot::channel();
        let id = {
            let mut state = self.ipc.lock().map_err(|_| ModuleError::Unavailable)?;
            if !state.available {
                return Err(ModuleError::Unavailable);
            }
            let id = state.next;
            state.next = state.next.checked_add(1).ok_or(ModuleError::Unavailable)?;
            state.replies.insert(id, (kind, waiting));
            id
        };
        let session = self.info.get().map_or([0; 16], |v| v.session_id);
        let fragment = max
            .checked_sub(HEADER_BYTES)
            .filter(|v| *v >= 136)
            .ok_or(ModuleError::InvalidInput)?;
        let mut offset = 0;
        loop {
            match received.try_recv() {
                Ok(frame) => return frame,
                Err(oneshot::error::TryRecvError::Closed) => return Err(ModuleError::Unavailable),
                Err(oneshot::error::TryRecvError::Empty) => {}
            }
            let count = if bulk {
                (body.len() - offset).min(fragment)
            } else {
                body.len()
            };
            let frame = DriverFrame {
                class: FrameClass::Request,
                message_type: kind,
                session,
                request_id: id,
                total_bytes: total,
                offset: offset as u64,
                body: body[offset..offset + count].to_vec(),
            };
            if let Err(error) = self.send_frame(frame).await {
                self.fail();
                return Err(error);
            }
            offset += count;
            if offset == body.len() {
                break;
            }
        }
        received.await.map_err(|_| ModuleError::Unavailable)?
    }
    pub(crate) async fn command(
        self: &Arc<Self>,
        kind: u16,
        body: Vec<u8>,
    ) -> Result<Vec<u8>, ModuleError> {
        if matches!(kind, 2 | 3) {
            return Err(ModuleError::InvalidInput);
        }
        let (done, waiting) = oneshot::channel();
        let owner = self.clone();
        {
            let state = self.ipc.lock().map_err(|_| ModuleError::Unavailable)?;
            if !state.available {
                return Err(ModuleError::Unavailable);
            }
            if state.retiring && !matches!(kind, 5 | 7..=9) {
                return Err(ModuleError::Stopped);
            }
            self.work.spawn(async move {
                let result = owner
                    .exchange(kind, body)
                    .await
                    .and_then(|frame| wire::reply(&frame.body).map(<[u8]>::to_vec));
                let _ = done.send(result);
            });
        }
        waiting.await.map_err(|_| ModuleError::Unavailable)?
    }
    pub(crate) fn cancel(self: &Arc<Self>, id: u64) {
        let owner = self.clone();
        if let Ok(state) = self.ipc.lock()
            && state.available
        {
            self.work.spawn(async move {
                match owner.command(9,id.to_le_bytes().to_vec()).await {Ok(_)|Err(ModuleError::Stopped)=>{},Err(error)=>tracing::error!(%error,original_id=id,"native cancellation was not confirmed")}
            });
        }
    }
    fn deliver_controlled(&self, input: DispatchInput) -> Result<(), ModuleError> {
        let permit = self
            .ipc
            .lock()
            .map_err(|_| ModuleError::Unavailable)?
            .control_permit
            .take()
            .ok_or(ModuleError::InvalidInput)?;
        if permit.send(input).is_closed() {
            Err(ModuleError::Unavailable)
        } else {
            Ok(())
        }
    }
    fn deliver(self: &Arc<Self>, event: DriverEvent) -> Result<(), ModuleError> {
        let mut state = self.ipc.lock().map_err(|_| ModuleError::Unavailable)?;
        let Some(events) = state.events.clone() else {
            self.reject();
            return Err(ModuleError::Unavailable);
        };
        if events.is_closed() {
            self.reject();
            return Err(ModuleError::Unavailable);
        }
        if !state.critical.is_empty() {
            state
                .critical
                .push_back(Critical::Required(Box::new(event)));
            return Ok(());
        }
        match events.try_send(event) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Full(event)) => {
                state
                    .critical
                    .push_back(Critical::Required(Box::new(event)));
                Ok(())
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                self.reject();
                Err(ModuleError::Unavailable)
            }
        }
    }
    async fn event(self: &Arc<Self>, frame: DriverFrame) -> Result<(), ModuleError> {
        let info = self.info.get().ok_or(ModuleError::InvalidInput)?;
        if frame.session != info.session_id {
            return Err(ModuleError::Unauthenticated);
        }
        match frame.message_type {
            0x100 => {
                let (mut request, tail) = wire::controlled(&frame.body, false)?;
                if !tail.is_empty() {
                    return Err(ModuleError::InvalidInput);
                }
                let transfer_hit = request.hit.take();
                let original = Original::new(self, request, info.session_id)?;
                let mut event = original.event();
                if let Some(workload) = docker::event_workload(&self.container_snapshot(), &event) {
                    Arc::make_mut(&mut event).process.workload = Some(workload);
                }
                {
                    let mut state = self.ipc.lock().map_err(|_| ModuleError::Unavailable)?;
                    if !state.available {
                        return Err(ModuleError::Unavailable);
                    }
                    if state.originals.contains_key(&original.id)
                        || state.originals.len() >= info.capabilities.max_pending as usize
                    {
                        return Err(ModuleError::InvalidInput);
                    }
                    state.originals.insert(original.id, original.clone());
                }
                self.deliver_controlled(DispatchInput {
                    event,
                    operation: Some(original),
                    transfer_hit,
                })
            }
            0x101 => {
                let result = wire::result(&frame.body)?;
                if result.id == 0 {
                    return Err(ModuleError::InvalidInput);
                }
                let original = self
                    .ipc
                    .lock()
                    .map_err(|_| ModuleError::Unavailable)?
                    .originals
                    .get(&result.id)
                    .cloned()
                    .ok_or(ModuleError::InvalidInput)?;
                original.receipt(result)?;
                self.ipc
                    .lock()
                    .map_err(|_| ModuleError::Unavailable)?
                    .originals
                    .remove(&original.id);
                Ok(())
            }
            0x102 => {
                let (mut request, tail) = wire::controlled(&frame.body, true)?;
                let result = wire::result(tail)?;
                if result.id != 0
                    || result.generation != request.generation
                    || result.rules != request.rules
                {
                    return Err(ModuleError::InvalidInput);
                }
                let outcome = result.outcome;
                let decision = match outcome {
                    as_types::operation::ExecutionOutcome::Allowed => Decision::Allow,
                    as_types::operation::ExecutionOutcome::Denied => Decision::Deny {
                        terminate: result.termination.requested,
                    },
                    _ => return Err(ModuleError::InvalidInput),
                };
                docker::enrich(&self.container_snapshot(), &mut request.event);
                self.deliver(DriverEvent::DirectResult {
                    event: request.event,
                    choice: RuleChoice {
                        decision,
                        hit: request.hit,
                        reason: ChoiceReason::Direct,
                    },
                    outcome,
                    termination: result.termination,
                })
            }
            0x103 => self.deliver(DriverEvent::Status(wire::status(&frame.body)?)),
            0x10b => self.deliver(DriverEvent::FileClose(wire::file_close(frame.body)?)),
            0x10c => self.deliver(DriverEvent::RequestHandoff(
                runtime_observation::wire::handoff(frame.body, self.host_boot_id)?,
            )),
            0x10a => {
                let event = wire::group_tcp(frame.body)?;
                let _first_loss = self
                    .ipc
                    .lock()
                    .map_err(|_| ModuleError::Unavailable)?
                    .group
                    .as_mut()
                    .ok_or(ModuleError::InvalidInput)?
                    .offer(event);
                Ok(())
            }
            0x109 => self.deliver(DriverEvent::FutexSelection(wire::futex_selection(
                &frame.body,
            )?)),
            0x108 => self.deliver(DriverEvent::RequestReturn(wire::request_return(
                &frame.body,
                frame.session,
            )?)),
            0x104 => {
                let mut event = frame.observation(info.session_id)?;
                if event.header().kind == as_types::main_ports::EventKind::Process
                    && event.header().operation
                        == as_types::event_operation::ProcessOperation::Exit as u32
                    && let (Some(pid), Some(birth)) = (
                        event.header().actor_tgid,
                        event.header().actor_birth_boottime_ns,
                    )
                {
                    self.runtime_observation.forget(pid, birth);
                }
                if event.header().kind == as_types::main_ports::EventKind::Process {
                    use as_types::{
                        dispatch_event::EventPayload, event_operation::ProcessOperation,
                    };
                    let pid = match (event.header().operation, &event.payload) {
                        (op, _) if op == ProcessOperation::Execute as u32 => {
                            event.header().actor_tgid
                        }
                        (op, EventPayload::Process(process))
                            if op == ProcessOperation::Create as u32 =>
                        {
                            process.child_pid
                        }
                        _ => None,
                    };
                    if let Some(pid) = pid {
                        self.runtime_observation.inspect_later(self, pid);
                    }
                }
                docker::enrich(&self.container_snapshot(), &mut event);
                self.deliver(DriverEvent::Observation(event))
            }
            0x105 => {
                if frame.body.len() != 8 {
                    return Err(ModuleError::InvalidInput);
                }
                let generation = wire::u64_at(&frame.body, 0)?;
                if generation == 0 {
                    return Err(ModuleError::InvalidInput);
                }
                let events = self
                    .ipc
                    .lock()
                    .map_err(|_| ModuleError::Unavailable)?
                    .events
                    .clone()
                    .ok_or(ModuleError::Unavailable)?;
                let mut state = self.ipc.lock().map_err(|_| ModuleError::Unavailable)?;
                if !state.generations.contains_key(&generation)
                    || state.critical.iter().any(
                        |pending| matches!(pending,Critical::Retired(value) if *value==generation),
                    )
                {
                    return Err(ModuleError::InvalidInput);
                }
                state.critical.push_back(Critical::Retired(generation));
                drop(events);
                Ok(())
            }
            0x106 => {
                let mut marker = wire::facts_marker(&frame.body)?;
                marker.backend_hit_rejected =
                    self.hit_rejected.load(std::sync::atomic::Ordering::Relaxed);
                let (processed, received) = oneshot::channel();
                let (result, receipt) = watch::channel(None);
                {
                    let mut state = self.ipc.lock().map_err(|_| ModuleError::Unavailable)?;
                    // Intentional retirement cancels fact waiters, but the native
                    // marker can still follow its Quiesce reply in the event FIFO.
                    if state.retiring {
                        return Ok(());
                    }
                    if !state.facts_active
                        || state
                            .facts_expected
                            .is_some_and(|key| key != (marker.tracking_epoch, marker.marker))
                        || !state.facts_markers.is_empty()
                        || state
                            .facts_markers
                            .insert((marker.tracking_epoch, marker.marker), receipt)
                            .is_some()
                    {
                        return Err(ModuleError::InvalidInput);
                    }
                    state
                        .critical
                        .push_back(Critical::Facts { marker, processed });
                }
                self.facts_changed.notify_one();
                let owner = self.clone();
                let mut stopped = self.admission_stop.subscribe();
                self.work.spawn(async move {
                    let value=tokio::select! {biased;
                        _=async{if !*stopped.borrow(){let _=stopped.changed().await;}}=>Err(ModuleError::Stopped),
                        received=received=>received.map(|()|{let mut marker=marker;marker.backend_rejected = *owner.rejected.borrow();marker}).map_err(|_|ModuleError::Unavailable)
                    };
                    result.send_replace(Some(value));
                });
                Ok(())
            }
            0x107 => {
                let batch = wire::direct_hits(&frame.body)?;
                let state = self.ipc.lock().map_err(|_| ModuleError::Unavailable)?;
                let accepted = state.critical.is_empty()
                    && state.events.as_ref().is_some_and(|events| {
                        (events.capacity() > 1 || events.max_capacity() == 1)
                            && events.try_send(DriverEvent::DirectHits(batch)).is_ok()
                    });
                if !accepted {
                    let _ = self.hit_rejected.fetch_update(
                        std::sync::atomic::Ordering::Relaxed,
                        std::sync::atomic::Ordering::Relaxed,
                        |value| Some(value.saturating_add(1)),
                    );
                }
                Ok(())
            }
            _ => Err(ModuleError::InvalidInput),
        }
    }
}
/// The receive pump owns at most one Group body; priority controls/completions may interleave.
fn group_frame(
    pending: &mut Option<DriverFrame>,
    mut frame: DriverFrame,
) -> Result<Option<DriverFrame>, ModuleError> {
    if frame.class == FrameClass::Reply
        || (frame.class == FrameClass::Event && matches!(frame.message_type, 0x100 | 0x101))
    {
        return Ok(Some(frame));
    }
    if frame.class != FrameClass::Event || frame.message_type != 0x10a {
        return if pending.is_some() {
            Err(ModuleError::InvalidInput)
        } else {
            Ok(Some(frame))
        };
    }
    if frame.request_id != 0 || frame.body.is_empty() || frame.total_bytes > wire::MAX_TRANSFER {
        return Err(ModuleError::InvalidInput);
    }
    let end = frame
        .offset
        .checked_add(frame.body.len() as u64)
        .filter(|end| *end <= frame.total_bytes)
        .ok_or(ModuleError::InvalidInput)?;
    if let Some(original) = pending.as_mut() {
        if original.session != frame.session
            || original.class != frame.class
            || original.message_type != frame.message_type
            || original.request_id != frame.request_id
            || original.total_bytes != frame.total_bytes
            || frame.offset != original.body.len() as u64
        {
            return Err(ModuleError::InvalidInput);
        }
        original.body.append(&mut frame.body);
        return Ok(if end == original.total_bytes {
            pending.take()
        } else {
            None
        });
    }
    if frame.offset != 0 {
        return Err(ModuleError::InvalidInput);
    }
    if end == frame.total_bytes {
        return Ok(Some(frame));
    }
    let total = usize::try_from(frame.total_bytes).map_err(|_| ModuleError::InvalidInput)?;
    frame
        .body
        .try_reserve_exact(total - frame.body.len())
        .map_err(|_| ModuleError::Busy)?;
    *pending = Some(frame);
    Ok(None)
}

async fn receive(owner: Weak<Shared>) -> Result<(), ModuleError> {
    // Only this terminal source failure is retained, not successfully consumed frames.
    let diagnostic = Mutex::new(None::<String>);
    let capture = |line: String| {
        let mut pending = match diagnostic.lock() {
            Ok(value) => value,
            Err(poison) => poison.into_inner(),
        };
        if let Some(previous) = pending.as_mut() {
            previous.push('\n');
            previous.push_str(&line);
        } else {
            *pending = Some(line);
        }
    };
    let result = async {
    let failed = |stage: &str, error: ModuleError, pending: Option<&DriverFrame>| {
        capture(format!(
            "AS_NATIVE_RECEIVE_FAILED stage={stage} error={error:?} pending_group_total_bytes={:?} pending_group_received_bytes={:?}",
            pending.map(|frame| frame.total_bytes),
            pending.map(|frame| frame.body.len())
        ));
        error
    };
    let Some(initial) = owner.upgrade() else {
        return Ok(());
    };
    let mut stop = initial.stop.subscribe();
    drop(initial);
    let mut group = None;
    let mut priority = None;
    loop {
        let Some(owner) = owner.upgrade() else {
            return if group.is_some() {
                Err(failed(
                    "owner_ended",
                    ModuleError::Unavailable,
                    group.as_ref(),
                ))
            } else {
                Ok(())
            };
        };
        if *stop.borrow() {
            if group.is_some() {
                let error = failed(
                    "stop_incomplete_group",
                    ModuleError::InvalidInput,
                    group.as_ref(),
                );
                owner.fail();
                return Err(error);
            }
            let state = match owner.ipc.lock() {
                Ok(state) => state,
                Err(_) => return Err(failed("stop_state", ModuleError::Unavailable, None)),
            };
            if state.critical.is_empty() {
                return Ok(());
            }
        }
        let io = match owner.file() {
            Ok(io) => io,
            Err(error) => {
                let error = failed("native_file", error, group.as_ref());
                owner.fail();
                return Err(error);
            }
        };
        let mut bytes = vec![0; wire::MAX_FRAME];
        let draining = *stop.borrow();
        let (critical, controlled, paused) = {
            let state = match owner.ipc.lock() {
                Ok(state) => state,
                Err(_) => {
                    return Err(failed(
                        "critical_state",
                        ModuleError::Unavailable,
                        group.as_ref(),
                    ));
                }
            };
            (
                (!state.critical.is_empty())
                    .then(|| state.events.clone())
                    .flatten(),
                (!draining && state.available && state.control_permit.is_none())
                    .then(|| state.controlled.clone())
                    .flatten(),
                state
                    .critical
                    .iter()
                    .any(|entry| matches!(entry, Critical::Required(_))),
            )
        };
        if paused && !draining && priority.is_none() {
            priority = match completion::poll(io.get_ref()) {
                Ok(poll) => Some(poll),
                Err(error) => {
                    capture(format!(
                        "AS_NATIVE_RECEIVE_FAILED stage=completion_poll error={:?} io_kind={:?} native_error={:?} source={error}",
                        ModuleError::Unavailable,
                        error.kind(),
                        error.raw_os_error()
                    ));
                    owner.fail();
                    return Err(ModuleError::Unavailable);
                }
            };
        }
        let delivery = async {
            match critical {
                Some(events) => events
                    .reserve_owned()
                    .await
                    .map_err(|_| ModuleError::Stopped),
                None => std::future::pending().await,
            }
        };
        let control_delivery = async {
            match controlled {
                Some(output) => output
                    .reserve_owned()
                    .await
                    .map_err(|_| ModuleError::Stopped),
                None => std::future::pending().await,
            }
        };
        let reading = async {
            if draining {
                std::future::pending().await
            } else if paused {
                match &priority {
                    Some(poll) => completion::read(poll, io.get_ref(), &mut bytes).await,
                    None => Err(std::io::Error::other("completion poll missing")),
                }
            } else {
                io.read_with(|file| {
                    let mut file = file;
                    file.read(&mut bytes)
                })
                .await
            }
        };
        let count = tokio::select! {biased;
            _ = stop.changed()=>{
                if group.is_some() {let error=failed("stop_incomplete_group",ModuleError::InvalidInput,group.as_ref());owner.fail();return Err(error);}
                continue;
            },
            delivery=control_delivery=>{
                {
                    let mut state=owner.ipc.lock().map_err(|_|ModuleError::Unavailable)?;
                    // A completed reservation does not authorize a new credit after final retirement.
                    if !state.available || *stop.borrow() { continue; }
                    let permit=match delivery{Ok(value)=>value,Err(error)=>{drop(state);let error=failed("controlled_reserve",error,group.as_ref());owner.fail();return Err(error);}};
                    if state.control_permit.is_some() { return Err(ModuleError::Unavailable); }
                    state.control_permit=Some(permit);
                }
                // The actual queue slot is held before the native producer receives one credit.
                let granted=owner.clone();
                owner.work.spawn(async move {
                    let result=granted.exchange(18,Vec::new()).await
                        .and_then(|frame|wire::reply(&frame.body).map(|_|()));
                    if let Err(error)=result {
                        granted.fail();
                        as_types::diagnostics::record_stderr(format!("AS_NATIVE_CONTROL_CREDIT_FAILED error={error}")).await;
                    }
                });
                continue;
            },
            delivery=delivery=>{
                let permit=match delivery{Ok(value)=>value,Err(error)=>{let error=failed("critical_reserve",error,group.as_ref());owner.fail();return Err(error);}};
                let mut state=match owner.ipc.lock(){Ok(state)=>state,Err(_)=>{let error=failed("critical_state",ModuleError::Unavailable,group.as_ref());owner.fail();return Err(error);}};
                let event=match state.critical.pop_front(){
                    Some(Critical::Required(event))=>*event,
                    Some(Critical::Retired(generation))=>{if state.generations.remove(&generation).is_none(){drop(state);let error=failed("critical_generation",ModuleError::InvalidInput,group.as_ref());owner.fail();return Err(error);}DriverEvent::PersistentRetired{generation}},
                    Some(Critical::Facts{mut marker,processed})=>{marker.backend_rejected = *owner.rejected.borrow();DriverEvent::FactsBarrier{marker,processed}},
                    _=>{drop(state);let error=failed("critical_empty",ModuleError::InvalidInput,group.as_ref());owner.fail();return Err(error);}
                };
                drop(state);
                let sender=permit.send(event);
                if sender.is_closed(){let error=failed("critical_dispatch",ModuleError::Unavailable,group.as_ref());owner.fail();return Err(error);}
                continue;
            },
            count=reading=>match count {
                Ok(0)=>{let error=failed("native_eof",ModuleError::Unavailable,group.as_ref());owner.fail();return Err(error);},
                Err(error)=>{
                    capture(format!("AS_NATIVE_RECEIVE_FAILED stage=native_read error={:?} io_kind={:?} native_error={:?} source={error}",ModuleError::Unavailable,error.kind(),error.raw_os_error()));
                    owner.fail();return Err(ModuleError::Unavailable);
                },
                Ok(count)=>count
            }
        };
        let frame = match DriverFrame::decode(&bytes[..count], wire::MAX_FRAME, wire::MAX_TRANSFER)
        {
            Ok(frame) => frame,
            Err(error) => {
                capture(format!(
                    "AS_NATIVE_RECEIVE_FAILED stage=frame_decode error={error:?} read_bytes={count}"
                ));
                owner.fail();
                return Err(error);
            }
        };
        let (class, message_type, request_id, total_bytes, offset, body_bytes) = (
            frame.class,
            frame.message_type,
            frame.request_id,
            frame.total_bytes,
            frame.offset,
            frame.body.len(),
        );
        let frame_failed = |stage: &str, error: ModuleError| {
            capture(format!(
                "AS_NATIVE_RECEIVE_FAILED stage={stage} error={error:?} class={class:?} type={message_type:#06x} request_id={request_id} read_bytes={count} frame_body_bytes={body_bytes} total_bytes={total_bytes} offset={offset}"
            ));
            error
        };
        if paused
            && frame.class != FrameClass::Reply
            && !(frame.class == FrameClass::Event && matches!(frame.message_type, 0x100 | 0x101))
        {
            let error = frame_failed("completion_class", ModuleError::InvalidInput);
            owner.fail();
            return Err(error);
        }
        if let Some(info) = owner.info.get() {
            if frame.session != info.session_id {
                let error = frame_failed("frame_session", ModuleError::Unauthenticated);
                owner.fail();
                return Err(error);
            }
        } else if frame.class != FrameClass::Reply || frame.message_type != 1 {
            let error = frame_failed("initial_reply", ModuleError::InvalidInput);
            owner.fail();
            return Err(error);
        }
        let frame = match group_frame(&mut group, frame) {
            Ok(Some(frame)) => frame,
            Ok(None) => continue,
            Err(error) => {
                let error = frame_failed("group_reassembly", error);
                owner.fail();
                return Err(error);
            }
        };
        let stage = match frame.class {
            FrameClass::Reply => "reply_dispatch",
            FrameClass::Event => "event_decode_or_dispatch",
            FrameClass::Request => "unexpected_request",
        };
        let result = match frame.class {
            FrameClass::Reply => {
                let slot = owner
                    .ipc
                    .lock()
                    .ok()
                    .and_then(|mut state| state.replies.remove(&frame.request_id));
                match slot {
                    Some((kind, waiting)) if kind == frame.message_type => {
                        let _ = waiting.send(Ok(frame));
                        Ok(())
                    }
                    Some((_, waiting)) => {
                        let _ = waiting.send(Err(ModuleError::InvalidInput));
                        Err(ModuleError::InvalidInput)
                    }
                    None => Err(ModuleError::InvalidInput),
                }
            }
            FrameClass::Event => owner.event(frame).await,
            FrameClass::Request => Err(ModuleError::InvalidInput),
        };
        if let Err(error) = result {
            let error = frame_failed(stage, error);
            owner.fail();
            return Err(error);
        }
    }

    }.await;
    if result.is_err()
        && let Some(source) = owner.upgrade()
    {
        source.fail();
    }
    let diagnostic = match diagnostic.into_inner() {
        Ok(value) => value,
        Err(poison) => poison.into_inner(),
    };
    if let Some(diagnostic) = diagnostic {
        as_types::diagnostics::record_stderr(diagnostic).await;
    }
    result
}

/// Concrete Linux platform adapter. No production software driver or alternate endpoint is selected.
#[derive(Debug, Default)]
pub struct LinuxDriverAdapter;
type StopReceipt = watch::Receiver<Option<Result<DriverStatus, ModuleError>>>;
struct NativeSession {
    shared: Arc<Shared>,
    info: DriverInfo,
    reader: Mutex<Option<tokio::task::JoinHandle<Result<(), ModuleError>>>>,
    loader: Mutex<Option<Loader>>,
    close: Mutex<Close>,
    quiesce: Mutex<Option<StopReceipt>>,
}
enum Close {
    Open,
    Running(watch::Receiver<Option<Result<(), ModuleError>>>),
}
struct StartupGuard(Option<Arc<Shared>>);
impl Drop for StartupGuard {
    fn drop(&mut self) {
        if let Some(owner) = self.0.take() {
            owner.fail();
        }
    }
}
#[async_trait::async_trait]
impl DriverAdapter for LinuxDriverAdapter {
    async fn connect(
        &self,
        input: &DriverConnectInput,
    ) -> Result<Arc<dyn DriverSession>, ModuleError> {
        tokio::runtime::Handle::try_current().map_err(|_| ModuleError::Unavailable)?;
        let input = input.clone();
        let (sent, waiting) = oneshot::channel();
        // The started connection/loader work remains owned if its caller stops waiting.
        tokio::spawn(async move {
            if sent.is_closed() {
                return;
            }
            let result = LinuxDriverAdapter::connect_owned(input).await;
            if let Err(Ok(session)) = sent.send(result) {
                let _ = session.shutdown().await;
            }
        });
        waiting.await.map_err(|_| ModuleError::Unavailable)?
    }
}
impl LinuxDriverAdapter {
    async fn connect_owned(
        input: DriverConnectInput,
    ) -> Result<Arc<dyn DriverSession>, ModuleError> {
        if input.platform != TargetPlatform::Linux {
            return Err(ModuleError::Unsupported);
        }
        if input.host_boot_id == [0; 16] {
            return Err(ModuleError::InvalidInput);
        }
        let file = tokio::task::spawn_blocking(|| {
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(
                    (rustix::fs::OFlags::NONBLOCK
                        | rustix::fs::OFlags::CLOEXEC
                        | rustix::fs::OFlags::NOFOLLOW)
                        .bits() as i32,
                )
                .open("/dev/as_wait")
                .map_err(|error| {
                    tracing::error!(stage="open_native_endpoint",errno=?error.raw_os_error(),%error,"native connection failed");
                    ModuleError::Unavailable
                })?;
            let meta = file.metadata().map_err(|error| {
                tracing::error!(stage="native_endpoint_metadata",errno=?error.raw_os_error(),%error,"native connection failed");
                ModuleError::Unavailable
            })?;
            let actual = std::fs::read_to_string("/sys/class/misc/as_wait/dev")
                .map_err(|error| {
                    tracing::error!(stage="read_native_device_identity",errno=?error.raw_os_error(),%error,"native connection failed");
                    ModuleError::Unauthenticated
                })?;
            let expected = format!(
                "{}:{}",
                rustix::fs::major(meta.rdev()),
                rustix::fs::minor(meta.rdev())
            );
            if !meta.file_type().is_char_device()
                || meta.uid() != 0
                || meta.mode() & 0o077 != 0
                || actual.trim() != expected
            {
                tracing::error!(stage="verify_native_device_identity","native endpoint identity rejected");
                return Err(ModuleError::Unauthenticated);
            }
            Async::new(file).map_err(|error| {
                tracing::error!(stage="register_native_endpoint_io",errno=?error.raw_os_error(),%error,"native connection failed");
                ModuleError::Unavailable
            })
        })
        .await
        .map_err(|_| ModuleError::Unavailable)??;
        let shared = Shared::new(file, input.host_boot_id);
        let mut startup = StartupGuard(Some(shared.clone()));
        let reader = tokio::spawn(receive(Arc::downgrade(&shared)));
        let hello = shared.exchange(1, Vec::new()).await;
        let mut info =
            match hello.and_then(|frame| wire::hello(wire::reply(&frame.body)?, frame.session)) {
                Ok(info) => info,
                Err(error) => {
                    tracing::error!(stage="native_hello",%error,"native connection failed");
                    shared.fail();
                    let _ = reader.await;
                    return Err(error);
                }
            };
        if info
            .capabilities
            .supports_feature(DriverFeature::ProcessFacts)
        {
            // This source is owned by the authenticated adapter, not the kernel rule VM.
            info.capabilities.features |= DriverFeature::ContainerBinding as u32;
        }
        if shared.info.set(info.clone()).is_err() {
            shared.fail();
            let _ = reader.await;
            return Err(ModuleError::InvalidInput);
        }
        let loader = match Loader::start(&input.driver_assets_directory, info.status.instance).await
        {
            Ok(loader) => loader,
            Err(error) => {
                tracing::error!(stage="start_bpf_loader",%error,"native connection failed");
                shared.fail();
                let _ = reader.await;
                return Err(error);
            }
        };
        shared
            .assets
            .set(input.driver_assets_directory)
            .map_err(|_| ModuleError::Unavailable)?;
        startup.0.take();
        Ok(Arc::new(NativeSession {
            shared,
            info,
            reader: Mutex::new(Some(reader)),
            loader: Mutex::new(Some(loader)),
            close: Mutex::new(Close::Open),
            quiesce: Mutex::new(None),
        }))
    }
}
async fn stop_result(mut receiving: StopReceipt) -> Result<DriverStatus, ModuleError> {
    loop {
        if let Some(result) = receiving.borrow_and_update().clone() {
            return result;
        }
        receiving
            .changed()
            .await
            .map_err(|_| ModuleError::Unavailable)?;
    }
}

impl NativeSession {
    async fn process_marker(&self, kind: u16) -> Result<ProcessFactsBarrier, ModuleError> {
        let owner = self.shared.clone();
        let (done, waiting) = oneshot::channel();
        {
            let state = owner.ipc.lock().map_err(|_| ModuleError::Unavailable)?;
            if !state.available || state.retiring {
                return Err(ModuleError::Stopped);
            }
            let work = owner.clone();
            owner.work.spawn(async move {
                let _serial = work.facts_serial.lock().await;
                let result = async {
                    if *work.admission_stop.borrow() {
                        return Err(ModuleError::Stopped);
                    }
                    {
                        let mut state = work.ipc.lock().map_err(|_| ModuleError::Unavailable)?;
                        if state.facts_active {
                            return Err(ModuleError::Conflict);
                        }
                        state.facts_active = true;
                    }
                    let raw = work.command(kind, Vec::new()).await?;
                    let target = wire::facts_marker(&raw)?;
                    {
                        let mut state=work.ipc.lock().map_err(|_|ModuleError::Unavailable)?;
                        let key=(target.tracking_epoch,target.marker);
                        if state.facts_markers.keys().any(|seen|*seen!=key){return Err(ModuleError::InvalidInput);}
                        state.facts_expected=Some(key);
                    }
                    let mut stopped = work.admission_stop.subscribe();
                    let mut receipt = loop {
                        let changed = work.facts_changed.notified();
                        let receipt = work.ipc.lock().map_err(|_| ModuleError::Unavailable)?
                            .facts_markers.get(&(target.tracking_epoch, target.marker)).cloned();
                        if let Some(receipt) = receipt {
                            break receipt;
                        }
                        tokio::select! {
                            biased;
                            _ = async { if !*stopped.borrow() { let _ = stopped.changed().await; } } => return Err(ModuleError::Stopped),
                            _ = changed => {}
                        }
                    };
                    loop {
                        if let Some(result) = *receipt.borrow_and_update() {
                            let marker = result?;
                            if marker.host_pid_namespace_inode != target.host_pid_namespace_inode
                                || marker.loss_epoch < target.loss_epoch
                                || marker.native_hit_loss < target.native_hit_loss
                                || marker.cutoff_boottime_ns != target.cutoff_boottime_ns {
                                return Err(ModuleError::InvalidInput);
                            }
                            return Ok(marker);
                        }
                        receipt.changed().await.map_err(|_| ModuleError::Unavailable)?;
                    }
                }.await;
                if let Ok(mut state) = work.ipc.lock() {
                    state.facts_active = false;
                    state.facts_expected = None;
                    state.facts_markers.clear();
                }
                let _ = done.send(result);
            });
        }
        waiting.await.map_err(|_| ModuleError::Unavailable)?
    }
    fn stop_request(&self) -> Result<StopReceipt, ModuleError> {
        let mut slot = self.quiesce.lock().map_err(|_| ModuleError::Unavailable)?;
        if let Some(receiving) = &*slot {
            return Ok(receiving.clone());
        }
        self.shared
            .ipc
            .lock()
            .map_err(|_| ModuleError::Unavailable)?
            .retiring = true;
        self.shared.admission_stop.send_replace(true);
        self.shared.facts_changed.notify_one();
        if let Some(budget) = self.shared.generation_budget.get() {
            budget.close();
        }
        let (sent, receiving) = watch::channel(None);
        let shared = self.shared.clone();
        let instance = self.info.status.instance;
        self.shared.work.spawn(async move {
            let result = shared
                .command(5, Vec::new())
                .await
                .and_then(|raw| wire::status(&raw))
                .and_then(|status| {
                    if status.instance == instance {
                        Ok(status)
                    } else {
                        Err(ModuleError::Unauthenticated)
                    }
                });
            let _ = sent.send(Some(result));
        });
        *slot = Some(receiving.clone());
        Ok(receiving)
    }
    async fn adopt(
        &self,
        kind: u16,
        parts: &DriverInitParts,
    ) -> Result<AppliedPersistentState, ModuleError> {
        let body = wire::images(parts, &self.info)?;
        let expected_generation = parts
            .expected_persistent_generation
            .checked_add(1)
            .ok_or(ModuleError::InvalidInput)?;
        let config = parts.configuration.driver_config_version;
        let rules = parts.rules.rule_version;
        let instance = self.info.status.instance;
        let shared = self.shared.clone();
        let (done, waiting) = oneshot::channel();
        {
            let state = self
                .shared
                .ipc
                .lock()
                .map_err(|_| ModuleError::Unavailable)?;
            if !state.available {
                return Err(ModuleError::Unavailable);
            }
            if state.retiring {
                return Err(ModuleError::Stopped);
            }
            self.shared.work.spawn(async move {
                let result = async {
                    let budget = shared
                        .generation_budget
                        .get()
                        .ok_or(ModuleError::Unavailable)?
                        .clone();
                    let permit = budget
                        .acquire_owned()
                        .await
                        .map_err(|_| ModuleError::Stopped)?;
                    let _candidate = shared.bulk.lock().await;
                    {
                        let state = shared.ipc.lock().map_err(|_| ModuleError::Unavailable)?;
                        if state.retiring {
                            return Err(ModuleError::Stopped);
                        }
                    }
                    let frame = match shared.exchange(kind, body).await {
                        Ok(frame) => frame,
                        Err(error) => {
                            shared
                                .ipc
                                .lock()
                                .map_err(|_| ModuleError::Unavailable)?
                                .unconfirmed
                                .push(permit);
                            return Err(error);
                        }
                    };
                    let raw = match wire::reply(&frame.body) {
                        Ok(raw) => raw,
                        Err(error) if wire::rejected_reply(&frame.body) => return Err(error),
                        Err(error) => {
                            shared
                                .ipc
                                .lock()
                                .map_err(|_| ModuleError::Unavailable)?
                                .unconfirmed
                                .push(permit);
                            shared.fail();
                            return Err(error);
                        }
                    };
                    let status = match wire::status(raw) {
                        Ok(status) => status,
                        Err(error) => {
                            shared
                                .ipc
                                .lock()
                                .map_err(|_| ModuleError::Unavailable)?
                                .unconfirmed
                                .push(permit);
                            shared.fail();
                            return Err(error);
                        }
                    };
                    if status.instance != instance
                        || status.persistent_generation != expected_generation
                        || status.driver_config_version != Some(config)
                        || status.rule_version != Some(rules)
                    {
                        shared
                            .ipc
                            .lock()
                            .map_err(|_| ModuleError::Unavailable)?
                            .unconfirmed
                            .push(permit);
                        shared.fail();
                        return Err(ModuleError::Unavailable);
                    }
                    if shared
                        .ipc
                        .lock()
                        .map_err(|_| ModuleError::Unavailable)?
                        .generations
                        .insert(status.persistent_generation, permit)
                        .is_some()
                    {
                        shared.fail();
                        return Err(ModuleError::InvalidInput);
                    }
                    if status.source_state != SourceState::Running {
                        return Err(ModuleError::Unavailable);
                    }
                    Ok(AppliedPersistentState {
                        persistent_generation: status.persistent_generation,
                        status,
                    })
                }
                .await;
                let _ = done.send(result);
            });
        }
        waiting.await.map_err(|_| ModuleError::Unavailable)?
    }
}
#[async_trait::async_trait]
impl DriverControl for NativeSession {
    async fn request(&self, command: DriverCommand) -> Result<DriverStatus, ModuleError> {
        if !self
            .shared
            .ipc
            .lock()
            .map_err(|_| ModuleError::Unavailable)?
            .available
        {
            return Err(ModuleError::Stopped);
        }
        let (kind, body) = match command {
            DriverCommand::GetStatus => (4, Vec::new()),
            DriverCommand::Quiesce => return stop_result(self.stop_request()?).await,
            DriverCommand::DeclareFailure => (6, Vec::new()),
            DriverCommand::SetRuntimeFeature { feature, enabled } => {
                let mut body = wire::feature_id(feature).to_le_bytes().to_vec();
                body.extend(u32::from(enabled).to_le_bytes());
                (10, body)
            }
            DriverCommand::ResumeRuntimeFeature { feature } => {
                (11, wire::feature_id(feature).to_le_bytes().to_vec())
            }
        };
        let raw = self.shared.command(kind, body).await?;
        let status = wire::status(&raw)?;
        if status.instance != self.info.status.instance {
            return Err(ModuleError::Unauthenticated);
        }
        Ok(status)
    }
}
#[async_trait::async_trait]
impl DriverTools for NativeSession {
    async fn kill_process(
        &self,
        target: DriverProcessTarget,
    ) -> Result<KillProcessOutcome, ModuleError> {
        if !self
            .info
            .capabilities
            .supports_feature(DriverFeature::ProcessKill)
        {
            return Err(ModuleError::Unsupported);
        }
        if target.driver_instance != self.info.status.instance {
            return Err(ModuleError::Conflict);
        }
        if target.host_tgid == 0 || target.host_tgid > i32::MAX as u32 {
            return Err(ModuleError::InvalidInput);
        }
        let mut body = target.driver_instance.to_vec();
        body.extend_from_slice(&target.host_tgid.to_le_bytes());
        body.extend_from_slice(&0u32.to_le_bytes());
        body.extend_from_slice(&target.birth_boottime_ns.to_le_bytes());
        let raw = self.shared.command(16, body).await?;
        if raw.len() != 8 || wire::u32_at(&raw, 4)? != 0 {
            return Err(ModuleError::InvalidInput);
        }
        match wire::u32_at(&raw, 0)? {
            0 => Ok(KillProcessOutcome::SignalSent),
            1 => Ok(KillProcessOutcome::NotRunning),
            _ => Err(ModuleError::InvalidInput),
        }
    }
}
impl Shared {
    async fn inspect(
        self: &Arc<Self>,
        host_tgids: &[u32],
    ) -> Result<Vec<ProcessInspection>, ModuleError> {
        if host_tgids.is_empty() || host_tgids.len() > 64 || host_tgids.contains(&0) {
            return Err(ModuleError::InvalidInput);
        }
        let mut body = Vec::with_capacity(8 + 4 * host_tgids.len());
        body.extend_from_slice(&(host_tgids.len() as u32).to_le_bytes());
        body.extend_from_slice(&0u32.to_le_bytes());
        for pid in host_tgids {
            body.extend_from_slice(&pid.to_le_bytes());
        }
        let raw = self.command(14, body).await?;
        wire::process_inspections(&raw, host_tgids)
    }
}
#[async_trait::async_trait]
impl ProcessFactsPort for NativeSession {
    async fn inspect(&self, host_tgids: &[u32]) -> Result<Vec<ProcessInspection>, ModuleError> {
        let records = self.shared.inspect(host_tgids).await?;
        self.shared
            .runtime_observation
            .observe(&self.shared, &records);
        Ok(records)
    }

    fn workload(&self, cgroup_id: u64, observed: u64) -> Option<as_types::workload::Workload> {
        docker::workload(&self.shared.container_snapshot(), cgroup_id, observed)
    }
    fn outside_containers(
        &self,
        instances: &[as_types::workload::ContainerInstance],
        lineage: &[u64],
        observed: u64,
    ) -> bool {
        docker::outside_containers(
            &self.shared.container_snapshot(),
            instances,
            lineage,
            observed,
        )
    }
    async fn facts_barrier(&self) -> Result<ProcessFactsBarrier, ModuleError> {
        self.process_marker(15).await
    }
}
#[async_trait::async_trait]
impl DriverSession for NativeSession {
    fn bind_group_tcp(
        &self,
        events: mpsc::Sender<as_types::driver::NativeGroupTcp>,
    ) -> Result<(), ModuleError> {
        let mut state = self
            .shared
            .ipc
            .lock()
            .map_err(|_| ModuleError::Unavailable)?;
        if !state.available || state.retiring {
            return Err(ModuleError::Stopped);
        }
        if state.group.is_some() {
            return Err(ModuleError::Conflict);
        }
        state.group = Some(GroupSink {
            events,
            full: 0,
            closed: 0,
            first_rejection: None,
        });
        Ok(())
    }

    async fn start_tls_ingress(
        &self,
        input: TlsIngressInput,
        output: mpsc::Sender<as_types::flow::AdmittedConnection>,
    ) -> Result<Arc<dyn TlsIngressSession>, ModuleError> {
        let lifetime = self.shared.retain_admission()?;
        let path = self.shared.assets.get().ok_or(ModuleError::Unavailable)?;
        tls_ingress::start(
            path,
            self.info.status.instance,
            input,
            output,
            Arc::downgrade(&self.shared),
            lifetime,
        )
        .await
    }

    fn info(&self) -> DriverInfo {
        self.info.clone()
    }
    fn rejected_callbacks(&self) -> u64 {
        *self.shared.rejected.borrow()
    }
    fn callback_loss(&self) -> watch::Receiver<u64> {
        self.shared.rejected.subscribe()
    }
    async fn start_process_tracking(
        &self,
        callbacks: DriverCallbacks,
    ) -> Result<ProcessTrackingStart, ModuleError> {
        let DriverCallbacks { controlled, events } = callbacks;
        if !self
            .info
            .capabilities
            .supports_feature(DriverFeature::ProcessFacts)
        {
            return Err(ModuleError::Unsupported);
        }
        {
            let mut state = self
                .shared
                .ipc
                .lock()
                .map_err(|_| ModuleError::Unavailable)?;
            if !state.available || state.retiring {
                return Err(ModuleError::Stopped);
            }
            if state.events.is_some() {
                return Err(ModuleError::Conflict);
            }
            state.events = Some(events.clone());
            state.controlled = Some(controlled);
            if self.shared.generation_budget.get().is_none() {
                let budget = Arc::new(Semaphore::new(
                    events
                        .max_capacity()
                        .checked_add(1)
                        .filter(|v| *v <= Semaphore::MAX_PERMITS)
                        .ok_or(ModuleError::Busy)?,
                ));
                if self.info.status.driver_config_version.is_some() {
                    let permit = budget
                        .clone()
                        .try_acquire_owned()
                        .map_err(|_| ModuleError::Busy)?;
                    state
                        .generations
                        .insert(self.info.status.persistent_generation, permit);
                }
                self.shared
                    .generation_budget
                    .set(budget)
                    .map_err(|_| ModuleError::InvalidInput)?;
            }
        }
        let marker = self.process_marker(13).await?;
        let (ready, initialized) = oneshot::channel();
        docker::start(self.shared.clone(), ready, self.shared.retain_admission()?);
        initialized.await.map_err(|_| ModuleError::Stopped)?;
        Ok(ProcessTrackingStart {
            host_pid_namespace_inode: marker.host_pid_namespace_inode,
            tracking_epoch: marker.tracking_epoch,
            begin_marker: marker.marker,
            loss_epoch: marker.loss_epoch,
            backend_rejected: marker.backend_rejected,
            cutoff_boottime_ns: marker.cutoff_boottime_ns,
        })
    }
    async fn initialize(
        &self,
        parts: &DriverInitParts,
    ) -> Result<AppliedPersistentState, ModuleError> {
        self.adopt(2, parts).await
    }
    async fn replace_persistent(
        &self,
        parts: &DriverInitParts,
    ) -> Result<AppliedPersistentState, ModuleError> {
        self.adopt(3, parts).await
    }
    fn stop_admission(&self) -> Result<(), ModuleError> {
        self.stop_request()?;
        Ok(())
    }
    async fn shutdown(&self) -> Result<(), ModuleError> {
        let mut receiving = {
            let mut close = self.close.lock().map_err(|_| ModuleError::Unavailable)?;
            match &*close {
                Close::Running(receiving) => receiving.clone(),
                Close::Open => {
                    let stopping = self.stop_request()?;
                    let (sent, receiving) = watch::channel(None);
                    let shared = self.shared.clone();
                    let reader = self
                        .reader
                        .lock()
                        .map_err(|_| ModuleError::Unavailable)?
                        .take();
                    let loader = self
                        .loader
                        .lock()
                        .map_err(|_| ModuleError::Unavailable)?
                        .take();
                    tokio::spawn(async move {
                        let result = async {
                            let quiesced = stop_result(stopping).await;
                            if quiesced.is_err() {
                                shared.fail();
                            }
                            // Stop all accepted native jobs before releasing this connection.
                            shared.work.close();
                            shared.work.wait().await;
                            // The shared Quiesce receipt is not a mutable current-state cache.
                            // After all accepted jobs finish, verify a fresh actual stop proof.
                            let quiesced = match quiesced {
                                Ok(_) => shared.exchange(4, Vec::new()).await.and_then(|frame| {
                                    wire::reply(&frame.body).and_then(wire::status)
                                }),
                                Err(error) => Err(error),
                            };
                            if quiesced.is_err() {
                                shared.fail();
                            }
                            let mut retire = quiesced.as_ref().is_ok_and(|s| {
                                s.pending == 0
                                    && s.pending_receipts == 0
                                    && (s.source_state == SourceState::Quiescing
                                        || (s.source_state == SourceState::Dormant
                                            && s.driver_config_version.is_none()
                                            && s.rule_version.is_none()))
                            });
                            {
                                let mut state =
                                    shared.ipc.lock().map_err(|_| ModuleError::Unavailable)?;
                                let healthy = state.available;
                                state.available = false;
                                retire &= healthy;
                            }
                            shared.stop.send_replace(true);
                            let received = match reader {
                                Some(reader) => reader
                                    .await
                                    .map_err(|_| ModuleError::Unavailable)
                                    .and_then(|result| result),
                                None => Ok(()),
                            };
                            retire &= received.is_ok();
                            shared
                                .io
                                .lock()
                                .map_err(|_| ModuleError::Unavailable)?
                                .take();
                            let group_diagnostic = {
                                let mut state =
                                    shared.ipc.lock().map_err(|_| ModuleError::Unavailable)?;
                                state.generations.clear();
                                state.unconfirmed.clear();
                                state.controlled.take();
                                state.control_permit.take();
                                state.events.take();
                                let current = state.group.take().and_then(GroupSink::retire);
                                match (state.group_diagnostic.take(), current) {
                                    (Some(mut previous), Some(current)) => {
                                        previous.push('\n');
                                        previous.push_str(&current);
                                        Some(previous)
                                    }
                                    (previous, current) => previous.or(current),
                                }
                            };
                            let unloaded = match loader {
                                Some(loader) => loader.finish(retire).await,
                                None => Ok(()),
                            };
                            if let Some(diagnostic) = group_diagnostic {
                                as_types::diagnostics::record_stderr(diagnostic).await;
                            }
                            let final_status = quiesced?;
                            received?;
                            unloaded?;
                            if !retire || final_status.source_state == SourceState::Failed {
                                return Err(ModuleError::Unavailable);
                            }
                            Ok(())
                        }
                        .await;
                        if result.is_err() {
                            shared.fail();
                        }
                        // A TLS exit error is reported after native cleanup; it
                        // must not turn a healthy native retirement into failure.
                        let result = result.and_then(|()| shared.tls_cleanup_result());
                        let _ = sent.send(Some(result));
                    });
                    *close = Close::Running(receiving.clone());
                    receiving
                }
            }
        };
        loop {
            if let Some(result) = *receiving.borrow_and_update() {
                return result;
            }
            receiving
                .changed()
                .await
                .map_err(|_| ModuleError::Unavailable)?;
        }
    }
}
impl Drop for NativeSession {
    fn drop(&mut self) {
        if !self
            .close
            .get_mut()
            .is_ok_and(|state| matches!(state, Close::Running(_)))
        {
            self.shared.fail();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::{
        fd::OwnedFd,
        unix::net::{UnixDatagram, UnixStream},
    };

    fn group_packet(payload: &[u8], session: [u8; 16]) -> DriverFrame {
        let mut body = vec![0; 72];
        body[..16].copy_from_slice(&[7; 16]);
        body[16..24].copy_from_slice(&71_u64.to_le_bytes());
        body[24..32].copy_from_slice(&2_u64.to_le_bytes());
        body[32..36].copy_from_slice(&3000233124_u32.to_le_bytes());
        body[40..44].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        body[56..58].copy_from_slice(&18789_u16.to_le_bytes());
        body[58..60].copy_from_slice(&35336_u16.to_le_bytes());
        body[61] = 24;
        body[64..72].copy_from_slice(&31_u64.to_le_bytes());
        body.extend_from_slice(payload);
        DriverFrame {
            class: FrameClass::Event,
            message_type: 0x10a,
            session,
            request_id: 0,
            total_bytes: body.len() as u64,
            offset: 0,
            body,
        }
    }

    fn group_fragments(mut original: DriverFrame) -> (DriverFrame, DriverFrame) {
        let body = original.body.split_off(wire::MAX_FRAME - HEADER_BYTES);
        let tail = DriverFrame {
            class: original.class,
            message_type: original.message_type,
            session: original.session,
            request_id: original.request_id,
            total_bytes: original.total_bytes,
            offset: original.body.len() as u64,
            body,
        };
        (original, tail)
    }

    #[tokio::test]
    async fn large_group_packets_complete_once_while_priority_replies_keep_their_original_waiter() {
        for length in [18851, 19590] {
            let (client, peer) = UnixDatagram::pair().expect("physical frame fixture");
            let fd: OwnedFd = client.into();
            let shared = Shared::new(Async::new(File::from(fd)).expect("source"), [1; 16]);
            let info = wire::tests::info();
            shared.info.set(info.clone()).expect("session");
            let (events, mut group) = mpsc::channel(1);
            let (sent, received) = oneshot::channel();
            {
                let mut state = shared.ipc.lock().expect("state");
                state.group = Some(GroupSink {
                    events,
                    full: 0,
                    closed: 0,
                    first_rejection: None,
                });
                state.replies.insert(33, (4, sent));
            }
            let reader = tokio::spawn(receive(Arc::downgrade(&shared)));
            let payload: Vec<_> = (0..length).map(|index| index as u8).collect();
            let (first, last) = group_fragments(group_packet(&payload, info.session_id));
            peer.send(&first.encode().expect("first physical frame"))
                .expect("first copied fragment");
            peer.send(
                &DriverFrame {
                    class: FrameClass::Reply,
                    message_type: 4,
                    session: info.session_id,
                    request_id: 33,
                    total_bytes: 0,
                    offset: 0,
                    body: vec![],
                }
                .encode()
                .expect("priority reply"),
            )
            .expect("reply between fragments");
            let reply = tokio::time::timeout(std::time::Duration::from_secs(2), received)
                .await
                .expect("reply stays live")
                .expect("original waiter")
                .expect("actual reply");
            assert_eq!(reply.request_id, 33);
            assert!(matches!(
                group.try_recv(),
                Err(mpsc::error::TryRecvError::Empty)
            ));
            peer.send(&last.encode().expect("last physical frame"))
                .expect("remaining copied bytes");
            let actual = tokio::time::timeout(std::time::Duration::from_secs(2), group.recv())
                .await
                .expect("complete logical body")
                .expect("one Group observation");
            assert_eq!(
                (actual.ordinal, actual.generation, actual.socket_cookie),
                (71, 2, 31)
            );
            let NativeGroupObservation::Packet {
                payload: actual, ..
            } = actual.observation
            else {
                panic!("original packet");
            };
            assert_eq!(actual, payload);
            assert!(matches!(
                group.try_recv(),
                Err(mpsc::error::TryRecvError::Empty)
            ));
            shared.fail();
            reader
                .await
                .expect("joined reader")
                .expect("no fragment left on stop");
        }
    }

    #[test]
    fn group_continuations_reject_cross_session_total_offset_and_other_events() {
        let original = group_packet(&vec![9; 18851], [5; 16]);
        let (first, last) = group_fragments(original);
        let mut wrong_session = last.clone();
        wrong_session.session = [6; 16];
        let mut wrong_total = last.clone();
        wrong_total.total_bytes += 1;
        let mut overlap = last.clone();
        overlap.offset -= 1;
        let mut other_event = last.clone();
        other_event.message_type = 0x104;
        for conflict in [wrong_session, wrong_total, overlap, other_event] {
            let mut pending = None;
            assert!(
                group_frame(&mut pending, first.clone())
                    .expect("first fragment")
                    .is_none()
            );
            assert!(group_frame(&mut pending, conflict).is_err());
        }
        assert!(
            group_frame(&mut None, last.clone()).is_err(),
            "a continuation cannot begin a new message"
        );
        let mut pending = None;
        group_frame(&mut pending, first).expect("first fragment");
        let completed = DriverFrame {
            class: FrameClass::Event,
            message_type: 0x101,
            session: [5; 16],
            request_id: 0,
            total_bytes: 64,
            offset: 0,
            body: vec![0; 64],
        };
        assert_eq!(
            group_frame(&mut pending, completed.clone())
                .expect("priority result between fragments"),
            Some(completed)
        );
        assert!(
            group_frame(&mut pending, last)
                .expect("original tail after completion")
                .is_some()
        );

        let whole = group_packet(b"original", [5; 16]);
        let pointer = whole.body.as_ptr();
        let complete = group_frame(&mut None, whole)
            .expect("same entry for whole packets")
            .expect("complete");
        let decoded = wire::group_tcp(complete.body).expect("original header and moved payload");
        let NativeGroupObservation::Packet { payload, .. } = decoded.observation else {
            panic!("packet");
        };
        assert_eq!(payload, b"original");
        assert_eq!(
            payload.as_ptr(),
            pointer,
            "decoding retains the original allocation"
        );
    }

    #[tokio::test]
    async fn shutdown_reports_an_incomplete_group_body_after_real_reader_cleanup() {
        let (client, peer) = UnixDatagram::pair().expect("physical frame fixture");
        let fd: OwnedFd = client.into();
        let shared = Shared::new(Async::new(File::from(fd)).expect("source"), [1; 16]);
        let info = wire::tests::info();
        shared.info.set(info.clone()).expect("session");
        let (events, mut group) = mpsc::channel(1);
        let (sent, received) = oneshot::channel();
        {
            let mut state = shared.ipc.lock().expect("state");
            state.group = Some(GroupSink {
                events,
                full: 0,
                closed: 0,
                first_rejection: None,
            });
            state.replies.insert(33, (4, sent));
        }
        let reader = tokio::spawn(receive(Arc::downgrade(&shared)));
        let (first, _) = group_fragments(group_packet(&vec![9; 19590], info.session_id));
        peer.send(&first.encode().expect("first fragment"))
            .expect("partial source body");
        peer.send(
            &DriverFrame {
                class: FrameClass::Reply,
                message_type: 4,
                session: info.session_id,
                request_id: 33,
                total_bytes: 0,
                offset: 0,
                body: vec![],
            }
            .encode()
            .expect("processing barrier reply"),
        )
        .expect("barrier");
        received
            .await
            .expect("first fragment already processed")
            .expect("reply");
        let session = Arc::new(NativeSession {
            shared: shared.clone(),
            info: info.clone(),
            reader: Mutex::new(Some(reader)),
            loader: Mutex::new(None),
            close: Mutex::new(Close::Open),
            quiesce: Mutex::new(None),
        });
        let peer = Async::new(peer).expect("peer");
        let responder = tokio::spawn(async move {
            for kind in [5, 4] {
                let mut bytes = vec![0; wire::MAX_FRAME];
                let count = peer
                    .read_with(|peer| peer.recv(&mut bytes))
                    .await
                    .expect("stop request");
                let request =
                    DriverFrame::decode(&bytes[..count], wire::MAX_FRAME, wire::MAX_TRANSFER)
                        .expect("request frame");
                assert_eq!(request.message_type, kind);
                let mut body = vec![0; 8];
                let mut status = wire::tests::status_body();
                status[16..20].copy_from_slice(&2_u32.to_le_bytes());
                body.extend(status);
                let reply = DriverFrame {
                    class: FrameClass::Reply,
                    message_type: kind,
                    session: info.session_id,
                    request_id: request.request_id,
                    total_bytes: body.len() as u64,
                    offset: 0,
                    body,
                }
                .encode()
                .expect("declared quiescence");
                peer.write_with(|peer| peer.send(&reply))
                    .await
                    .expect("reply");
            }
        });
        assert!(matches!(
            session.shutdown().await,
            Err(ModuleError::InvalidInput)
        ));
        responder.await.expect("reply source stopped");
        assert!(shared.io.lock().expect("I/O retired").is_none());
        assert!(shared.ipc.lock().expect("source retired").group.is_none());
        assert!(
            group.recv().await.is_none(),
            "no partial observation is manufactured"
        );
    }

    #[tokio::test]
    async fn group_rejection_counts_a_tail_drop_and_a_closed_consumer() {
        let (events, mut receiver) = mpsc::channel(1);
        let mut sink = GroupSink {
            events,
            full: 0,
            closed: 0,
            first_rejection: None,
        };
        let fact = |ordinal| NativeGroupTcp {
            source: [1; 16],
            ordinal,
            generation: 1,
            source_port: 40000,
            destination_port: 19090,
            sequence: 10,
            acknowledgment: 0,
            socket_cookie: 1,
            observation: NativeGroupObservation::FirstSend,
        };
        assert_eq!(sink.offer(fact(1)), None);
        assert_eq!(sink.offer(fact(2)), Some("full"));
        assert_eq!(receiver.recv().await.expect("original frame").ordinal, 1);
        assert_eq!(sink.full, 1);
        drop(receiver);
        assert_eq!(sink.offer(fact(3)), None);
        assert_eq!((sink.full, sink.closed), (1, 1));
        let diagnostic = sink.retire().expect("original loss report");
        assert!(diagnostic.contains("AS_GROUP_ADAPTER_REJECTED reason=full"));
        assert!(diagnostic.contains("AS_GROUP_ADAPTER_LOSS full=1 closed=1"));
    }

    fn required_event_frame(direct: bool, session: [u8; 16]) -> DriverFrame {
        let mut event = vec![0; 88];
        event[..2].copy_from_slice(&1u16.to_le_bytes());
        event[2..4].copy_from_slice(&88u16.to_le_bytes());
        event[4..8].copy_from_slice(&88u32.to_le_bytes());
        event[8..10].copy_from_slice(&2u16.to_le_bytes());
        event[10..12].copy_from_slice(&(if direct { 3u16 } else { 2 }).to_le_bytes());
        event[12..16].copy_from_slice(&1u32.to_le_bytes());
        event[16..32].copy_from_slice(&[7; 16]);
        event[32..40].copy_from_slice(&1u64.to_le_bytes());
        event[40..48].copy_from_slice(&1u64.to_le_bytes());
        let mut hit = Vec::new();
        if direct {
            let group = b"fixture";
            let rule = b"driver-deny";
            for value in [3u32, 1, 0, 2] {
                hit.extend(value.to_le_bytes());
            }
            hit.extend((group.len() as u16).to_le_bytes());
            hit.extend((rule.len() as u16).to_le_bytes());
            hit.extend(group);
            hit.extend(rule);
        }
        let mut body = vec![0; 128];
        if !direct {
            let clock = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
            let now = clock.tv_sec as u64 * 1_000_000_000 + clock.tv_nsec as u64;
            body[..8].copy_from_slice(&7u64.to_le_bytes());
            body[8..16].copy_from_slice(&now.to_le_bytes());
            body[16..24].copy_from_slice(&(now + 5_000_000_000).to_le_bytes());
        }
        body[24..32].copy_from_slice(&1u64.to_le_bytes());
        body[40..72].copy_from_slice(&[8; 32]);
        body[72..104].copy_from_slice(&[9; 32]);
        body[104..108].copy_from_slice(&(event.len() as u32).to_le_bytes());
        body[108..112].copy_from_slice(&(hit.len() as u32).to_le_bytes());
        body.extend(hit);
        body.extend(event);
        if direct {
            let mut result = vec![0; 64];
            result[8..16].copy_from_slice(&1u64.to_le_bytes());
            result[16..20].copy_from_slice(&(-1i32).to_le_bytes());
            result[24..28].copy_from_slice(&(-61i32).to_le_bytes());
            result[28..32].copy_from_slice(&(-61i32).to_le_bytes());
            result[32..64].copy_from_slice(&[9; 32]);
            body.extend(result);
        }
        DriverFrame {
            class: FrameClass::Event,
            message_type: if direct { 0x102 } else { 0x100 },
            session,
            request_id: 0,
            total_bytes: body.len() as u64,
            offset: 0,
            body,
        }
    }
    #[tokio::test]
    async fn auxiliary_facts_preserve_originals_fifo_and_zero_loss_under_pressure() {
        fn original(event: &DriverEvent) -> serde_json::Value {
            match event {
                DriverEvent::RequestReturn(value) => serde_json::to_value(value),
                DriverEvent::FutexSelection(value) => serde_json::to_value(value),
                DriverEvent::Status(value) => serde_json::to_value(value),
                DriverEvent::Observation(value) => serde_json::to_value(value),
                _ => panic!("fixture event"),
            }
            .expect("original fields")
        }
        for boundary in ["full", "critical"] {
            let event = as_types::process_tree::EventKey {
                source_instance: [3; 16],
                sequence_domain: 2,
                sequence: 11,
            };
            let mut facts = vec![
                DriverEvent::RequestReturn(NativeRequestReturn {
                    event,
                    operation: NativeRequestOperation::FileOpen,
                    decision_id: as_types::DecisionId([4; 16]),
                    wait_id: 9,
                    returned_boottime_ns: 17,
                    return_value: -1,
                }),
                DriverEvent::FutexSelection(NativeFutexSelection {
                    event,
                    waker: NativeSyncThread {
                        tgid: 2,
                        tid: 2,
                        birth_boottime_ns: 3,
                    },
                    waiter: NativeSyncThread {
                        tgid: 2,
                        tid: 3,
                        birth_boottime_ns: 4,
                    },
                    selected_boottime_ns: 17,
                    wait: None,
                }),
                DriverEvent::Status(wire::tests::info().status),
            ];
            for process in [false, true] {
                let mut frame = required_event_frame(false, [1; 16]);
                let bytes = &mut frame.body[128..];
                bytes[10..12].copy_from_slice(&1u16.to_le_bytes());
                if process {
                    bytes[8..10].copy_from_slice(&1u16.to_le_bytes());
                    bytes[12..16].copy_from_slice(
                        &(as_types::event_operation::ProcessOperation::Exit as u32).to_le_bytes(),
                    );
                }
                facts.push(DriverEvent::Observation(
                    as_types::dispatch_event::decode(bytes).expect("original frame fixture"),
                ));
            }
            for fact in facts {
                let expected = original(&fact);
                let (client, _peer) = UnixDatagram::pair().expect("original queue fixture");
                let fd: OwnedFd = client.into();
                let shared = Shared::new(Async::new(File::from(fd)).expect("source"), [1; 16]);
                let (events, mut receiving) = mpsc::channel(1);
                let (controlled, _controlled_receiver) = mpsc::channel(1);
                let loss = shared.rejected.subscribe();
                if boundary == "full" {
                    events
                        .try_send(DriverEvent::Status(wire::tests::info().status))
                        .expect("existing FIFO head");
                }
                {
                    let mut state = shared.ipc.lock().expect("state");
                    state.events = Some(events);
                    state.controlled = Some(controlled);
                    if boundary == "critical" {
                        state.generations.insert(
                            1,
                            Arc::new(Semaphore::new(1))
                                .try_acquire_owned()
                                .expect("generation"),
                        );
                        state.critical.push_back(Critical::Retired(1));
                    }
                }
                shared.deliver(fact).expect("retained original");
                assert!(!loss.has_changed().expect("source loss watch"));
                assert_eq!(*loss.borrow(), 0);
                // Stop drains the already-read original after the earlier FIFO entry.
                shared.ipc.lock().expect("final retirement").available = false;
                shared.stop.send_replace(true);
                let reader = tokio::spawn(receive(Arc::downgrade(&shared)));
                let first = receiving.recv().await.expect("prior FIFO item");
                assert!(matches!(
                    (boundary, first),
                    ("full", DriverEvent::Status(_))
                        | ("critical", DriverEvent::PersistentRetired { generation: 1 })
                ));
                let actual = receiving.recv().await.expect("same original delivered");
                assert!(
                    !matches!(actual, DriverEvent::TransportUnavailable),
                    "normal drain returned TransportUnavailable instead of its retained original"
                );
                assert_eq!(original(&actual), expected);
                reader
                    .await
                    .expect("joined original pump")
                    .expect("normal drain");
                let state = shared.ipc.lock().expect("state");
                assert!(!state.available && state.critical.is_empty());
                assert!(state.generations.is_empty());
                assert!(matches!(
                    receiving.try_recv(),
                    Err(mpsc::error::TryRecvError::Empty)
                ));
                assert_eq!(*loss.borrow(), 0);
            }
        }
    }
    #[tokio::test]
    async fn mandatory_output_waits_for_capacity_and_preserves_real_closed_failures() {
        for kind in ["direct", "handoff", "file_close"] {
            for boundary in ["full", "closed", "critical", "unbound"] {
                let (client, _peer) = UnixDatagram::pair().expect("native-shaped fixture");
                let fd: OwnedFd = client.into();
                let shared = Shared::new(Async::new(File::from(fd)).expect("source"), [1; 16]);
                let info = wire::tests::info();
                assert!(shared.info.set(info.clone()).is_ok());
                let (events, mut receiving) = mpsc::channel(1);
                if boundary == "full" {
                    events
                        .try_send(DriverEvent::Status(info.status.clone()))
                        .expect("full");
                }
                if boundary == "closed" {
                    receiving.close();
                }
                {
                    let mut state = shared.ipc.lock().expect("state");
                    if boundary != "unbound" {
                        state.events = Some(events);
                    }
                    if boundary == "critical" {
                        let permit = Arc::new(Semaphore::new(1))
                            .try_acquire_owned()
                            .expect("generation");
                        state.generations.insert(1, permit);
                        state.critical.push_back(Critical::Retired(1));
                    }
                }
                let result = if kind == "file_close" {
                    shared.deliver(DriverEvent::FileClose(
                        as_types::security_log::FileCloseAuditRecord::FileWriteClose {
                            reporter_tid: 2,
                            path: Some(b"/audit/\xff".to_vec()),
                            path_error: None,
                            device_major: 0,
                            device_minor: 0,
                            inode: 1,
                        },
                    ))
                } else if kind == "handoff" {
                    shared.deliver(DriverEvent::RequestHandoff(
                        as_types::runtime_observation::NativeRequestHandoff {
                            observation: as_types::process_tree::EventKey {
                                source_instance: [2; 16],
                                sequence_domain: 2,
                                sequence: 9,
                            },
                            request: None,
                            operation: None,
                            actor: as_types::driver::NativeSyncThread {
                                tgid: 2,
                                tid: 3,
                                birth_boottime_ns: 4,
                            },
                            observed_boottime_ns: 5,
                            phase: as_types::runtime_observation::NativeRequestPhase::ApiReturned {
                                result: 0,
                            },
                        },
                    ))
                } else {
                    shared
                        .event(required_event_frame(kind == "direct", info.session_id))
                        .await
                };
                if matches!(boundary, "closed" | "unbound") {
                    assert!(
                        matches!(result, Err(ModuleError::Unavailable)),
                        "{boundary}"
                    );
                    assert_eq!(*shared.rejected.borrow(), 1);
                    assert!(matches!(
                        shared.deliver(DriverEvent::Status(info.status.clone())),
                        Err(ModuleError::Unavailable)
                    ));
                    assert_eq!(*shared.rejected.borrow(), 2);
                } else {
                    result.expect("temporary pressure retains the original");
                    {
                        let state = shared.ipc.lock().expect("retained source");
                        assert!(state.available);
                        assert_eq!(
                            state
                                .critical
                                .iter()
                                .filter(|entry| matches!(entry, Critical::Required(_)))
                                .count(),
                            1
                        );
                    }
                    assert_eq!(*shared.rejected.borrow(), 0, "pressure is not loss");
                    let reader = tokio::spawn(receive(Arc::downgrade(&shared)));
                    let first = receiving.recv().await.expect("existing FIFO head");
                    if boundary == "full" {
                        assert!(matches!(first, DriverEvent::Status(_)));
                    } else {
                        assert!(matches!(
                            first,
                            DriverEvent::PersistentRetired { generation: 1 }
                        ));
                    }
                    let event = receiving.recv().await.expect("original delivered");
                    assert!(matches!(
                        (&event, kind),
                        (DriverEvent::DirectResult { .. }, "direct")
                            | (DriverEvent::RequestHandoff(_), "handoff")
                            | (DriverEvent::FileClose(_), "file_close")
                    ));
                    assert!(shared.ipc.lock().expect("source").available);
                    shared.stop.send_replace(true);
                    reader
                        .await
                        .expect("reader joined")
                        .expect("normal tail completed");
                }
                assert_eq!(
                    shared.work.len(),
                    0,
                    "rejection must not start a detached cancel command"
                );
                shared.fail();
                assert!(!shared.ipc.lock().expect("failed").available);
                assert!(
                    shared
                        .ipc
                        .lock()
                        .expect("released originals")
                        .originals
                        .is_empty()
                );
            }
        }
        let (client, _peer) = UnixDatagram::pair().expect("closed-after-pressure fixture");
        let fd: OwnedFd = client.into();
        let shared = Shared::new(Async::new(File::from(fd)).expect("source"), [1; 16]);
        let info = wire::tests::info();
        shared.info.set(info.clone()).expect("session");
        let (events, mut receiving) = mpsc::channel(1);
        events
            .try_send(DriverEvent::Status(info.status.clone()))
            .expect("full output");
        shared.ipc.lock().expect("state").events = Some(events);
        shared
            .event(required_event_frame(true, info.session_id))
            .await
            .expect("retained Direct");
        receiving.close();
        let result = receive(Arc::downgrade(&shared)).await;
        assert!(
            matches!(result, Err(ModuleError::Stopped)),
            "actual consumer close is still failure"
        );
        let state = shared.ipc.lock().expect("failed source");
        assert!(!state.available);
        assert!(
            state.critical.is_empty(),
            "failed in-flight ownership is released"
        );
    }
    #[tokio::test]
    async fn priority_controls_and_completions_continue_while_required_auxiliary_output_is_full() {
        let (client, mut peer) = UnixStream::pair().expect("PRI fixture");
        let fd: OwnedFd = client.into();
        let shared = Shared::new(Async::new(File::from(fd)).expect("source"), [1; 16]);
        let info = wire::tests::info();
        assert!(shared.info.set(info.clone()).is_ok());
        let (events, mut receiving) = mpsc::channel(1);
        events
            .try_send(DriverEvent::Status(info.status.clone()))
            .expect("full callback queue");
        shared.ipc.lock().expect("state").events = Some(events);
        let (controls, mut controlled) = mpsc::channel(1);
        shared.ipc.lock().expect("state").controlled = Some(controls);
        let held = required_event_frame(false, info.session_id);
        let (request, _) = wire::controlled(&held.body, false).expect("original");
        let original = Original::new(&shared, request, info.session_id).expect("retained original");
        shared
            .ipc
            .lock()
            .expect("state")
            .originals
            .insert(original.id, original.clone());
        let reader = tokio::spawn(receive(Arc::downgrade(&shared)));
        let body = wire::tests::status_body();
        let expected_status = wire::status(&body).expect("original status");
        let frame = DriverFrame {
            class: FrameClass::Event,
            message_type: 0x103,
            session: info.session_id,
            request_id: 0,
            total_bytes: body.len() as u64,
            offset: 0,
            body,
        }
        .encode()
        .expect("actual encoded auxiliary status");
        peer.write_all(&frame).expect("native-shaped source write");
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if shared
                    .ipc
                    .lock()
                    .expect("source")
                    .critical
                    .iter()
                    .any(|entry| matches!(entry, Critical::Required(_)))
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("auxiliary original retained without failing source");
        assert!(!reader.is_finished());
        let mut body = vec![0; 64];
        body[..8].copy_from_slice(&7u64.to_le_bytes());
        body[8..16].copy_from_slice(&1u64.to_le_bytes());
        body[16..20].copy_from_slice(&(-5i32).to_le_bytes());
        body[24..28].copy_from_slice(&(-61i32).to_le_bytes());
        body[28..32].copy_from_slice(&(-61i32).to_le_bytes());
        body[32..64].copy_from_slice(&[9; 32]);
        let result = DriverFrame {
            class: FrameClass::Event,
            message_type: 0x101,
            session: info.session_id,
            request_id: 0,
            total_bytes: 64,
            offset: 0,
            body,
        }
        .encode()
        .expect("actual result");
        // Unix stream OOB provides real kernel PRI; the ordinary read skips the
        // marker and consumes exactly the following native-shaped completion.
        rustix::net::send(&peer, b"!", rustix::net::SendFlags::OOB).expect("PRI result readiness");
        peer.write_all(&result).expect("result bytes");
        let receipt = tokio::time::timeout(std::time::Duration::from_secs(2), original.result())
            .await
            .expect("completion must bypass upper pressure")
            .expect("actual original result");
        assert_eq!(
            receipt.outcome,
            as_types::operation::ExecutionOutcome::Failed { native_error: -5 }
        );
        let (sent, received) = oneshot::channel();
        shared
            .ipc
            .lock()
            .expect("source")
            .replies
            .insert(33, (4, sent));
        let reply = DriverFrame {
            class: FrameClass::Reply,
            message_type: 4,
            session: info.session_id,
            request_id: 33,
            total_bytes: 0,
            offset: 0,
            body: vec![],
        }
        .encode()
        .expect("reply");
        rustix::net::send(&peer, b"!", rustix::net::SendFlags::OOB).expect("PRI reply readiness");
        peer.write_all(&reply).expect("reply bytes");
        tokio::time::timeout(std::time::Duration::from_secs(2), received)
            .await
            .expect("reply continues")
            .expect("waiter")
            .expect("actual reply");
        let mut request = required_event_frame(false, info.session_id);
        request.body[..8].copy_from_slice(&8u64.to_le_bytes());
        let request = request.encode().expect("new controlled original");
        rustix::net::send(&peer, b"!", rustix::net::SendFlags::OOB)
            .expect("PRI original readiness");
        peer.write_all(&request).expect("controlled bytes");
        let input = controlled
            .recv()
            .await
            .expect("control bypasses auxiliary pressure");
        assert!(input.operation.is_some());
        assert!(
            shared
                .ipc
                .lock()
                .expect("retained auxiliary original")
                .critical
                .iter()
                .any(|entry| matches!(entry, Critical::Required(_)))
        );
        let ordinary = DriverFrame {
            class: FrameClass::Event,
            message_type: 0x103,
            session: info.session_id,
            request_id: 0,
            total_bytes: wire::tests::status_body().len() as u64,
            offset: 0,
            body: wire::tests::status_body(),
        }
        .encode()
        .expect("ordinary status");
        let mut loss = shared.rejected.subscribe();
        peer.write_all(&ordinary)
            .expect("ordinary readiness without PRI");
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), loss.changed())
                .await
                .is_err(),
            "ordinary POLLIN cannot consume/drop another event while one required result is retained"
        );
        assert!(shared.ipc.lock().expect("source").available);
        // A real stop request still drains the already-read required tail.
        shared.stop.send_replace(true);
        assert!(matches!(
            receiving.recv().await,
            Some(DriverEvent::Status(_))
        ));
        assert!(matches!(
            receiving.recv().await,
            Some(DriverEvent::Status(value)) if value == expected_status
        ));
        reader
            .await
            .expect("reader join")
            .expect("normal stop drains required tail");
        assert!(shared.ipc.lock().expect("source").critical.is_empty());
        assert_eq!(*shared.rejected.borrow(), 0);
    }
    #[tokio::test]
    async fn facts_barrier_waits_for_consumer_without_blocking_native_replies() {
        let (client, peer) = UnixDatagram::pair().expect("packet fixture");
        let fd: OwnedFd = client.into();
        let shared = Shared::new(Async::new(File::from(fd)).expect("source"), [1; 16]);
        let info = wire::tests::info();
        assert!(shared.info.set(info.clone()).is_ok());
        let (events, mut receiving) = mpsc::channel(1);
        events
            .try_send(DriverEvent::TransportUnavailable)
            .expect("full callback queue");
        shared.ipc.lock().expect("state").events = Some(events);
        let reader = tokio::spawn(receive(Arc::downgrade(&shared)));
        let session = Arc::new(NativeSession {
            shared: shared.clone(),
            info: info.clone(),
            reader: Mutex::new(None),
            loader: Mutex::new(None),
            close: Mutex::new(Close::Open),
            quiesce: Mutex::new(None),
        });
        let transport = Async::new(peer).expect("peer");
        let server = tokio::spawn(async move {
            let mut bytes = vec![0; wire::MAX_FRAME];
            for kind in [15, 4] {
                let n = transport
                    .read_with(|p| p.recv(&mut bytes))
                    .await
                    .expect("request");
                let request = DriverFrame::decode(&bytes[..n], wire::MAX_FRAME, wire::MAX_TRANSFER)
                    .expect("request frame");
                assert_eq!(request.message_type, kind);
                let mut body = vec![0; 8];
                if kind == 15 {
                    let marker = |loss: u64| {
                        [1u64, 2, loss, 4026531836, 100, loss - 6]
                            .into_iter()
                            .flat_map(u64::to_le_bytes)
                            .collect::<Vec<_>>()
                    };
                    let value = marker(7);
                    let event = DriverFrame {
                        class: FrameClass::Event,
                        message_type: 0x106,
                        session: info.session_id,
                        request_id: 0,
                        total_bytes: 48,
                        offset: 0,
                        body: value,
                    }
                    .encode()
                    .expect("marker");
                    transport
                        .write_with(|p| p.send(&event))
                        .await
                        .expect("marker before reply");
                    body.extend(marker(6));
                } else {
                    body.extend(wire::tests::status_body());
                }
                let response = DriverFrame {
                    class: FrameClass::Reply,
                    message_type: kind,
                    session: info.session_id,
                    request_id: request.request_id,
                    total_bytes: body.len() as u64,
                    offset: 0,
                    body,
                }
                .encode()
                .expect("reply");
                transport
                    .write_with(|p| p.send(&response))
                    .await
                    .expect("reply while callbacks full");
            }
        });
        let waiting = session.clone();
        let barrier = tokio::spawn(async move { waiting.facts_barrier().await });
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while shared.ipc.lock().expect("state").facts_markers.is_empty() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("native marker arrived without consumer readiness");
        let status = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            shared.command(4, Vec::new()),
        )
        .await
        .expect("reply pump live")
        .expect("status reply");
        assert_eq!(
            wire::status(&status).expect("status").persistent_generation,
            1
        );
        assert!(!barrier.is_finished());
        assert!(matches!(
            receiving.recv().await,
            Some(DriverEvent::TransportUnavailable)
        ));
        match receiving.recv().await.expect("reliable fact marker") {
            DriverEvent::FactsBarrier { marker, processed } => {
                assert_eq!(marker.loss_epoch, 7);
                assert!(!barrier.is_finished());
                processed.send(()).expect("consumer ACK");
            }
            _ => panic!("wrong critical event"),
        }
        let actual = barrier
            .await
            .expect("owned fact request")
            .expect("consumed marker");
        assert_eq!(actual.loss_epoch, 7);
        assert_eq!(actual.native_hit_loss, 1);
        assert_eq!(actual.backend_rejected, 0);
        shared.ipc.lock().expect("retirement state").retiring = true;
        shared.admission_stop.send_replace(true);
        let body = [1u64, 3, 7, 4026531836, 100, 0]
            .into_iter()
            .flat_map(u64::to_le_bytes)
            .collect();
        shared
            .event(DriverFrame {
                class: FrameClass::Event,
                message_type: 0x106,
                session: shared.info.get().expect("info").session_id,
                request_id: 0,
                total_bytes: 48,
                offset: 0,
                body,
            })
            .await
            .expect("late fact marker after waiter cancellation is not source failure");
        assert!(shared.ipc.lock().expect("state").available);
        assert!(shared.ipc.lock().expect("state").critical.is_empty());
        server.await.expect("peer finished");
        shared.fail();
        reader
            .await
            .expect("reader joined")
            .expect("reader stopped");
    }
    #[tokio::test]
    async fn full_event_queue_does_not_block_a_native_reply_or_lose_retirement() {
        let (client, server) = UnixDatagram::pair().expect("packet transport fixture");
        let fd: OwnedFd = client.into();
        let file = File::from(fd);
        let shared = Shared::new(Async::new(file).expect("safe native-shaped fd"), [1; 16]);
        let info = wire::tests::info();
        assert!(shared.info.set(info.clone()).is_ok());
        let (events, mut receiving) = mpsc::channel(1);
        events
            .try_send(DriverEvent::TransportUnavailable)
            .expect("fill fixture queue");
        let budget = Arc::new(Semaphore::new(2));
        let permit = budget
            .clone()
            .try_acquire_owned()
            .expect("active generation permit");
        assert!(shared.generation_budget.set(budget.clone()).is_ok());
        {
            let mut ipc = shared.ipc.lock().expect("state");
            ipc.events = Some(events);
            ipc.generations.insert(1, permit);
        }
        let loss = shared.rejected.subscribe();
        let reader = tokio::spawn(receive(Arc::downgrade(&shared)));
        let server = Async::new(server).expect("async fixture peer");
        let peer = tokio::spawn(async move {
            let mut bytes = vec![0; wire::MAX_FRAME];
            let count = server
                .read_with(|s| s.recv(&mut bytes))
                .await
                .expect("request");
            let request = DriverFrame::decode(&bytes[..count], wire::MAX_FRAME, wire::MAX_TRANSFER)
                .expect("frame");
            let retired = DriverFrame {
                class: FrameClass::Event,
                message_type: 0x105,
                session: info.session_id,
                request_id: 0,
                total_bytes: 8,
                offset: 0,
                body: 1u64.to_le_bytes().to_vec(),
            }
            .encode()
            .expect("retirement");
            server
                .write_with(|s| s.send(&retired))
                .await
                .expect("retire while upper queue is full");
            let mut body = vec![0; 8];
            body.extend(wire::tests::status_body());
            let reply = DriverFrame {
                class: FrameClass::Reply,
                message_type: request.message_type,
                session: info.session_id,
                request_id: request.request_id,
                total_bytes: body.len() as u64,
                offset: 0,
                body,
            }
            .encode()
            .expect("reply");
            server.write_with(|s| s.send(&reply)).await.expect("answer");
        });
        let raw = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            shared.command(4, Vec::new()),
        )
        .await
        .expect("pump must continue reading")
        .expect("actual reply");
        assert_eq!(wire::status(&raw).expect("status").persistent_generation, 1);
        assert!(matches!(
            receiving.recv().await,
            Some(DriverEvent::TransportUnavailable)
        ));
        assert!(matches!(
            tokio::time::timeout(std::time::Duration::from_secs(2), receiving.recv())
                .await
                .expect("reliable retirement"),
            Some(DriverEvent::PersistentRetired { generation: 1 })
        ));
        assert_eq!(budget.available_permits(), 2);
        assert!(!loss.has_changed().expect("source loss watch"));
        assert_eq!(*loss.borrow(), 0);
        peer.await.expect("peer");
        shared.fail();
        reader
            .await
            .expect("receive task stops")
            .expect("reader stopped");
    }
    #[tokio::test]
    async fn dropped_apply_waiter_retains_the_original_until_its_native_result() {
        use as_types::{
            dispatch_event,
            main_ports::RuleChoice,
            operation::{ExecutionOutcome, OriginalOperation},
        };
        let (client, server) = UnixDatagram::pair().expect("packet fixture");
        let fd: OwnedFd = client.into();
        let shared = Shared::new(Async::new(File::from(fd)).expect("async endpoint"), [1; 16]);
        let info = wire::tests::info();
        assert!(shared.info.set(info.clone()).is_ok());
        let mut bytes = vec![0u8; 88];
        bytes[..2].copy_from_slice(&1u16.to_le_bytes());
        bytes[2..4].copy_from_slice(&88u16.to_le_bytes());
        bytes[4..8].copy_from_slice(&88u32.to_le_bytes());
        bytes[8..10].copy_from_slice(&1u16.to_le_bytes());
        bytes[10..12].copy_from_slice(&2u16.to_le_bytes());
        bytes[12..16].copy_from_slice(&1u32.to_le_bytes());
        bytes[16..32].copy_from_slice(&[7; 16]);
        bytes[32..40].copy_from_slice(&1u64.to_le_bytes());
        bytes[40..48].copy_from_slice(&1u64.to_le_bytes());
        let event = dispatch_event::decode(&bytes).expect("facts");
        let clock = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
        let now = (clock.tv_sec as u64) * 1_000_000_000 + clock.tv_nsec as u64;
        let request = wire::Controlled {
            id: 7,
            started: now,
            deadline: now + 5_000_000_000,
            generation: 1,
            rules: as_types::RuleVersion::from_digest([9; 32]),
            event: event.clone(),
            hit: None,
        };
        let original = Original::new(&shared, request, info.session_id).expect("retained original");
        shared
            .ipc
            .lock()
            .expect("state")
            .originals
            .insert(7, original.clone());
        let reader = tokio::spawn(receive(Arc::downgrade(&shared)));
        let peer = Async::new(server).expect("async peer");
        let (replied, acknowledged) = oneshot::channel();
        let release = Arc::new(tokio::sync::Notify::new());
        let wait = release.clone();
        let server = tokio::spawn(async move {
            let mut bytes = vec![0; wire::MAX_FRAME];
            let count = peer
                .read_with(|s| s.recv(&mut bytes))
                .await
                .expect("decision");
            let request = DriverFrame::decode(&bytes[..count], wire::MAX_FRAME, wire::MAX_TRANSFER)
                .expect("frame");
            assert_eq!(request.message_type, 8);
            let frame = DriverFrame {
                class: FrameClass::Reply,
                message_type: 8,
                session: info.session_id,
                request_id: request.request_id,
                total_bytes: 8,
                offset: 0,
                body: vec![0; 8],
            }
            .encode()
            .expect("admission reply");
            peer.write_with(|s| s.send(&frame)).await.expect("admitted");
            let _ = replied.send(());
            wait.notified().await;
            let mut result = vec![0; 64];
            result[..8].copy_from_slice(&7u64.to_le_bytes());
            result[8..16].copy_from_slice(&1u64.to_le_bytes());
            result[16..20].copy_from_slice(&(-1i32).to_le_bytes());
            result[24..28].copy_from_slice(&(-61i32).to_le_bytes());
            result[28..32].copy_from_slice(&(-61i32).to_le_bytes());
            result[32..64].copy_from_slice(&[9; 32]);
            let frame = DriverFrame {
                class: FrameClass::Event,
                message_type: 0x101,
                session: info.session_id,
                request_id: 0,
                total_bytes: 64,
                offset: 0,
                body: result,
            }
            .encode()
            .expect("physical result");
            peer.write_with(|s| s.send(&frame))
                .await
                .expect("completed");
        });
        let executing = original.clone();
        let work = tokio::spawn(async move {
            executing
                .apply(
                    &event,
                    &RuleChoice {
                        decision: Decision::Deny {
                            terminate: as_types::main_ports::Termination::None,
                        },
                        hit: None,
                        reason: ChoiceReason::Direct,
                    },
                )
                .await
        });
        acknowledged.await.expect("decision admitted");
        work.abort();
        let _ = work.await;
        assert!(
            original.try_result().is_none(),
            "send admission is not an execution receipt"
        );
        assert!(shared.ipc.lock().expect("state").originals.contains_key(&7));
        release.notify_one();
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), original.result())
            .await
            .expect("same native completion")
            .expect("receipt");
        assert_eq!(result.outcome, ExecutionOutcome::Denied);
        assert!(shared.ipc.lock().expect("state").originals.is_empty());
        server.await.expect("peer");
        shared.fail();
        reader
            .await
            .expect("reader retirement")
            .expect("reader stopped");
    }
    #[tokio::test]
    async fn original_native_errors_do_not_fail_the_source_but_real_eof_does() {
        use as_types::operation::{ExecutionOutcome, OriginalOperation, StopReason};
        let (client, peer) = std::os::unix::net::UnixStream::pair().expect("real EOF fixture");
        let fd: OwnedFd = client.into();
        let shared = Shared::new(Async::new(File::from(fd)).expect("source"), [1; 16]);
        let info = wire::tests::info();
        assert!(shared.info.set(info.clone()).is_ok());
        let mut facts = vec![0u8; 88];
        facts[..2].copy_from_slice(&1u16.to_le_bytes());
        facts[2..4].copy_from_slice(&88u16.to_le_bytes());
        facts[4..8].copy_from_slice(&88u32.to_le_bytes());
        facts[8..10].copy_from_slice(&1u16.to_le_bytes());
        facts[10..12].copy_from_slice(&2u16.to_le_bytes());
        facts[12..16].copy_from_slice(&1u32.to_le_bytes());
        facts[16..32].copy_from_slice(&[7; 16]);
        facts[32..40].copy_from_slice(&1u64.to_le_bytes());
        facts[40..48].copy_from_slice(&1u64.to_le_bytes());
        let event = as_types::dispatch_event::decode(&facts).expect("same original facts");
        let clock = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
        let now = (clock.tv_sec as u64) * 1_000_000_000 + clock.tv_nsec as u64;
        let original = |id| {
            Original::new(
                &shared,
                wire::Controlled {
                    id,
                    started: now,
                    deadline: now + 5_000_000_000,
                    generation: 1,
                    rules: as_types::RuleVersion::from_digest([9; 32]),
                    event: event.clone(),
                    hit: None,
                },
                info.session_id,
            )
            .expect("retained original")
        };
        let pending = original(9);
        shared
            .ipc
            .lock()
            .expect("state")
            .originals
            .insert(9, pending.clone());
        let mut completed = Vec::new();
        for (id, native_error) in [(1u64, -110i32), (2, -5)] {
            let operation = original(id);
            shared
                .ipc
                .lock()
                .expect("state")
                .originals
                .insert(id, operation.clone());
            let mut body = vec![0; 64];
            body[..8].copy_from_slice(&id.to_le_bytes());
            body[8..16].copy_from_slice(&1u64.to_le_bytes());
            body[16..20].copy_from_slice(&native_error.to_le_bytes());
            body[24..28].copy_from_slice(&(-61i32).to_le_bytes());
            body[28..32].copy_from_slice(&(-61i32).to_le_bytes());
            body[32..64].copy_from_slice(&[9; 32]);
            let bytes = DriverFrame {
                class: FrameClass::Event,
                message_type: 0x101,
                session: info.session_id,
                request_id: 0,
                total_bytes: 64,
                offset: 0,
                body,
            }
            .encode()
            .expect("native result frame");
            shared
                .event(
                    DriverFrame::decode(&bytes, wire::MAX_FRAME, wire::MAX_TRANSFER)
                        .expect("same frame"),
                )
                .await
                .expect("valid operation failure is not a protocol failure");
            let receipt = operation.result().await.expect("actual operation receipt");
            assert_eq!(receipt.outcome, ExecutionOutcome::Failed { native_error });
            assert_eq!(receipt.decision_id, operation.decision_id());
            assert_eq!(
                receipt.termination,
                as_types::operation::TerminationReport::NONE
            );
            assert_eq!(
                *operation.stop_signal().expect("retirement").borrow(),
                Some(StopReason::Orderly)
            );
            assert!(shared.ipc.lock().expect("source still live").available);
            assert!(!*shared.stop.borrow());
            assert!(
                pending.try_result().is_none(),
                "one failed operation does not retire another"
            );
            completed.push((operation, receipt));
        }
        let reader = tokio::spawn(receive(Arc::downgrade(&shared)));
        drop(peer);
        tokio::time::timeout(std::time::Duration::from_secs(2), reader)
            .await
            .expect("actual EOF retired the reader")
            .expect("reader join")
            .expect_err("actual EOF is a transport failure");
        assert!(!shared.ipc.lock().expect("failed source").available);
        assert!(matches!(
            pending.result().await,
            Err(ModuleError::Unavailable)
        ));
        assert_eq!(
            *pending.stop_signal().expect("source signal").borrow(),
            Some(StopReason::SourceFailure)
        );
        for (operation, receipt) in completed {
            assert_eq!(
                operation
                    .result()
                    .await
                    .expect("finished result stays authoritative"),
                receipt
            );
        }
    }
    #[tokio::test]
    async fn cancelled_startup_closes_the_event_source_even_when_failure_queue_is_full() {
        let (client, _peer) = UnixDatagram::pair().expect("packet fixture");
        let fd: OwnedFd = client.into();
        let shared = Shared::new(Async::new(File::from(fd)).expect("source"), [1; 16]);
        let (events, mut receiving) = mpsc::channel(1);
        events
            .try_send(DriverEvent::TransportUnavailable)
            .expect("prior queued event");
        shared.ipc.lock().expect("state").events = Some(events);
        let reader = tokio::spawn(receive(Arc::downgrade(&shared)));
        drop(StartupGuard(Some(shared.clone())));
        assert!(shared.io.lock().expect("resources").is_none());
        assert!(!shared.ipc.lock().expect("state").available);
        assert!(matches!(
            receiving.recv().await,
            Some(DriverEvent::TransportUnavailable)
        ));
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(2), receiving.recv())
                .await
                .expect("actual producer closure")
                .is_none()
        );
        reader
            .await
            .expect("no detached session reader")
            .expect("reader stopped");
    }
    #[tokio::test]
    async fn shutdown_joins_admitted_resource_cleanup_before_reporting_success() {
        for cleanup_error in [None, Some(ModuleError::Forbidden)] {
            let (client, peer) = UnixDatagram::pair().expect("packet pair");
            let fd: OwnedFd = client.into();
            let shared = Shared::new(Async::new(File::from(fd)).expect("async fd"), [1; 16]);
            let info = wire::tests::info();
            shared.info.set(info.clone()).expect("info");
            let reader = tokio::spawn(receive(Arc::downgrade(&shared)));
            let session = Arc::new(NativeSession {
                shared: shared.clone(),
                info: info.clone(),
                reader: Mutex::new(Some(reader)),
                loader: Mutex::new(None),
                close: Mutex::new(Close::Open),
                quiesce: Mutex::new(None),
            });
            let peer = Async::new(peer).expect("peer");
            let responder = tokio::spawn(async move {
                for kind in [5u16, 4] {
                    let mut bytes = vec![0; wire::MAX_FRAME];
                    let n = peer
                        .read_with(|p| p.recv(&mut bytes))
                        .await
                        .expect("request");
                    let request =
                        DriverFrame::decode(&bytes[..n], wire::MAX_FRAME, wire::MAX_TRANSFER)
                            .expect("frame");
                    assert_eq!(request.message_type, kind);
                    let mut body = vec![0; 8];
                    let mut status = wire::tests::status_body();
                    status[16..20].copy_from_slice(&2u32.to_le_bytes());
                    body.extend(status);
                    let reply = DriverFrame {
                        class: FrameClass::Reply,
                        message_type: kind,
                        session: info.session_id,
                        request_id: request.request_id,
                        total_bytes: body.len() as u64,
                        offset: 0,
                        body,
                    }
                    .encode()
                    .expect("reply");
                    peer.write_with(|p| p.send(&reply)).await.expect("sent");
                }
            });
            // A cancelled startup caller drops its token, but the already started
            // asset/resource task still owns the clone until cleanup really ends.
            let caller = shared.retain_admission().expect("admitted");
            let retained = caller.clone();
            drop(caller);
            let mut stop = shared.admission_stop.subscribe();
            let gate = Arc::new(Notify::new());
            let release = gate.clone();
            let owner = shared.clone();
            let cleanup = tokio::spawn(async move {
                let _retained = retained;
                stop.changed().await.expect("retiring signal");
                release.notified().await;
                if let Some(error) = cleanup_error {
                    owner.record_tls_exit(Err(error));
                    owner.record_tls_exit(Err(ModuleError::Busy));
                }
            });
            let stopping = session.clone();
            let closed = tokio::spawn(async move { stopping.shutdown().await });
            tokio::time::timeout(std::time::Duration::from_secs(2), async {
                while !shared.work.is_closed() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("native quiesced, awaiting admitted resources");
            assert!(
                !closed.is_finished(),
                "shutdown must retain unfinished resource cleanup"
            );
            let (output, _rx) = mpsc::channel(1);
            let rejected = session
                .start_tls_ingress(
                    TlsIngressInput {
                        boot_id: [1; 16],
                        scope: as_types::flow::FlowScope::AllProtected,
                        destinations: vec!["192.0.2.1:443".parse().expect("address")],
                        config_version: as_types::configuration::ConfigVersion::from_digest(
                            [1; 32],
                        ),
                        max_connections: 1,
                    },
                    output,
                )
                .await;
            assert!(
                matches!(rejected, Err(ModuleError::Stopped)),
                "retirement precedes asset access or new resource ownership"
            );
            gate.notify_one();
            cleanup.await.expect("cleanup ended");
            let outcome = tokio::time::timeout(std::time::Duration::from_secs(2), closed)
                .await
                .expect("joined shutdown")
                .expect("owner task");
            assert!(matches!(
                (cleanup_error, outcome),
                (None, Ok(())) | (Some(ModuleError::Forbidden), Err(ModuleError::Forbidden))
            ));
            responder.await.expect("peer ended");
            assert!(shared.work.is_empty());
        }
    }
}
