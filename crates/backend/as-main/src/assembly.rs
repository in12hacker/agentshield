//! Main composes initialized modules through their interfaces; configuration alone discovers paths.
use as_policy_engine::{
    PolicyEngine,
    interaction::{Interaction, Prompt},
};
use as_runtime_config::{ConfigError, ConfigModule};
use as_types::context::{MainContext, TargetPlatform};
use as_types::{
    driver::{DriverAdapter, DriverConnectInput},
    driver_control::{FeatureState, SourceState},
    driver_image::DriverInitParts,
    main_ports::{MainModules, MessageModule},
};
use std::{num::NonZeroUsize, sync::Arc};
use tokio::sync::mpsc;
/// Actual supplied backend capabilities; no process-tree or media implementation is invented here.
pub struct MainComponents {
    /// Actual message consumer.
    pub messages: Arc<dyn MessageModule>,
}
/// Initialized source owners; business readiness is returned only after actual native adoption.
pub struct MainAssembly {
    credentials: Arc<std::sync::OnceLock<as_credentials::CredentialModule>>,
    configuration: Arc<ConfigModule>,
    context: MainContext,
    policy: Arc<PolicyEngine>,
    interaction: Arc<Interaction>,
    prompts: Option<mpsc::Receiver<Prompt>>,
    log_storage: Arc<as_types::log_storage::LogStorageBudget>,
}
/// Initialization failed before business admission.
#[derive(Debug, thiserror::Error)]
pub enum AssemblyError {
    /// The security-audit owner could not open its actual fixed database.
    #[error("security log initialization: {0}")]
    SecurityLog(#[source] std::io::Error),
    /// Private material could not be read or actually persisted.
    #[error(transparent)]
    Credentials(#[from] as_types::flow::CredentialError),
    /// Configured TLS runtime could not start.
    #[error(transparent)]
    Flow(#[from] as_types::flow::FlowError),
    /// System process inventory or source fence could not be confirmed.
    #[error(transparent)]
    ProcessTree(#[from] as_process_tree::Error),
    /// Owning runtime or actual session execution failed.
    #[error(transparent)]
    Runtime(#[from] std::io::Error),
    /// Actual driver rejected the connection or complete initialization.
    #[error(transparent)]
    Native(#[from] as_types::main_ports::ModuleError),
    /// Fixed configuration initialization failed.
    #[error(transparent)]
    Configuration(#[from] ConfigError),
    /// The rule owner rejected its fixed source.
    #[error(transparent)]
    Rules(#[from] as_policy_engine::Error),
}
impl MainAssembly {
    /// Initialize configuration first, then compose shared context and initialize ordinary rules.
    pub async fn initialize() -> Result<Self, AssemblyError> {
        let configuration = Arc::new(ConfigModule::initialize()?);
        let context = MainContext::new(configuration.shared_paths(), TargetPlatform::current());
        let capacity: NonZeroUsize = configuration.snapshot().document().queues.review;
        let (interaction, prompts) = Interaction::channel(capacity);
        let policy = Arc::new(PolicyEngine::initialize(&context, interaction.clone()).await?);
        let log_storage = as_types::log_storage::LogStorageBudget::new(
            configuration.snapshot().document().log_storage,
        );
        Ok(Self {
            credentials: Arc::new(std::sync::OnceLock::new()),
            configuration,
            context,
            policy,
            interaction,
            prompts: Some(prompts),
            log_storage,
        })
    }
    /// The only configuration writer.
    pub fn configuration(&self) -> &Arc<ConfigModule> {
        &self.configuration
    }
    /// Same shared retention budget for both original database owners.
    pub fn log_storage(&self) -> &Arc<as_types::log_storage::LogStorageBudget> {
        &self.log_storage
    }
    /// Same path allocation as the configuration owner.
    pub fn context(&self) -> &MainContext {
        &self.context
    }
    /// Initialized rule module; no second rule implementation in main.
    pub fn policy(&self) -> &Arc<PolicyEngine> {
        &self.policy
    }
    /// Rule-owned channel registration interface used after middleware authentication.
    pub fn interaction(&self) -> &Arc<Interaction> {
        &self.interaction
    }
    /// Compose all queues before native admission and return only after actual complete adoption.
    pub async fn start(
        &self,
        adapter: Arc<dyn DriverAdapter>,
        components: MainComponents,
        shutdown: tokio::sync::broadcast::Receiver<()>,
    ) -> Result<crate::MainThreads, AssemblyError> {
        let configuration = self.configuration.clone();
        let context = self.context.clone();
        let policy = self.policy.clone();
        let credential_slot = self.credentials.clone();
        let log_storage = self.log_storage.clone();
        retain_startup(async move {
            let snapshot = configuration.snapshot();
            let queues = snapshot.document().queues.clone();
            let mut threads = crate::MainThreads::prepare(context.clone(), queues.clone()).await?;
            let tree = match as_process_tree::ProcessTree::prepare(
                configuration.snapshot().document().process_tree.clone(),
            ) {
                Ok(tree) => tree,
                Err(error) => {
                    let _ = threads.shutdown().await;
                    return Err(AssemblyError::ProcessTree(error));
                }
            };
            threads.bind_tree(tree.clone());
            let input = DriverConnectInput {
                platform: context.platform(),
                host_boot_id: tree.boot_id(),
                driver_assets_directory: context.paths().driver_assets_directory().to_path_buf(),
            };
            let connected = threads
                .dispatch()
                .spawn(crate::ProcessingThread::Fast, async move {
                    adapter.connect(&input).await
                })
                .await
                .map_err(|_| std::io::Error::other("native connect task failed"));
            let session = match connected {
                Ok(Ok(session)) => session,
                failed => {
                    let _ = threads.shutdown().await;
                    return Err(match failed {
                        Ok(Err(error)) => AssemblyError::Native(error),
                        Err(error) => AssemblyError::Runtime(error),
                        _ => AssemblyError::Native(as_types::main_ports::ModuleError::Unavailable),
                    });
                }
            };
            let info = session.info();
            let port = crate::messages::driver::DriverPort::bind(session);
            if let Err(error) = threads.bind_driver(port.clone()) {
                let _ = threads.shutdown().await;
                return Err(AssemblyError::Runtime(error));
            }
            // Protected writes require this real authenticated controller.
            // A cold backend has an exec identity before connect, but not the
            // controller authority needed to open the existing audit database.
            let security_log =
                match as_security_log::SecurityLog::open(context.paths(), log_storage).await {
                    Ok(log) => Box::new(log),
                    Err(error) => {
                        let _ = threads.shutdown().await;
                        return Err(AssemblyError::SecurityLog(error));
                    }
                };
            threads.bind_security_log(security_log.reader());
            if let Err(error) = policy.bind_scope_source(port.process_facts()?) {
                let _ = threads.shutdown().await;
                return Err(error.into());
            }
            let prepared = (|| {
                let configuration = configuration.prepare_driver_config(&info.capabilities)?;
                let (rules, lease) = policy.prepare_driver_policy(&info.capabilities)?;
                Ok::<_, AssemblyError>((
                    DriverInitParts {
                        configuration,
                        rules,
                        expected_persistent_generation: info.status.persistent_generation,
                        expected_lifecycle_epoch: info.status.lifecycle_epoch,
                    },
                    lease,
                ))
            })();
            let (parts, lease) = match prepared {
                Ok(parts) => parts,
                Err(error) => {
                    let _ = threads.shutdown().await;
                    return Err(error);
                }
            };
            let modules = MainModules {
                rules: policy.clone(),
                fast_facts: tree.clone(),
                slow_facts: None,
                messages: components.messages,
            };
            let ingress = match threads.connect(modules.clone(), security_log, shutdown) {
                Ok(ingress) => ingress,
                Err(error) => {
                    let _ = threads.shutdown().await;
                    return Err(AssemblyError::Runtime(error));
                }
            };
            let (sending, events) = mpsc::channel(queues.callbacks.get());
            let (controlled, operations) = mpsc::channel(queues.callbacks.get());
            let callbacks = as_types::driver::DriverCallbacks {
                controlled,
                events: sending,
            };
            let message_consumer = modules.messages.clone();
            threads.native_events(operations, events, ingress, modules);
            let start_port = port.clone();
            let start = threads
                .dispatch()
                .spawn(crate::ProcessingThread::Fast, async move {
                    start_port.start_tracking(callbacks).await
                })
                .await
                .map_err(|_| std::io::Error::other("fact startup task failed"));
            let tree_ready = match start {
                Ok(Ok(start)) => tree
                    .initialize(start, port.process_facts()?)
                    .await
                    .map_err(AssemblyError::from),
                Ok(Err(e)) => Err(AssemblyError::Native(e)),
                Err(e) => Err(AssemblyError::Runtime(e)),
            };
            if let Err(error) = tree_ready {
                let _ = threads.shutdown().await;
                return Err(error);
            }
            if configuration.startup_group_observation().is_some() {
                let (sending, events) = mpsc::channel(queues.messages.get());
                if let Err(error) = port.bind_group_tcp(sending) {
                    let _ = threads.shutdown().await;
                    return Err(error.into());
                }
                threads.group_consumer(message_consumer.clone(), events, tree.boot_id());
            }
            let native = threads
                .dispatch()
                .spawn(crate::ProcessingThread::Fast, async move {
                    port.initialize(parts, lease).await
                })
                .await;
            match native {
                Ok(Ok(applied))
                    if applied.status.source_state == SourceState::Running
                        && applied.status.self_protection == FeatureState::Enabled =>
                {
                    threads.start_tree_reconciliation();
                    let snapshot = configuration.snapshot();
                    let credentials = as_credentials::CredentialModule::initialize(
                        as_credentials::CredentialInit {
                            protected_root: snapshot.paths().self_protection_root().to_path_buf(),
                            store_file: snapshot.paths().credential_store_file().to_path_buf(),
                            ca_public_file: snapshot.paths().ca_public_file().to_path_buf(),
                            bindings: Arc::new(snapshot.document().credentials.clone()),
                            routes: snapshot
                                .document()
                                .flow
                                .as_ref()
                                .map(|f| f.provider_routes.clone())
                                .unwrap_or_default(),
                            config_version: snapshot.version(),
                        },
                    )
                    .await;
                    let credentials = match credentials {
                        Ok(owner) => owner,
                        Err(error) => {
                            let _ = threads.shutdown().await;
                            return Err(error.into());
                        }
                    };
                    if let Err(error) = threads
                        .start_flow(configuration.clone(), credentials.clone(), message_consumer)
                        .await
                    {
                        let _ = threads.shutdown().await;
                        return Err(error);
                    }
                    if credential_slot.set(credentials.clone()).is_err() {
                        let _ = credentials.shutdown().await;
                        let _ = threads.shutdown().await;
                        return Err(as_types::main_ports::ModuleError::Conflict.into());
                    }

                    Ok(threads)
                }
                result => {
                    let _ = threads.shutdown().await;
                    Err(match result {
                        Ok(Err(error)) => AssemblyError::Native(error),
                        _ => AssemblyError::Native(as_types::main_ports::ModuleError::Unavailable),
                    })
                }
            }
        })
        .await
    }
    /// Wire the initialized owners into main's one authenticated message entry.
    pub fn messages(&self, threads: &crate::MainThreads) -> Arc<crate::messages::MainMessages> {
        crate::messages::MainMessages::new(
            self.configuration.clone(),
            self.policy.clone(),
            self.interaction.clone(),
            threads.driver_port(),
            threads,
            self.credentials
                .get()
                .map(|credentials| crate::messages::FlowPorts {
                    ingress: threads.flow_ingress_port(),
                    status: threads.flow_status_port(),
                    credentials: credentials.clone(),
                }),
        )
    }
    /// Move the single rule prompt receiver to the transport task once; no duplicate consumer.
    pub fn take_prompts(&mut self) -> Option<mpsc::Receiver<Prompt>> {
        self.prompts.take()
    }
}

async fn retain_startup(
    work: impl std::future::Future<Output = Result<crate::MainThreads, AssemblyError>> + Send + 'static,
) -> Result<crate::MainThreads, AssemblyError> {
    let (delivery, receiving) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let result = work.await;
        if let Err(Ok(threads)) = delivery.send(result)
            && let Err(error) = threads.shutdown().await
        {
            tracing::error!(%error,"cancelled startup resource retirement unconfirmed");
        }
    });
    receiving
        .await
        .map_err(|_| AssemblyError::Runtime(std::io::Error::other("startup owner unavailable")))?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn cancelled_start_delivery_keeps_real_work_and_joins_prepared_threads() {
        let (_root, context, queues) = crate::test_driver::environment();
        let threads = crate::test_driver::start(
            context,
            queues,
            Arc::new(crate::test_driver::SoftwareDriver),
        )
        .await
        .expect("actual worker threads");
        let dispatch = threads.dispatch();
        let entered = Arc::new(tokio::sync::Notify::new());
        let gate = Arc::new(tokio::sync::Notify::new());
        let step = entered.clone();
        let release = gate.clone();
        let caller = tokio::spawn(retain_startup(async move {
            step.notify_one();
            release.notified().await;
            Ok(threads)
        }));
        entered.notified().await;
        caller.abort();
        let _ = caller.await;
        dispatch
            .spawn(crate::ProcessingThread::Fast, async {})
            .await
            .expect("work still owned before completion");
        gate.notify_one();
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if dispatch
                    .spawn(crate::ProcessingThread::Fast, async {})
                    .await
                    .is_err()
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("unclaimed completed startup really retires workers");
    }
}
