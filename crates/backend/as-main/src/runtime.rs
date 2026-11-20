//! Three fixed processing threads; submodules keep their typed queues and state.
use crate::messages::MainMessages;
use as_types::main_ports::ModuleError;
use std::{future::Future, io, thread};
use tokio::{
    runtime::{Builder, Handle},
    sync::oneshot,
    task::JoinHandle,
};
use tokio_util::task::TaskTracker;

/// One accepted host request; the observer confirms only the original reply's full local write.
pub struct MainStopRequest {
    /// Success means the original writer completed the Accepted frame, not host exit or peer read.
    pub reply_written: oneshot::Receiver<()>,
}

/// Unique runtime-only capability moved with the original Accepted reply into its actual writer.
pub struct StopReplyWrite {
    reply_written: oneshot::Sender<()>,
}
impl StopReplyWrite {
    /// Consume only after that original frame was fully written. Dropping it confirms nothing.
    pub fn written(self) {
        let _ = self.reply_written.send(());
    }
}

#[derive(Clone)]
pub(crate) struct MainStopPort(
    std::sync::Arc<std::sync::Mutex<Option<oneshot::Sender<MainStopRequest>>>>,
);
impl MainStopPort {
    fn channel() -> (Self, oneshot::Receiver<MainStopRequest>) {
        let (sending, receiving) = oneshot::channel();
        (
            Self(std::sync::Arc::new(std::sync::Mutex::new(Some(sending)))),
            receiving,
        )
    }

    pub(crate) fn request(&self) -> Result<StopReplyWrite, as_types::main_ports::ModuleError> {
        use as_types::main_ports::ModuleError;
        let sending = self
            .0
            .lock()
            .map_err(|_| ModuleError::Unavailable)?
            .take()
            .ok_or(ModuleError::Unavailable)?;
        let (reply_written, receiving) = oneshot::channel();
        sending
            .send(MainStopRequest {
                reply_written: receiving,
            })
            .map_err(|_| ModuleError::Unavailable)?;
        Ok(StopReplyWrite { reply_written })
    }
}

/// Explicit execution affinity for each processing component.
#[derive(Clone, Copy, Debug)]
pub enum ProcessingThread {
    /// Driver reception and quick matching on the callback thread.
    Fast,
    /// Serial slow matching and the one active question on the calling main runtime.
    Main,
    /// Frontend query, message and log work.
    Messages,
}

#[derive(Clone)]
struct Lane {
    handle: Handle,
    work: TaskTracker,
}

/// Execution ports without a second policy, process registry or health store.
#[derive(Clone)]
pub struct MainDispatch {
    fast: Lane,
    main: Lane,
    messages: Lane,
}
impl MainDispatch {
    fn lane(&self, target: ProcessingThread) -> &Lane {
        match target {
            ProcessingThread::Fast => &self.fast,
            ProcessingThread::Main => &self.main,
            ProcessingThread::Messages => &self.messages,
        }
    }
    /// Construct a module's internal tasks on its assigned runtime.
    pub fn in_thread<R>(&self, target: ProcessingThread, start: impl FnOnce() -> R) -> R {
        let _runtime = self.lane(target).handle.enter();
        start()
    }
    /// Enqueue original work on the assigned runtime.
    pub fn spawn<F>(&self, target: ProcessingThread, work: F) -> JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        let lane = self.lane(target);
        lane.work.spawn_on(work, &lane.handle)
    }
}

struct Worker {
    lane: Lane,
    stop: Option<oneshot::Sender<()>>,
    join: Option<thread::JoinHandle<()>>,
}

impl Worker {
    async fn start(name: &'static str) -> io::Result<Self> {
        let (stop, stopped) = oneshot::channel();
        let (ready, started) = oneshot::channel();
        let join = thread::Builder::new().name(name.into()).spawn(move || {
            // Build and drop on the owning OS thread, including startup failure.
            let runtime = match Builder::new_current_thread().enable_all().build() {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = ready.send(Err(error));
                    return;
                }
            };
            let work = TaskTracker::new();
            let lane = Lane {
                handle: runtime.handle().clone(),
                work: work.clone(),
            };
            if ready.send(Ok(lane)).is_err() {
                return;
            }
            runtime.block_on(async move {
                let _ = stopped.await;
                work.close();
                work.wait().await;
            });
        })?;
        let lane = match started.await {
            Ok(Ok(lane)) => lane,
            result => {
                let error = match result {
                    Ok(Err(error)) => error,
                    _ => io::Error::other("main processing thread startup unavailable"),
                };
                let _ = stop.send(());
                if let Err(join_error) = join_thread(join).await {
                    tracing::error!(%join_error, "failed processing thread retirement unconfirmed");
                }
                return Err(error);
            }
        };
        Ok(Self {
            lane,
            stop: Some(stop),
            join: Some(join),
        })
    }

    async fn shutdown(mut self) -> io::Result<()> {
        self.stop();
        match self.join.take() {
            Some(join) => join_thread(join).await,
            None => Ok(()),
        }
    }

    fn stop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

async fn join_thread(join: thread::JoinHandle<()>) -> io::Result<()> {
    tokio::task::spawn_blocking(move || {
        join.join()
            .map_err(|_| io::Error::other("main processing thread panicked"))
    })
    .await
    .map_err(|_| io::Error::other("main processing thread join unavailable"))?
}

impl Drop for Worker {
    fn drop(&mut self) {
        // Early failure requests retirement; the owning runtime drains real work.
        // A dropped join handle never claims that its thread has ended.
        self.stop();
    }
}

/// Owns callback/message threads; the caller runs the serial main loop.
pub struct MainThreads {
    group_consumer: Option<JoinHandle<Result<(), ModuleError>>>,
    native_consumers: [Option<JoinHandle<Result<(), ModuleError>>>; 2],

    credentials: Option<as_credentials::CredentialModule>,
    flow: Option<crate::flow_runtime::FlowRuntime>,
    context: as_types::context::MainContext,
    queues: as_types::configuration::QueueSettings,
    driver: Option<crate::messages::driver::DriverPort>,
    main: Lane,
    fast: Worker,
    messages: Worker,
    connection: Option<MainConnection>,
    management: std::sync::OnceLock<std::sync::Arc<crate::messages::MainMessages>>,
    stop_port: std::sync::OnceLock<Option<MainStopPort>>,
    tree: Option<std::sync::Arc<as_process_tree::ProcessTree>>,
    security_log: Option<as_security_log::SecurityLogReader>,
    tree_stop: Option<tokio::sync::watch::Sender<bool>>,
}

impl MainThreads {
    /// Prepare the three owning runtimes and queues; no native capability or business readiness is assumed.
    pub async fn prepare(
        context: as_types::context::MainContext,
        queues: as_types::configuration::QueueSettings,
    ) -> io::Result<Self> {
        let handle =
            Handle::try_current().map_err(|_| io::Error::other("main runtime unavailable"))?;
        if handle.runtime_flavor() != tokio::runtime::RuntimeFlavor::CurrentThread {
            return Err(io::Error::other(
                "main loop requires the current-thread main runtime",
            ));
        }
        let main = Lane {
            handle,
            work: TaskTracker::new(),
        };
        let fast = Worker::start("as-callbacks").await?;
        let messages = match Worker::start("as-messages").await {
            Ok(worker) => worker,
            Err(error) => {
                let _ = fast.shutdown().await;
                return Err(error);
            }
        };
        Ok(Self {
            group_consumer: None,
            native_consumers: [None, None],
            flow: None,
            credentials: None,
            context,
            queues,
            driver: None,
            main,
            fast,
            messages,
            connection: None,
            management: std::sync::OnceLock::new(),
            stop_port: std::sync::OnceLock::new(),
            tree: None,
            security_log: None,
            tree_stop: None,
        })
    }
    /// Bind the current host's one-shot stop listener once, before composing MainMessages.
    pub fn bind_stop_listener(
        &mut self,
    ) -> Result<oneshot::Receiver<MainStopRequest>, as_types::main_ports::ModuleError> {
        let (port, receiving) = MainStopPort::channel();
        self.stop_port
            .set(Some(port))
            .map_err(|_| as_types::main_ports::ModuleError::Conflict)?;
        Ok(receiving)
    }
    pub(crate) fn stop_port(&self) -> Option<MainStopPort> {
        // Resolving the sole composition setting seals an unbound host as unbound.
        self.stop_port.get_or_init(|| None).clone()
    }
    pub(crate) fn bind_driver(
        &mut self,
        port: crate::messages::driver::DriverPort,
    ) -> io::Result<()> {
        if self.driver.is_some() {
            return Err(io::Error::other("native session already bound"));
        }
        self.driver = Some(port);
        Ok(())
    }
    /// This instance's actual lower call port; never accepts a second independently supplied driver.
    pub fn driver_port(&self) -> crate::messages::driver::DriverPort {
        self.driver
            .clone()
            .unwrap_or_else(crate::messages::driver::DriverPort::unavailable)
    }
    pub(crate) fn management_entry(
        &self,
        initialize: impl FnOnce() -> std::sync::Arc<crate::messages::MainMessages>,
    ) -> std::sync::Arc<crate::messages::MainMessages> {
        self.management.get_or_init(initialize).clone()
    }
    pub(crate) fn group_consumer(
        &mut self,
        module: std::sync::Arc<dyn as_types::main_ports::MessageModule>,
        events: tokio::sync::mpsc::Receiver<as_types::driver::NativeGroupTcp>,
        boot: [u8; 16],
    ) {
        self.group_consumer = Some(
            self.dispatch()
                .spawn(ProcessingThread::Messages, async move {
                    module.consume_group(events, boot).await
                }),
        );
    }
    pub(crate) fn native_events(
        &mut self,
        controlled: tokio::sync::mpsc::Receiver<as_types::main_ports::DispatchInput>,
        events: tokio::sync::mpsc::Receiver<as_types::driver::DriverEvent>,
        ingress: crate::dispatch::MainIngress,
        modules: as_types::main_ports::MainModules,
    ) {
        let dispatch = self.dispatch();
        let port = self.driver_port();
        let tree = self.tree.clone();
        let control_port = port.clone();
        let control_ingress = ingress.clone();
        let control_modules = modules.clone();
        let control_tree = tree.clone();
        self.native_consumers[0] = Some(dispatch.spawn(ProcessingThread::Fast, async move {
            let originals = crate::native_events::controlled(
                controlled,
                control_port.clone(),
                control_ingress.clone(),
                control_modules,
                control_tree,
            );
            supervise_native(originals, control_port, control_ingress).await
        }));
        self.native_consumers[1] = Some(dispatch.spawn(ProcessingThread::Messages, async move {
            let auxiliary =
                crate::native_events::run(events, port.clone(), ingress.clone(), modules, tree);
            supervise_native(auxiliary, port, ingress).await
        }));
    }
    pub(crate) fn bind_tree(&mut self, tree: std::sync::Arc<as_process_tree::ProcessTree>) {
        self.tree = Some(tree);
    }
    pub(crate) fn bind_security_log(&mut self, reader: as_security_log::SecurityLogReader) {
        self.security_log = Some(reader);
    }
    pub(crate) fn security_log(&self) -> Option<as_security_log::SecurityLogReader> {
        self.security_log.clone()
    }
    pub(crate) fn start_tree_reconciliation(&mut self) {
        if let Some(tree) = self.tree.clone() {
            let (stop, watch) = tokio::sync::watch::channel(false);
            self.dispatch()
                .spawn(ProcessingThread::Messages, async move {
                    tree.run_reconciliation(watch).await;
                });
            self.tree_stop = Some(stop);
        }
    }
    /// The one process tree composed and initialized by this main instance.
    pub fn process_tree(&self) -> Option<std::sync::Arc<as_process_tree::ProcessTree>> {
        self.tree.clone()
    }

    /// Common readonly interface data, sharing configuration's original path allocation.
    pub fn context(&self) -> &as_types::context::MainContext {
        &self.context
    }

    /// Obtain execution ports without transferring their lifetime ownership.
    pub fn dispatch(&self) -> MainDispatch {
        MainDispatch {
            fast: self.fast.lane.clone(),
            main: self.main.clone(),
            messages: self.messages.lane.clone(),
        }
    }

    /// Actual failed completion/status enqueues; None before the main entry connects.
    /// This is independent of Native dropped and subsequent media delivery.
    pub fn rejected_notices(&self) -> Option<u64> {
        self.connection
            .as_ref()
            .map(|c| c.ingress.rejected_notices())
    }

    /// Actual failed rule-log enqueues; None before the main entry connects.
    pub fn rejected_logs(&self) -> Option<u64> {
        self.connection.as_ref().map(|c| c.ingress.rejected_logs())
    }

    pub(crate) async fn start_flow(
        &mut self,
        configuration: std::sync::Arc<as_runtime_config::ConfigModule>,
        credentials: as_credentials::CredentialModule,
        messages: std::sync::Arc<dyn as_types::main_ports::MessageModule>,
    ) -> Result<(), crate::assembly::AssemblyError> {
        self.credentials = Some(credentials.clone());
        if self.connection.is_none() {
            return Err(io::Error::other("main event entry missing").into());
        }
        let snapshot = configuration.snapshot();
        let driver = self.driver_port();
        let boot_id = self
            .tree
            .as_ref()
            .ok_or_else(|| io::Error::other("TLS ingress requires the initialized process tree"))?
            .boot_id();
        self.flow = self
            .dispatch()
            .spawn(ProcessingThread::Messages, async move {
                crate::flow_runtime::FlowRuntime::start(
                    snapshot,
                    credentials,
                    driver,
                    messages,
                    boot_id,
                )
                .await
            })
            .await
            .map_err(|_| io::Error::other("TLS startup task failed"))??;
        Ok(())
    }
    pub(crate) fn flow_status_port(
        &self,
    ) -> Option<std::sync::Arc<dyn as_types::flow::FlowStatusPort>> {
        self.flow.as_ref().map(|flow| flow.status_port())
    }

    pub(crate) fn flow_ingress_port(
        &self,
    ) -> Option<std::sync::Arc<dyn as_types::driver::TlsIngressSession>> {
        self.flow.as_ref().map(|flow| flow.ingress_port())
    }
    /// Connect the one new event entry with explicit functional modules.
    /// Smoke implementations and later production modules use this same contract.
    pub fn connect(
        &mut self,
        modules: as_types::main_ports::MainModules,
        security_log: Box<dyn as_types::main_ports::SecurityLogSink>,
        shutdown: tokio::sync::broadcast::Receiver<()>,
    ) -> io::Result<crate::dispatch::MainIngress> {
        if self.connection.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "main event entry already connected",
            ));
        }
        let (ingress, workers) =
            crate::dispatch::main_dispatch(modules, security_log, &self.queues, shutdown);
        let dispatch = self.dispatch();
        let tasks = [
            Some(dispatch.spawn(ProcessingThread::Fast, workers.fast)),
            Some(dispatch.spawn(ProcessingThread::Main, workers.main)),
            Some(dispatch.spawn(ProcessingThread::Messages, workers.messages)),
        ];
        self.connection = Some(MainConnection {
            ingress: ingress.clone(),
            tasks,
        });
        Ok(ingress)
    }

    /// Retire external management first, retaining callback consumers until
    /// internal resource owners finish the I/O required for their own cleanup.
    /// The independent retirement task remains owned if this waiter is cancelled.
    pub fn shutdown(self) -> impl Future<Output = io::Result<()>> {
        self.start_shutdown(MainMessages::close)
    }

    /// Retire the same runtime while the host still owns an authenticated content writer.
    /// The host closes the connection after its committed tail is written or delivery fails.
    pub fn shutdown_draining_content(self) -> impl Future<Output = io::Result<()>> {
        self.start_shutdown(MainMessages::seal_for_content_drain)
    }

    fn start_shutdown(
        self,
        seal: fn(&MainMessages) -> Result<(), ModuleError>,
    ) -> impl Future<Output = io::Result<()>> {
        let management_closed = self
            .management
            .get()
            .map_or(Ok(()), |messages| seal(messages).map_err(io::Error::other));
        let handle = self.main.handle.clone();
        let retirement = handle.spawn(async move { self.retire(management_closed).await });
        async move {
            retirement
                .await
                .map_err(|_| io::Error::other("main retirement task failed"))?
        }
    }

    async fn retire(mut self, management_closed: io::Result<()>) -> io::Result<()> {
        if let Some(stop) = self.tree_stop.take() {
            stop.send_replace(true);
        }
        if let Some(messages) = self.management.get() {
            messages.drain_commands().await;
        }
        let commands = management_closed;
        // These owners can still need ordinary LSM callbacks (for example nft
        // exec/file operations). Do not close their consumer before they finish.
        let flow = match self.flow.take() {
            Some(flow) => flow.stop().await,
            None => Ok(()),
        };
        let credentials = match self.credentials.take() {
            Some(credentials) => credentials.shutdown().await.map_err(io::Error::other),
            None => Ok(()),
        };
        if let Some(tree) = &self.tree {
            tree.stop_refresh().await;
        }
        if let Some(connection) = &self.connection {
            connection.ingress.close_admission();
        }
        let admission = match &self.driver {
            Some(driver) => driver.stop_admission().map_err(io::Error::other),
            None => Ok(()),
        };
        let mut connection = self.connection.take();
        let connected = match &mut connection {
            Some(connection) => connection.drain_processing().await,
            None => Ok(()),
        };
        // Native read/write tasks still run on the fast runtime until actual source retirement.
        let driver = match &self.driver {
            Some(driver) => driver.shutdown().await.map_err(io::Error::other),
            None => Ok(()),
        };
        let mut native = Ok(());
        for consumer in self.native_consumers.iter_mut().filter_map(Option::take) {
            let joined = join_work(consumer).await;
            native = native.and(joined);
        }
        self.main.work.close();
        self.main.work.wait().await;
        self.fast.stop();
        let callbacks = self.fast.shutdown().await;
        let tree = match &self.tree {
            Some(tree) => tree.close().await.map_err(io::Error::other),
            None => Ok(()),
        };
        let distributed = match connection {
            Some(connection) => connection.drain_messages().await,
            None => Ok(()),
        };
        let group = match self.group_consumer.take() {
            Some(consumer) => consumer
                .await
                .map_err(io::Error::other)
                .and_then(|result| result.map_err(io::Error::other)),
            None => Ok(()),
        };
        self.messages.stop();
        let messages = self.messages.shutdown().await;
        let leases = self.driver.as_ref().map_or(Ok(()), |driver| {
            driver.release_leases().map_err(io::Error::other)
        });
        if let Some(driver) = &self.driver {
            let rejected = driver.rejected_callbacks();
            if rejected != 0 {
                tracing::warn!(
                    rejected,
                    "backend callbacks were not delivered; native dropped counter is separate"
                );
            }
        }
        // Every retirement operation above has already executed. Preserve the
        // same first-error ordering, with enough context to identify its source.
        [
            ("commands", commands),
            ("flow", flow),
            ("credentials", credentials),
            ("admission", admission),
            ("processing", connected),
            ("driver", driver),
            ("native_consumers", native),
            ("workers", callbacks),
            ("tree", tree),
            ("messages", distributed),
            ("group", group),
            ("workers", messages),
            ("leases", leases),
        ]
        .into_iter()
        .try_for_each(|(stage, result)| result.map_err(|error| retirement_error(stage, error)))
    }
}

#[derive(Debug)]
struct RetirementError {
    stage: &'static str,
    source: io::Error,
}
impl std::fmt::Display for RetirementError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "retirement stage={}: {}", self.stage, self.source)
    }
}
impl std::error::Error for RetirementError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}
/// Annotate an actual failed retirement step without erasing its error/source chain.
pub(crate) fn retirement_error(stage: &'static str, source: io::Error) -> io::Error {
    io::Error::new(source.kind(), RetirementError { stage, source })
}

struct MainConnection {
    ingress: crate::dispatch::MainIngress,
    tasks: [Option<JoinHandle<Result<(), as_types::main_ports::ModuleError>>>; 3],
}
impl MainConnection {
    async fn drain_processing(&mut self) -> io::Result<()> {
        self.ingress.close();
        let mut result = Ok(());
        for task in self.tasks[..2].iter_mut().filter_map(Option::take) {
            result = result.and(join_work(task).await);
        }
        result
    }
    async fn drain_messages(mut self) -> io::Result<()> {
        match self.tasks[2].take() {
            Some(task) => join_work(task).await,
            None => Ok(()),
        }
    }
}
async fn supervise_native(
    work: impl Future<Output = Result<(), ModuleError>>,
    port: crate::messages::driver::DriverPort,
    ingress: crate::dispatch::MainIngress,
) -> Result<(), ModuleError> {
    let result = work.await;
    if let Err(error) = result {
        tracing::error!(%error,"native callback stream failed");
        ingress.close();
        if let Err(failure) = port
            .request(as_types::driver_control::DriverCommand::DeclareFailure)
            .await
        {
            tracing::warn!(%failure,"native source failure acknowledgement unavailable");
        }
        if let Err(failure) = ingress
            .driver_notice(as_types::main_ports::MainMessage::DriverUnavailable)
            .await
        {
            tracing::warn!(%failure,"native failure consumer closed");
        }
    }
    result
}

async fn join_work(
    task: JoinHandle<Result<(), as_types::main_ports::ModuleError>>,
) -> io::Result<()> {
    match task.await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(io::Error::other(error)),
        Err(_) => Err(io::Error::other("main event worker failed")),
    }
}

impl Drop for MainConnection {
    fn drop(&mut self) {
        self.ingress.close();
    }
}

#[cfg(test)]
#[path = "smoke.rs"]
mod smoke;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stop_port_requires_a_live_listener_and_an_actual_write_capability() {
        use as_types::main_ports::ModuleError;
        let (port, listener) = MainStopPort::channel();
        let written = port.request().expect("host accepted");
        let mut request = listener.await.expect("the actual host request");
        assert!(matches!(port.request(), Err(ModuleError::Unavailable)));
        assert!(matches!(
            request.reply_written.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        written.written();
        request.reply_written.await.expect("consumed write witness");

        let (port, listener) = MainStopPort::channel();
        let unwritten = port.request().expect("second host accepted");
        let request = listener.await.expect("second original request");
        drop(unwritten);
        assert!(
            request.reply_written.await.is_err(),
            "drop never proves a write"
        );

        let (port, listener) = MainStopPort::channel();
        drop(listener);
        assert!(matches!(port.request(), Err(ModuleError::Unavailable)));
    }

    #[tokio::test]
    async fn stop_binding_is_sealed_at_composition_and_retirement_does_not_wait_for_the_reply() {
        use as_types::main_ports::ModuleError;
        let (_root, context, queues) = crate::test_driver::environment();
        let mut unbound = MainThreads::prepare(context.clone(), queues.clone())
            .await
            .expect("real worker threads");
        assert!(unbound.stop_port().is_none());
        assert!(matches!(
            unbound.bind_stop_listener(),
            Err(ModuleError::Conflict)
        ));
        unbound.shutdown().await.expect("unbound threads joined");

        let mut threads = MainThreads::prepare(context, queues)
            .await
            .expect("real host threads");
        let listener = threads.bind_stop_listener().expect("bind once");
        assert!(matches!(
            threads.bind_stop_listener(),
            Err(ModuleError::Conflict)
        ));
        let port = threads.stop_port().expect("same composed port");
        let pending = threads
            .dispatch()
            .spawn(ProcessingThread::Messages, async move { port.request() });
        let request = listener.await.expect("host took the original request");
        let unwritten = pending
            .await
            .expect("command task ended")
            .expect("accepted");
        tokio::time::timeout(std::time::Duration::from_secs(2), threads.shutdown())
            .await
            .expect("no wait on the host's own reply observer")
            .expect("all actual worker joins completed");
        drop(unwritten);
        assert!(request.reply_written.await.is_err());
    }

    struct RejectedDriver(as_types::main_ports::ModuleError);
    #[async_trait::async_trait]
    impl as_types::driver::DriverAdapter for RejectedDriver {
        async fn connect(
            &self,
            _: &as_types::driver::DriverConnectInput,
        ) -> Result<
            std::sync::Arc<dyn as_types::driver::DriverSession>,
            as_types::main_ports::ModuleError,
        > {
            Err(self.0)
        }
    }
    #[tokio::test]
    async fn driver_load_or_authentication_failure_stops_before_business_start() {
        let (_root, context, queues) = crate::test_driver::environment();
        for reason in [
            as_types::main_ports::ModuleError::Unavailable,
            as_types::main_ports::ModuleError::Unauthenticated,
        ] {
            let error = match crate::test_driver::start(
                context.clone(),
                queues.clone(),
                std::sync::Arc::new(RejectedDriver(reason)),
            )
            .await
            {
                Err(error) => error,
                Ok(_) => panic!("failed driver/authentication must not start business threads"),
            };
            assert!(
                error
                    .to_string()
                    .starts_with("driver initialization failed:")
            );
        }
    }

    #[tokio::test]
    async fn three_real_threads_execute_their_work_and_join() {
        let (_root, context, queues) = crate::test_driver::environment();
        let threads = crate::test_driver::start(
            context,
            queues,
            std::sync::Arc::new(crate::test_driver::SoftwareDriver),
        )
        .await
        .expect("processing threads");
        let dispatch = threads.dispatch();
        let current = thread::current();
        let main_id = current.id();
        let main_name = current.name().map(str::to_owned);
        let mut observed = Vec::new();
        for (target, expected) in [
            (ProcessingThread::Main, main_name.as_deref().unwrap_or("")),
            (ProcessingThread::Fast, "as-callbacks"),
            (ProcessingThread::Messages, "as-messages"),
        ] {
            let (name, id) = dispatch
                .spawn(target, async {
                    let current = thread::current();
                    (current.name().map(str::to_owned), current.id())
                })
                .await
                .expect("actual dispatched work");
            assert_eq!(name.as_deref().unwrap_or(""), expected);
            if matches!(target, ProcessingThread::Main) {
                assert_eq!(id, main_id);
            }
            let internal =
                dispatch.in_thread(target, || tokio::spawn(async { thread::current().id() }));
            assert_eq!(internal.await.expect("module internal task"), id);
            assert!(!observed.contains(&id));
            observed.push(id);
        }
        threads.shutdown().await.expect("three physical joins");
    }
}
