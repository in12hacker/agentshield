//! Controlled TLS transport and current protocol transactions. History belongs to middleware.
#![forbid(unsafe_code)]
mod content;
mod feedback;
mod feedback_write;
mod message;
mod observe;
mod protocol;
mod tls;
pub use as_types::flow::FlowError;
use as_types::{configuration::ConfigVersion, flow::*};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU8, AtomicU64, Ordering},
};
use tokio::sync::{Semaphore, mpsc};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
/// Inputs composed by Main before enabling real ingress.
pub struct FlowInit {
    /// Immutable current flow settings.
    pub settings: Arc<FlowSettings>,
    /// Actual configuration digest.
    pub config_version: ConfigVersion,
    /// Real admitted streams, owned by the trusted adapter.
    pub ingress_rx: mpsc::Receiver<AdmittedConnection>,
    /// Actual credential owner.
    pub credentials: Arc<dyn CredentialPort>,
    /// Actual CA signing owner.
    pub certificates: Arc<dyn CertificatePort>,
    /// Existing Main message capacity, bounding unfinished feedback transactions.
    pub feedback_capacity: std::num::NonZeroUsize,
    /// Single real-time output; Main owns its unique consumer even without a frontend.
    pub output: ChatSender,
}
/// Runtime lifecycle owner. Status/feedback handles do not own a history store.
pub struct FlowModule {
    shared: Arc<Shared>,
    cancel: CancellationToken,
    worker: Option<tokio::task::JoinHandle<()>>,
}
pub(crate) struct Shared {
    settings: Arc<FlowSettings>,
    destinations: tls::Destinations,
    version: ConfigVersion,
    instance: [u8; 16],
    credentials: Arc<dyn CredentialPort>,
    certificates: Arc<dyn CertificatePort>,
    crypto: Arc<rustls::crypto::CryptoProvider>,
    upstream: Arc<rustls::ClientConfig>,
    roots: Arc<rustls::RootCertStore>,
    tasks: TaskTracker,
    force: CancellationToken,
    exchanges: Arc<Semaphore>,
    intake: AtomicU8,
    next: AtomicU64,
    event_sequence: AtomicU64,
    lost: AtomicU64,
    parse_failures: AtomicU64,
    output: Mutex<Option<ChatSender>>,
    feedback: Mutex<feedback::Feedback>,
    feedback_stop: CancellationToken,
}
impl FlowModule {
    /// Prepare TLS and bounded consumers before the caller enables redirection.
    pub async fn initialize(
        input: FlowInit,
    ) -> Result<(Self, Vec<std::net::SocketAddr>), FlowError> {
        let mut roots = rustls::RootCertStore::empty();
        let loaded = rustls_native_certs::load_native_certs();
        for cert in loaded.certs {
            roots.add(cert).map_err(|_| FlowError::Unavailable)?;
        }
        if roots.is_empty() {
            return Err(FlowError::Unavailable);
        }
        Self::with_roots(input, roots).await
    }
    async fn with_roots(
        input: FlowInit,
        roots: rustls::RootCertStore,
    ) -> Result<(Self, Vec<std::net::SocketAddr>), FlowError> {
        let s = &input.settings;
        let e = s.egress.as_ref().ok_or(FlowError::InvalidInput)?;
        if !s.enabled
            || s.mode != FlowMode::MediateTls
            || s.provider_routes.is_empty()
            || e.max_connections == 0
            || e.max_streams_per_connection == 0
            || s.decode_limits.max_message_bytes == 0
            || s.decode_limits.max_active_exchanges == 0
            || s.event_queue_bytes == 0
            || s.event_queue_bytes > u32::MAX as u64
            || u64::from(input.output.capacity_bytes()) != s.event_queue_bytes
            || s.decode_limits.max_connection_buffer_bytes == 0
            || s.decode_limits.max_message_bytes > s.decode_limits.max_connection_buffer_bytes
        {
            return Err(FlowError::InvalidInput);
        }
        input
            .certificates
            .public_ca(e.ca)
            .map_err(|_| FlowError::Unavailable)?;
        let destinations = tls::destinations(s).await?;
        for origin in destinations.keys() {
            input
                .certificates
                .server_identity(e.ca, origin.clone())
                .await
                .map_err(|_| FlowError::Unavailable)?;
        }
        let crypto = Arc::new(rustls::crypto::ring::default_provider());
        let roots = Arc::new(roots);
        let mut upstream = rustls::ClientConfig::builder_with_provider(crypto.clone())
            .with_safe_default_protocol_versions()
            .map_err(|_| FlowError::Unavailable)?
            .with_root_certificates((*roots).clone())
            .with_no_client_auth();
        upstream.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        upstream.enable_early_data = false;
        let mut instance = [0; 16];
        getrandom::fill(&mut instance).map_err(|_| FlowError::Unavailable)?;
        let max_connections = e.max_connections as usize;
        let ingress_addresses = destinations
            .values()
            .flatten()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let shared = Arc::new(Shared {
            exchanges: Arc::new(Semaphore::new(
                s.decode_limits.max_active_exchanges as usize,
            )),
            output: Mutex::new(Some(input.output)),
            settings: input.settings,
            destinations,
            version: input.config_version,
            instance,
            credentials: input.credentials,
            certificates: input.certificates,
            crypto,
            upstream: Arc::new(upstream),
            roots,
            tasks: TaskTracker::new(),
            force: CancellationToken::new(),
            intake: AtomicU8::new(0),
            next: AtomicU64::new(0),
            event_sequence: AtomicU64::new(0),
            lost: AtomicU64::new(0),
            parse_failures: AtomicU64::new(0),
            feedback: Mutex::new(feedback::Feedback::new(input.feedback_capacity)),
            feedback_stop: CancellationToken::new(),
        });
        let cancel = CancellationToken::new();
        let stop = cancel.clone();
        let state = shared.clone();
        let mut rx = input.ingress_rx;
        let worker = tokio::spawn(async move {
            let connections = Arc::new(Semaphore::new(max_connections));
            loop {
                tokio::select! {_ = stop.cancelled()=>break,_=state.force.cancelled()=>break,next=rx.recv()=>match next {
                    Some(connection)=>if let Ok(permit)=connections.clone().try_acquire_owned(){
                        let s=state.clone();
                        let channel=connection.facts.channel.clone();
                        let initiator=connection.facts.connection_initiator.process;
                        state.tasks.spawn(async move{
                            let _permit=permit;
                            tokio::select!{
                                _=s.force.cancelled()=>{let _terminal=tls::diagnosed(Some(s.as_ref()),None,Err::<(),_>(FlowError::Stopped),
                                    |_|format!("AS_FLOW_CONNECTION boottime_ns={} direction=agent channel={channel:?} initiator={initiator:?} stage=force_cancel",boottime())).await;},
                                result=tls::connection(s.clone(),connection)=>{let _terminal=tls::diagnosed(Some(s.as_ref()),None,result,
                                    |error|format!("AS_FLOW_CONNECTION_ERROR boottime_ns={} direction=agent channel={channel:?} initiator={initiator:?} error={error:?}",boottime())).await;},
                            }
                        });
                    },
                    None=>{state.source_closed();break;},
                }}
            }
            rx.close();
        });
        Ok((
            Self {
                shared,
                cancel,
                worker: Some(worker),
            },
            ingress_addresses,
        ))
    }

    /// Actual status only; no subscription, snapshot or historical context queries.
    pub fn status_port(&self) -> Arc<dyn FlowStatusPort> {
        self.shared.clone()
    }
    /// Explicit unfinished fixed-feedback transactions.
    pub fn feedback_port(&self) -> Arc<dyn FlowFeedbackPort> {
        Arc::new(feedback::Port(self.shared.clone()))
    }
    /// Seal admission, explicitly cancel owned network work, and wait for real retirement.
    pub async fn stop(mut self) -> Result<(), FlowError> {
        self.cancel.cancel();
        let admission = self.shared.feedback.lock();
        self.shared.intake.store(1, Ordering::Release);
        self.shared.feedback_stop.cancel();
        let sealed = admission.map(drop).map_err(|_| FlowError::Failed);
        self.shared.force.cancel();
        let worker = match self.worker.take() {
            Some(worker) => worker.await.map_err(|_| FlowError::Failed),
            None => Ok(()),
        };
        self.shared.tasks.close();
        self.shared.tasks.wait().await;
        self.shared.intake.store(2, Ordering::Release);
        let cleared = self
            .shared
            .feedback
            .lock()
            .map(|mut state| state.clear())
            .map_err(|_| FlowError::Failed);
        self.shared.close_output();
        sealed?;
        worker?;
        cleared?;
        Ok(())
    }
}
impl Shared {
    fn close_output(&self) {
        let output = self.output.lock().ok().and_then(|mut slot| slot.take());
        drop(output);
    }

    pub(crate) fn next_exchange(&self) -> Result<ExchangeKey, FlowError> {
        let sequence = self
            .next
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| FlowError::Stopped)?
            + 1;
        Ok(ExchangeKey {
            flow_instance: self.instance,
            sequence,
        })
    }
    fn source_closed(self: &Arc<Self>) {
        self.feedback_stop.cancel();
        if self
            .intake
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            let shared = self.clone();
            self.tasks.spawn(async move {
                let _ = shared
                    .publish(ChatEvent::Gap(ObservationGap {
                        kind: "capture".into(),
                        exchange: None,
                        cursor_range: None,
                        reason: "source_closed".into(),
                    }))
                    .await;
            });
        }
    }
    pub(crate) async fn publish(&self, event: ChatEvent) -> Result<(), FlowError> {
        let prepared = PreparedChatEvent::new(event)?;
        let sequence = self
            .event_sequence
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| FlowError::Stopped)?
            + 1;
        let envelope = prepared.into_envelope(ChatCursor {
            service_instance: self.instance,
            sequence,
        });
        let output = self
            .output
            .lock()
            .map_err(|_| FlowError::Failed)?
            .clone()
            .ok_or(FlowError::Stopped)?;
        let result = output.send(&envelope).await;
        if result.is_err() {
            self.lost.fetch_add(1, Ordering::Relaxed);
        }
        result
    }
    pub(crate) async fn credential_fact(
        &self,
        facts: &IngressFacts,
        id: CredentialUseId,
        binding: BindingId,
        origin: String,
        state: CredentialUseState,
        slot: AuthSlot,
    ) -> Result<(), FlowError> {
        let revision = match state {
            CredentialUseState::ScopeRejected => 1,
            CredentialUseState::Prepared => 1,
            CredentialUseState::AppliedToRequest => 2,
            CredentialUseState::Submitted => 3,
            CredentialUseState::Failed => 4,
        };
        self.publish(ChatEvent::CredentialUse(Arc::new(CredentialUseFact {
            id,
            revision,
            actor: facts.connection_initiator.clone(),
            binding_ref: Some(binding),
            destination_origin: origin,
            state,
            auth_field: Some(slot.header_name().as_str().to_owned()),
            reason: None,
            observed_boottime_ns: boottime(),
        })))
        .await
    }
}
impl FlowStatusPort for Shared {
    fn status(&self) -> FlowStatus {
        FlowStatus {
            intake_state: match self.intake.load(Ordering::Acquire) {
                0 => FlowIntakeState::Running,
                1 => FlowIntakeState::SourceClosed,
                _ => FlowIntakeState::Stopped,
            },
            mode: FlowMode::MediateTls,
            decoder_profiles: self
                .settings
                .provider_routes
                .iter()
                .map(|r| r.profile)
                .collect(),
            active_exchanges: (self.settings.decode_limits.max_active_exchanges as usize
                - self.exchanges.available_permits()) as u64,
            lost_deliveries: self.lost.load(Ordering::Relaxed),
            parse_failures: self.parse_failures.load(Ordering::Relaxed),
        }
    }
}
fn boottime() -> u64 {
    let t = rustix::time::clock_gettime(rustix::time::ClockId::Boottime);
    (t.tv_sec as u64)
        .saturating_mul(1_000_000_000)
        .saturating_add(t.tv_nsec as u64)
}
#[cfg(test)]
mod tests;
impl Drop for FlowModule {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.shared.feedback_stop.cancel();
        self.shared.force.cancel();
        let _ = self
            .shared
            .intake
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire);
        self.shared.close_output();
    }
}
