//! Independent chat SQL intake and retained bounded control output on the original message runtime.
use crate::{
    business_log::{BusinessLog, RecordOutcome, StoreError},
    transport,
};
use as_main::messages::{MainMessages, protocol};
use as_policy_engine::interaction::Prompt;
use as_types::main_ports::{MainMessage, MessageModule, ModuleError, NativeFact};
use futures::{FutureExt, future::BoxFuture};
use std::{
    io,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::sync::mpsc;

type IntakeOutcome = (bool, Result<RecordOutcome, StoreError>);

/// One message module: chat intake is independent of its retained control-notice output.
pub struct Media {
    business: BusinessLog,
    feedback: OnceLock<Option<Arc<dyn as_types::flow::FlowFeedbackPort>>>,
    feedback_stop: tokio_util::sync::CancellationToken,
    group: Option<as_types::configuration::GroupObservation>,
    peers: Mutex<Peers>,
    not_forwarded: AtomicU64,
    stored: AtomicU64,
}
struct Peers {
    admission_stop: tokio_util::sync::CancellationToken,
    connections: Vec<Arc<transport::Peer>>,
}
impl Media {
    /// Use the original business database owner, without a second writer or source queue.
    pub fn new(business: BusinessLog) -> Self {
        Self::with_group(business, None)
    }

    /// Bind the same frozen scope as the native source for this service lifetime.
    pub fn with_group(
        business: BusinessLog,
        group: Option<as_types::configuration::GroupObservation>,
    ) -> Self {
        Self {
            group,
            business,
            feedback: OnceLock::new(),
            feedback_stop: tokio_util::sync::CancellationToken::new(),
            peers: Mutex::new(Peers {
                admission_stop: tokio_util::sync::CancellationToken::new(),
                connections: Vec::new(),
            }),
            not_forwarded: AtomicU64::new(0),
            stored: AtomicU64::new(0),
        }
    }
    fn peers(&self) -> io::Result<Vec<Arc<transport::Peer>>> {
        self.peers
            .lock()
            .map(|peers| peers.connections.clone())
            .map_err(|_| io::Error::other("media state unavailable"))
    }

    /// Replace the single local management connection without changing WebSocket peers.
    pub fn set_peer(&self, peer: Option<Arc<transport::Peer>>) -> io::Result<()> {
        let mut peers = self
            .peers
            .lock()
            .map_err(|_| io::Error::other("media state unavailable"))?;
        if peer.is_some() && peers.admission_stop.is_cancelled() {
            return Err(io::Error::other("media is retiring"));
        }
        if let Some(index) = peers
            .connections
            .iter()
            .position(|existing| !existing.is_websocket())
        {
            let previous = peers.connections.remove(index);
            if !peer
                .as_ref()
                .is_some_and(|next| Arc::ptr_eq(next, &previous))
            {
                previous.close();
            }
        }
        peers.connections.extend(peer);
        Ok(())
    }

    /// Close the local management connection and release its prompt.
    pub fn close_peer(&self) -> io::Result<()> {
        self.set_peer(None)
    }

    /// Seal the existing registry and retain the same authorized writers for final cursor delivery.
    pub fn begin_chat_drain(&self) -> io::Result<Vec<Arc<transport::Peer>>> {
        let peers = self
            .peers
            .lock()
            .map_err(|_| io::Error::other("media state unavailable"))?;
        peers.admission_stop.cancel();
        for peer in &peers.connections {
            peer.begin_chat_drain();
        }
        Ok(peers.connections.clone())
    }

    /// Offer the original prompt once to a ready connection.
    pub fn prompt(&self, mut prompt: Prompt) -> io::Result<()> {
        for peer in self.peers()? {
            let Some(returned) = peer.try_prompt(prompt) else {
                return Ok(());
            };
            prompt = returned;
        }
        Ok(())
    }

    /// Bind an authenticated WebSocket to this instance's shared message consumer.
    pub async fn serve_websocket(
        &self,
        stream: tokio::net::TcpStream,
        origin: Option<String>,
        messages: Arc<MainMessages>,
    ) -> Result<(), transport::TransportError> {
        let admission_stop = self
            .peers
            .lock()
            .map_err(|_| transport::TransportError::PromptStateUnavailable)?
            .admission_stop
            .clone();
        let authenticating = transport::WebSocketEndpoint::authenticate(
            stream,
            origin,
            messages,
            self.business.clone(),
        );
        let (peer, session) = tokio::select! {biased;
            _=admission_stop.cancelled()=>return Err(transport::TransportError::OutboundClosed),
            result=authenticating=>result?,
        };
        {
            let mut peers = self
                .peers
                .lock()
                .map_err(|_| transport::TransportError::PromptStateUnavailable)?;
            if peers.admission_stop.is_cancelled() {
                return Err(transport::TransportError::OutboundClosed);
            }
            peers.connections.push(peer.clone());
        }
        let _registration = Registration {
            media: self,
            peer: peer.clone(),
        };
        session.await
    }

    /// Control notifications not admitted to a peer queue; chat is read from SQL, not counted here.
    pub fn not_forwarded(&self) -> u64 {
        self.not_forwarded.load(Ordering::Relaxed)
    }
    /// Original business inputs for which SQLite confirmed a commit.
    pub fn stored_records(&self) -> u64 {
        self.stored.load(Ordering::Relaxed)
    }
}
struct Registration<'a> {
    media: &'a Media,
    peer: Arc<transport::Peer>,
}

impl Drop for Registration<'_> {
    fn drop(&mut self) {
        self.peer.close();
        if let Ok(mut peers) = self.media.peers.lock() {
            peers
                .connections
                .retain(|peer| !Arc::ptr_eq(peer, &self.peer));
        }
    }
}

#[async_trait::async_trait]
impl MessageModule for Media {
    async fn consume_group(
        &self,
        mut events: mpsc::Receiver<as_types::driver::NativeGroupTcp>,
        boot: [u8; 16],
    ) -> Result<(), ModuleError> {
        let mut group =
            crate::agent_graph::Collector::new(self.group.ok_or(ModuleError::InvalidInput)?);
        while let Some(fact) = events.recv().await {
            if let Err(error) = group.consume(&self.business, fact, boot).await {
                as_types::diagnostics::record_stderr(format!("AS_GROUP_STORE_FAILED code={error}"))
                    .await;
                return Err(ModuleError::Unavailable);
            }
            if events.is_empty()
                && let Err(error) = group.flush_unframed(&self.business, boot).await
            {
                as_types::diagnostics::record_stderr(format!("AS_GROUP_STORE_FAILED code={error}"))
                    .await;
                return Err(ModuleError::Unavailable);
            }
        }
        if let Err(error) = group.finish(&self.business, boot).await {
            as_types::diagnostics::record_stderr(format!("AS_GROUP_STORE_FAILED code={error}"))
                .await;
            return Err(ModuleError::Unavailable);
        }
        group.report().await;
        Ok(())
    }

    async fn consume_chat(
        &self,
        mut events: as_types::flow::ChatReceiver,
        feedback: Option<Arc<dyn as_types::flow::FlowFeedbackPort>>,
    ) -> Result<(), ModuleError> {
        self.feedback
            .set(feedback)
            .map_err(|_| ModuleError::Conflict)?;
        let _lifetime = self.feedback_stop.clone().drop_guard();
        while let Some(envelope) = events.recv().await {
            if let Err(error) = self.business.record_chat(envelope).await {
                as_types::diagnostics::record_stderr(format!(
                    "AS_BUSINESS_STORE_FAILED code={error} lane=chat source={error:?}"
                ))
                .await;
                return Err(ModuleError::Unavailable);
            }
            self.stored.fetch_add(1, Ordering::Relaxed);
        }
        Ok(())
    }
    async fn consume_native_fact(&self, fact: NativeFact) -> Result<(), ModuleError> {
        let stored = match fact {
            NativeFact::RequestHandoff(fact) => self.business.record_request_handoff(fact).await,
            NativeFact::RequestReturn(fact) => self.business.record_request_return(fact).await,
            NativeFact::FutexSelection(fact) => self.business.record_futex_selection(fact).await,
        };
        if let Err(error) = stored {
            as_types::diagnostics::record_stderr(format!(
                "AS_BUSINESS_STORE_FAILED code={error} lane=native_facts source={error:?}"
            ))
            .await;
            return Err(ModuleError::Unavailable);
        }
        self.stored.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    async fn run(&self, mut notices: mpsc::Receiver<MainMessage>) -> Result<(), ModuleError> {
        let mut storing: Option<BoxFuture<'static, IntakeOutcome>> = None;
        loop {
            tokio::select! {biased;
                accepted=async{match storing.as_mut(){Some(work)=>work.await,None=>std::future::pending().await}},if storing.is_some()=>{
                    storing=None;
                    let(not_queued,stored)=accepted;
                    if not_queued{self.not_forwarded.fetch_add(1,Ordering::Relaxed);}
                    match stored{Ok(RecordOutcome::Stored(_))=>{self.stored.fetch_add(1,Ordering::Relaxed);},Ok(RecordOutcome::NotApplicable)=>{},Err(error)=>{as_types::diagnostics::record_stderr(format!("AS_BUSINESS_STORE_FAILED code={error} lane=notices source={error:?}")).await;return Err(ModuleError::Unavailable);}}
                },
                Some(notice)=notices.recv(),if storing.is_none()=>{
                    let peers=self.peers().map_err(|_|ModuleError::Unavailable)?;
                    let business=self.business.clone();
                    let feedback=self.feedback.get().and_then(Clone::clone);
                    let cancelled=self.feedback_stop.child_token();
                    // Retain this control notice through its SQL and bounded
                    // output admission. Independent chat intake does not wait here.
                    storing=Some(async move {match notice {
                        MainMessage::Completed(notice)=>{
                            let notice:Arc<as_types::main_ports::MainNotice>=Arc::from(notice);
                            let Some(protocol::Up::OperationResult(result))=protocol::operation_result(&notice) else {
                                return (false,Ok(RecordOutcome::NotApplicable));
                            };
                            let mut stored=business.record_completion(&notice, result.clone()).await;
                            if stored.is_ok() && let Some(executor)=feedback {
                                match crate::execution_feedback::execute_feedback(executor.as_ref(), &business, notice, &result, cancelled).await {
                                    Ok(crate::execution_feedback::FeedbackExecution::NotApplicable)=>{},
                                    Ok(result)=>tracing::debug!(?result,"AS rule feedback preparation"),
                                    Err(error)=>stored=Err(error),
                                }
                            }
                            let not_queued=broadcast(&peers, Arc::new(protocol::Up::OperationResult(result))).await;
                            (not_queued,stored)
                        },
                        MainMessage::DriverStatus(status)=>(broadcast(&peers, Arc::new(protocol::driver_status(status))).await,Ok(RecordOutcome::NotApplicable)),
                        MainMessage::DriverUnavailable=>{
                            let not_forwarded=futures::future::join_all(peers.iter().map(|peer|peer.source_unavailable())).await.iter().all(Result::is_err);
                            as_types::diagnostics::record_stdout("AS_SCRIPTED_SOURCE_UNAVAILABLE".into()).await;
                            (not_forwarded,Ok(RecordOutcome::NotApplicable))
                        },

                    }}.boxed());
                },
                else=>return Ok(()),
            }
        }
    }
}

async fn broadcast(peers: &[Arc<transport::Peer>], notice: Arc<protocol::Up>) -> bool {
    let results = futures::future::join_all(
        peers
            .iter()
            .filter(|peer| peer.accepts_notice(&notice))
            .map(|peer| peer.notice(notice.clone())),
    )
    .await;
    results.iter().all(Result::is_err)
}
