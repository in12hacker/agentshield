//! Shared Unix/WS session, subscription and reply writer over original Main ports.
use crate::{
    business::{self, BusinessData, BusinessQuery, BusinessResult},
    business_log::BusinessLog,
    codec::*,
};
use as_main::messages::{CommandReply, MainMessages, connections::Connection, protocol::*};
use as_policy_engine::interaction::Prompt;
use as_types::{configuration::Access, flow::*, main_ports::ModuleError};
use futures::{FutureExt, StreamExt, future::BoxFuture, stream::FuturesUnordered};
use serde::{Deserialize, Serialize};
use std::{
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio::{
    net::{UnixListener, UnixStream},
    sync::{Notify, mpsc},
};
use tokio_util::sync::CancellationToken;
use zeroize::{Zeroize, Zeroizing};

mod management;
mod websocket;
pub use websocket::WebSocketEndpoint;
use websocket::{WsReader, WsText, WsWriter};

const QUEUE: usize = 16;
const INFLIGHT: usize = 16;
type PendingReplies = FuturesUnordered<BoxFuture<'static, Output>>;

/// Finite diagnostics never contain the rejected frame, key, token or parser error text.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    /// The peer ended a partial or oversized frame, or supplied a different envelope.
    #[error("invalid_frame")]
    InvalidFrame,
    /// The actual socket writer has stopped accepting output.
    #[error("outbound_closed")]
    OutboundClosed,
    /// The local prompt slot cannot be accessed.
    #[error("prompt_state_unavailable")]
    PromptStateUnavailable,
    /// This thin scripted host supports only its own OS uid as middleware peer.
    #[error("unsupported_peer_uid")]
    UnsupportedPeer,
    /// An actual output-permission check failed at the stated writer phase.
    #[error("output_authorization_failed grant={grant} stage={stage} reason={reason:?}")]
    OutputAuthorization {
        /// The actual permission category selected by the original output.
        grant: FrameGrant,
        /// The phase at which the real check ran.
        stage: OutputCheckStage,
        /// The original module error, without inferring a cause from a notification.
        #[source]
        reason: ModuleError,
    },
    /// The actual revocation watcher ended, but a fresh permission check still succeeded.
    #[error("output_authorization_watch_ended grant={grant} stage=revoked_watch")]
    AuthorizationWatchEnded {
        /// The category actually rechecked when the watcher ended.
        grant: FrameGrant,
    },
    /// WebSocket handshake, frame or I/O failure.
    #[error("websocket_io")]
    WebSocket,
    /// WebSocket authentication was not completed.
    #[error("websocket_authentication")]
    Authentication,
    /// The active chat subscription could not finish its committed tail.
    #[error("chat_drain_incomplete")]
    ChatDrain,
    /// Real socket operation failed.
    #[error("socket_io")]
    Io(#[from] io::Error),
}

/// Own only the socket inode actually created by this listener.
pub struct Endpoint {
    listener: UnixListener,
    path: PathBuf,
    device: u64,
    inode: u64,
}
impl Endpoint {
    /// Bind configuration's fixed endpoint, refusing an existing object or unsafe parent.
    pub fn bind(path: &Path, peer_uid: u32) -> Result<Self, TransportError> {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if peer_uid != rustix::process::geteuid().as_raw() {
            return Err(TransportError::UnsupportedPeer);
        }
        let parent = path.parent().ok_or(TransportError::InvalidFrame)?;
        match std::fs::create_dir(parent) {
            Ok(()) => std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o711))?,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }
        let metadata = std::fs::symlink_metadata(parent)?;
        if !metadata.is_dir()
            || metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.mode() & 0o022 != 0
        {
            return Err(TransportError::InvalidFrame);
        }
        let listener = UnixListener::bind(path)?;
        let metadata = std::fs::symlink_metadata(path)?;
        let endpoint = Self {
            listener,
            path: path.into(),
            device: metadata.dev(),
            inode: metadata.ino(),
        };
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        Ok(endpoint)
    }
    /// Accept a transport; MainMessages::connect must authenticate it before decoding anything.
    pub async fn accept(&self) -> Result<UnixStream, TransportError> {
        Ok(self.listener.accept().await?.0)
    }
}
impl Drop for Endpoint {
    fn drop(&mut self) {
        use std::os::unix::fs::{FileTypeExt, MetadataExt};
        if let Ok(m) = std::fs::symlink_metadata(&self.path)
            && m.file_type().is_socket()
            && m.dev() == self.device
            && m.ino() == self.inode
        {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

enum ServerEvent {
    Notice(Arc<Up>),
    SourceUnavailable,
}
struct ChatSubscription {
    id: CommandId,
    start_after: Option<ChatCursor>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PeerPhase {
    Live,
    Draining,
    Finished(Option<ChatCursor>),
}
/// Host handle has no Connection or grant ownership; OS EOF drops the actual Connection.
pub struct Peer {
    events: mpsc::Sender<ServerEvent>,
    prompt: Mutex<Option<Prompt>>,
    output_changed: Notify,
    stop: CancellationToken,
    review_ready: Box<dyn Fn() -> bool + Send + Sync>,
    websocket: bool,
    subscription: Mutex<Option<Arc<ChatSubscription>>>,
    phase: tokio::sync::watch::Sender<PeerPhase>,
}
impl Peer {
    pub(crate) fn is_websocket(&self) -> bool {
        self.websocket
    }

    pub(crate) fn accepts_notice(&self, notice: &Up) -> bool {
        !self.websocket
            || (self.ready()
                && matches!(notice, Up::OperationResult(result) if result.decision_id.is_some()))
    }
    /// Stop this transport without stopping AS or its driver.
    pub fn close(&self) {
        self.stop.cancel();
        self.retire_prompt();
    }

    /// Stop admitting application work while keeping this subscriber's authorized writer alive.
    pub fn begin_chat_drain(&self) {
        self.phase.send_if_modified(|phase| {
            if *phase != PeerPhase::Live {
                return false;
            }
            *phase = PeerPhase::Draining;
            true
        });
        self.retire_prompt();
    }

    /// Supply the final SQLite cursor after the source and its consumer have joined.
    pub fn finish_chat_drain(&self, tail: Option<ChatCursor>) {
        self.phase.send_replace(PeerPhase::Finished(tail));
    }

    fn check_chat_drain(
        &self,
        end: &Result<SessionEnd, TransportError>,
    ) -> Result<(), TransportError> {
        if !matches!(end, Ok(SessionEnd::Drained))
            && *self.phase.borrow() != PeerPhase::Live
            && current_subscription(self)?.is_some()
        {
            return Err(TransportError::ChatDrain);
        }
        Ok(())
    }

    fn retire_prompt(&self) {
        if let Ok(mut prompt) = self.prompt.lock() {
            *prompt = None;
        }
    }
    /// Actual ready declaration accepted on this transport, never a peer-supplied role.
    pub fn ready(&self) -> bool {
        !self.stop.is_cancelled()
            && *self.phase.borrow() == PeerPhase::Live
            && (self.review_ready)()
    }
    /// Await the existing control queue's admission; saturation does not retire this identity.
    /// Success is queue admission, not socket delivery.
    pub async fn notice(&self, notice: Arc<Up>) -> Result<(), TransportError> {
        self.offer(ServerEvent::Notice(notice)).await
    }
    /// Admit the original Prompt once, or return it unchanged before this peer takes ownership.
    pub fn try_prompt(&self, prompt: Prompt) -> Option<Prompt> {
        // The scheduler has only one active original question. A separate slot
        // prevents ordinary completion traffic from displacing that question.
        let Ok(mut slot) = self.prompt.lock() else {
            return Some(prompt);
        };
        if !self.ready()
            || slot.as_ref().is_some_and(|old| {
                !old.reply.is_closed() && old.deadline > tokio::time::Instant::now()
            })
        {
            return Some(prompt);
        }
        *slot = Some(prompt);
        self.output_changed.notify_one();
        None
    }
    fn take_prompt(&self) -> Result<Option<Prompt>, TransportError> {
        Ok(self
            .prompt
            .lock()
            .map_err(|_| TransportError::PromptStateUnavailable)?
            .take())
    }
    /// Preserve a real source-disconnect observation independently of DriverStatus.
    pub async fn source_unavailable(&self) -> Result<(), TransportError> {
        self.offer(ServerEvent::SourceUnavailable).await
    }
    async fn offer(&self, event: ServerEvent) -> Result<(), TransportError> {
        let mut phase = self.phase.subscribe();
        tokio::select! {biased;
            _=self.stop.cancelled()=>Err(TransportError::OutboundClosed),
            _=phase.wait_for(|phase| *phase != PeerPhase::Live)=>Err(TransportError::OutboundClosed),
            accepted=self.events.send(event)=>accepted.map_err(|_|TransportError::OutboundClosed),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BusinessRequest {
    id: CommandId,
    query: BusinessQuery,
}
#[derive(Serialize)]
struct BusinessReply {
    id: CommandId,
    result: Box<BusinessResult>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StreamRequest {
    id: CommandId,
    command: StreamCommand,
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum StreamCommand {
    Subscribe {
        filter: ChatFilter,
        after: Option<ChatCursor>,
    },
    Unsubscribe {
        subscription_id: CommandId,
    },
}
#[derive(Serialize)]
struct StreamReply {
    id: CommandId,
    result: StreamResult,
}
#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum StreamResult {
    Subscribed { subscription_id: CommandId },
    Unsubscribed { subscription_id: CommandId },
    Failed { code: &'static str },
}
fn stream_store_error(error: crate::business_log::StoreError) -> &'static str {
    use crate::business_log::StoreError;
    match error {
        StoreError::NotProvided => "not_found",
        StoreError::Closed => "stopped",
        StoreError::Invalid | StoreError::Conflict => "invalid",
        _ => "unavailable",
    }
}

// Private payloads have no Debug or Serialize. Parse only after ManageService succeeds.
struct SecretInput(PrivateBytes);
impl<'de> Deserialize<'de> for SecretInput {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut value = String::deserialize(d)?;
        if value.len() > MAX_SECRET_BYTES {
            value.zeroize();
            return Err(serde::de::Error::custom("secret_too_large"));
        }
        Ok(Self(PrivateBytes::new(value.into_bytes())))
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PrivateRequest {
    id: CommandId,
    command: PrivateCommand,
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum PrivateCommand {
    EnsureCa,
    PublicCa {
        ca: CaRef,
    },
    ImportSecret {
        value: SecretInput,
    },
    ReplaceSecret {
        secret: SecretRef,
        #[serde(with = "as_types::decimal")]
        expected_revision: u64,
        value: SecretInput,
    },
    IssuePhantom {
        binding: BindingId,
        expected_config: as_types::configuration::ConfigVersion,
    },
    RevokePhantom {
        token: PhantomTokenId,
    },
}
enum PrivateData {
    Ca(CaPublicInfo),
    Saved(SecretWriteReceipt),
    Issued(PhantomIssue),
    Revoked(u64),
}
struct PrivateReply {
    id: CommandId,
    result: Result<PrivateData, CredentialError>,
}

enum Output {
    // The write capability is linear; only the immutable wire message is shared.
    Ordinary(
        Arc<Up>,
        Option<tokio::time::Instant>,
        Option<as_main::runtime::StopReplyWrite>,
    ),
    Stream(StreamReply),
    Business(BusinessReply),
    Private(PrivateReply),
    Diagnostic(Diagnostic),
}

#[derive(Serialize)]
struct Diagnostic {
    #[serde(rename = "type")]
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<CommandId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<&'static str>,
}
fn diagnostic(kind: &'static str) -> Output {
    Output::Diagnostic(Diagnostic {
        kind,
        id: None,
        code: None,
    })
}
fn rejected_diagnostic(kind: &'static str, code: &'static str) -> Output {
    Output::Diagnostic(Diagnostic {
        kind,
        id: None,
        code: Some(code),
    })
}

fn reply_unavailable(id: Option<CommandId>, code: &'static str) -> Output {
    Output::Diagnostic(Diagnostic {
        kind: "reply_output_unavailable",
        id,
        code: Some(code),
    })
}

/// Start exactly one authenticated middleware session; call the returned future on a live runtime.
pub fn session(
    messages: Arc<MainMessages>,
    connection: Connection,
    business_log: BusinessLog,
) -> (
    Arc<Peer>,
    impl std::future::Future<Output = Result<(), TransportError>> + Send,
) {
    session_on(messages, connection, business_log, None)
}

fn session_on(
    messages: Arc<MainMessages>,
    connection: Connection,
    business_log: BusinessLog,
    websocket: Option<(WsReader, WsWriter)>,
) -> (
    Arc<Peer>,
    impl std::future::Future<Output = Result<(), TransportError>> + Send,
) {
    let (send, receive) = mpsc::channel(QUEUE);
    let peer = Arc::new(Peer {
        events: send,
        prompt: Mutex::new(None),
        output_changed: Notify::new(),
        stop: CancellationToken::new(),
        review_ready: Box::new(connection.review_readiness()),
        websocket: websocket.is_some(),
        subscription: Mutex::new(None),
        phase: tokio::sync::watch::Sender::new(PeerPhase::Live),
    });
    let owner = peer.clone();
    (peer, async move {
        let result = run(
            messages,
            connection,
            receive,
            owner.clone(),
            business_log,
            websocket,
        )
        .await;
        owner.close();
        result
    })
}

enum FrameSource<'a> {
    Local(Lines, &'a UnixStream),
    WebSocket(WsReader),
}
enum InputFrame {
    Local(Zeroizing<Vec<u8>>),
    WebSocket(WsText),
    Close,
}

enum SessionEnd {
    Closed,
    PeerClose,
    Drained,
}

impl FrameSource<'_> {
    async fn next(&mut self) -> Result<Option<InputFrame>, TransportError> {
        match self {
            Self::Local(lines, stream) => lines
                .next(stream)
                .await
                .map(|line| line.map(InputFrame::Local)),
            Self::WebSocket(stream) => websocket::receive(stream).await,
        }
    }
}
enum FrameSink<'a> {
    Local(&'a UnixStream),
    WebSocket(WsWriter),
}

struct Lines {
    pending: Zeroizing<Vec<u8>>,
}
impl Lines {
    async fn next(
        &mut self,
        socket: &UnixStream,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, TransportError> {
        loop {
            if let Some(end) = self.pending.iter().position(|v| *v == b'\n') {
                if end > MAX_FRAME_BYTES {
                    return Err(TransportError::InvalidFrame);
                }
                let frame = Zeroizing::new(self.pending[..end].to_vec());
                let rest = Zeroizing::new(self.pending[end + 1..].to_vec());
                self.pending.zeroize();
                self.pending = rest;
                return Ok(Some(frame));
            }
            if self.pending.len() > MAX_FRAME_BYTES {
                return Err(TransportError::InvalidFrame);
            }
            socket.readable().await?;
            let mut bytes = Zeroizing::new([0u8; 8192]);
            match socket.try_read(&mut bytes[..]) {
                Ok(0) => {
                    return if self.pending.is_empty() {
                        Ok(None)
                    } else {
                        Err(TransportError::InvalidFrame)
                    };
                }
                Ok(n) => self.pending.extend_from_slice(&bytes[..n]),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(e.into()),
            }
        }
    }
}
async fn enqueue(sender: &mpsc::Sender<Output>, output: Output) -> Result<(), TransportError> {
    sender
        .send(output)
        .await
        .map_err(|_| TransportError::OutboundClosed)?;
    // The producer and socket writer are driven by the same owning task.
    // Let the writer run after a successful offer instead of filling the whole
    // queue within one producer poll and blaming a peer that could not be polled.
    tokio::task::yield_now().await;
    Ok(())
}
async fn run(
    messages: Arc<MainMessages>,
    connection: Connection,
    events: mpsc::Receiver<ServerEvent>,
    peer: Arc<Peer>,
    business_log: BusinessLog,
    websocket: Option<(WsReader, WsWriter)>,
) -> Result<(), TransportError> {
    let (mut reader, mut sink) = match websocket {
        Some((reader, writer)) => (FrameSource::WebSocket(reader), FrameSink::WebSocket(writer)),
        None => {
            let stream = connection.stream().ok_or(TransportError::InvalidFrame)?;
            (
                FrameSource::Local(
                    Lines {
                        pending: Zeroizing::new(Vec::new()),
                    },
                    stream,
                ),
                FrameSink::Local(stream),
            )
        }
    };
    let end = {
        let (control, control_rx) = mpsc::channel(QUEUE);
        let work = async {
            let mut phase = peer.phase.subscribe();
            let mut pending = PendingReplies::new();
            loop {
                if peer.stop.is_cancelled() {
                    return Ok(SessionEnd::Closed);
                }
                let live = *phase.borrow_and_update() == PeerPhase::Live;
                if matches!(*phase.borrow(), PeerPhase::Finished(_)) && pending.is_empty() {
                    // Every admitted command has handed its actual result to this queue.
                    // Close its sender, then keep reading the same transport while the writer drains.
                    drop(control);
                    loop {
                        tokio::select! {biased;
                            _=peer.stop.cancelled()=>return Ok(SessionEnd::Closed),
                            frame=reader.next()=>match frame? {
                                Some(InputFrame::Close)=>return Ok(SessionEnd::PeerClose),
                                None=>return Ok(SessionEnd::Closed),
                                _=>{},
                            },
                        }
                    }
                }
                tokio::select! {
                    _=peer.stop.cancelled()=>return Ok(SessionEnd::Closed),
                    _=phase.changed()=>continue,
                    frame=reader.next(),if live || peer.websocket=>{
                        let frame = frame?;
                        let bytes = match &frame {
                            Some(InputFrame::Local(bytes)) => bytes.as_slice(),
                            Some(InputFrame::WebSocket(text)) => text.as_bytes(),
                            Some(InputFrame::Close) => return Ok(SessionEnd::PeerClose),
                            None => return Ok(SessionEnd::Closed),
                        };
                        if *phase.borrow() != PeerPhase::Live { continue; }
                        let input = decode_incoming(bytes).map_err(|_| TransportError::InvalidFrame)?;
                        let management_request = input.message.map(management::Request::Message)
                            .or_else(|| input.private.map(management::Request::Private));
                        if let Some(request) = management_request {
                            let output = management::admit(request, &messages, &connection, &mut pending)?;
                            drop(frame);
                            if let Some(output) = output {
                                enqueue(&control, output).await?;
                            }
                        } else if let Some(raw)=input.stream {
                            let request:StreamRequest=serde_json::from_str(raw.get()).map_err(|_|TransportError::InvalidFrame)?;
                            let access=match messages.content_access(&connection){Ok(access)=>access,Err(error)=>{enqueue(&control,Output::Stream(StreamReply{id:request.id,result:StreamResult::Failed{code:module_code(error)}})).await?;continue;}};
                            match request.command {
                                StreamCommand::Subscribe{filter,after}=>{
                                    if filter!=ChatFilter::default(){enqueue(&control,Output::Stream(StreamReply{id:request.id,result:StreamResult::Failed{code:"unsupported"}})).await?;continue;}
                                    if pending.len()>=INFLIGHT{enqueue(&control,Output::Stream(StreamReply{id:request.id,result:StreamResult::Failed{code:"busy"}})).await?;continue;}
                                    let source=business_log.clone();let subscribing=peer.clone();
                                    pending.push(async move {
                                        let prepared=async {
                                            let start_after=match after{Some(cursor)=>{source.validate_chat_cursor(cursor).await.map_err(stream_store_error)?;Some(cursor)},None=>source.chat_tail().await.map_err(stream_store_error)?};
                                            access.authorize().map_err(module_code)?;
                                            if subscribing.stop.is_cancelled(){return Err("stopped");}
                                            let mut slot=subscribing.subscription.lock().map_err(|_|"unavailable")?;
                                            if slot.is_some(){return Err("busy");}
                                            *slot=Some(Arc::new(ChatSubscription{id:request.id,start_after}));
                                            subscribing.output_changed.notify_one();
                                            Ok(())
                                        }.await;
                                        Output::Stream(StreamReply{id:request.id,result:match prepared{Ok(())=>StreamResult::Subscribed{subscription_id:request.id},Err(code)=>StreamResult::Failed{code}}})
                                    }.boxed());
                                }
                                StreamCommand::Unsubscribe{subscription_id}=>{
                                    let result={let mut slot=peer.subscription.lock().map_err(|_|TransportError::PromptStateUnavailable)?;
                                        if slot.as_ref().is_some_and(|s|s.id==subscription_id){*slot=None;peer.output_changed.notify_one();StreamResult::Unsubscribed{subscription_id}}else{StreamResult::Failed{code:"not_found"}}
                                    };
                                    enqueue(&control,Output::Stream(StreamReply{id:request.id,result})).await?;
                                }
                            }
                        } else if let Some(raw)=input.business {
                            let request:BusinessRequest=serde_json::from_str(raw.get()).map_err(|_|TransportError::InvalidFrame)?;
                            if pending.len()>=INFLIGHT{enqueue(&control,Output::Business(BusinessReply{id:request.id,result:Box::new(BusinessResult::Failed{code:FailureCode::Busy,message:"business query busy"})})).await?;continue;}
                            let query=business::start_query(&messages,&connection,&business_log,request.query);
                            pending.push(async move{Output::Business(BusinessReply{id:request.id,result:Box::new(query.await)})}.boxed());

                        }
                    },
                    result=async {
                        let permit=control.reserve().await.map_err(|_|TransportError::OutboundClosed)?;
                        if let Some(output)=pending.next().await{permit.send(output);}
                        Ok::<_,TransportError>(())
                    },if !pending.is_empty()=>result?,


                }
            }
        };
        tokio::pin!(work);
        let writing = writer(
            &messages,
            &connection,
            control_rx,
            events,
            &peer,
            &business_log,
            &mut sink,
        );
        tokio::pin!(writing);
        tokio::select! {biased;
            _=connection.revoked(), if peer.websocket => Ok(SessionEnd::Closed),
            result=&mut work=>result,
            result=&mut writing=>result,
        }
    };
    let drained = peer.check_chat_drain(&end);
    let closed = match (end, reader, sink) {
        (Ok(end), FrameSource::WebSocket(mut reader), FrameSink::WebSocket(mut sink)) => {
            // Release the original session before any final protocol-only write.
            peer.retire_prompt();
            drop(connection);
            match end {
                SessionEnd::PeerClose => websocket::finish_close(&mut sink, &peer.stop).await,
                SessionEnd::Drained => {
                    websocket::close_drained(&mut reader, &mut sink, &peer.stop).await
                }
                SessionEnd::Closed => Ok(()),
            }
        }
        (end, _, _) => end.map(|_| ()),
    };
    if matches!(drained, Err(TransportError::ChatDrain))
        && let Err(error) = &closed
        && !matches!(error, TransportError::ChatDrain)
    {
        as_types::diagnostics::record_stderr(format!("AS_CHAT_DRAIN_FAILED code={error}")).await;
    }
    drained.and(closed)
}

fn ordinary(up: Up, deadline: Option<tokio::time::Instant>) -> Output {
    Output::Ordinary(Arc::new(up), deadline, None)
}

fn received_reply(reply: CommandReply) -> Output {
    Output::Ordinary(Arc::new(reply.message), None, reply.stop_reply_write)
}

fn module_code(error: ModuleError) -> &'static str {
    match error {
        ModuleError::Forbidden => "forbidden",
        ModuleError::Unauthenticated => "unauthenticated",
        ModuleError::Busy => "busy",
        ModuleError::Stopped => "stopped",
        ModuleError::InvalidInput => "invalid",
        ModuleError::Unsupported => "unsupported",
        ModuleError::Conflict => "conflict",
        ModuleError::Unavailable => "unavailable",
    }
}
fn rejected(id: CommandId, error: ModuleError) -> Up {
    Up::Reply {
        id,
        result: CommandResult::Failed {
            code: FailureCode::from(error),
            message: module_code(error).into(),
            facts: None,
        },
    }
}

/// Output-permission diagnostic category; this value grants no authorization capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameGrant {
    /// Ordinary output. WebSocket writes still require the original live read grant.
    Public,
    /// The original connection's permission to receive content.
    Content,
    /// The original connection's permission and actual ready declaration for questions.
    Review,
    /// The original connection's service-management permission.
    Admin,
}

impl std::fmt::Display for FrameGrant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Public => "public",
            Self::Content => "content",
            Self::Review => "review",
            Self::Admin => "admin",
        })
    }
}

/// Actual writer phase of a permission check; never a guessed reason for revocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputCheckStage {
    /// Immediately before admitting the current SQL content read.
    BeforeChatRead,
    /// After that original SQL read returns, before adopting its result.
    AfterChatRead,
    /// Before encoding the original output.
    BeforeEncode,
    /// After encoding, before classifying its result or writing any bytes.
    AfterEncode,
    /// Before waiting for writability in the current frame.
    BeforeWrite,
    /// After writable readiness completes, immediately before the actual write attempt.
    WriteReady,
    /// A fresh check after the original revocation watcher ended.
    RevokedWatch,
}

impl std::fmt::Display for OutputCheckStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::BeforeChatRead => "before_chat_read",
            Self::AfterChatRead => "after_chat_read",
            Self::BeforeEncode => "before_encode",
            Self::AfterEncode => "after_encode",
            Self::BeforeWrite => "before_write",
            Self::WriteReady => "write_ready",
            Self::RevokedWatch => "revoked_watch",
        })
    }
}
impl FrameGrant {
    fn for_output(output: &Output) -> Self {
        match output {
            Output::Private(_) => Self::Admin,
            Output::Business(reply) => match reply.result.as_ref() {
                BusinessResult::Completed {
                    data: BusinessData::ProtectionOverview { .. },
                } => Self::Public,
                BusinessResult::Completed { .. } => Self::Content,
                BusinessResult::Failed { .. } => Self::Public,
            },
            Output::Ordinary(up, _, _) => match up.as_ref() {
                Up::Chat { .. } => Self::Content,
                Up::Ask { .. } => Self::Review,
                Up::Reply {
                    result:
                        CommandResult::Completed {
                            data: ReplyData::SecurityLog { .. },
                        },
                    ..
                } => Self::Admin,
                _ => Self::Public,
            },
            _ => Self::Public,
        }
    }
    fn check(
        self,
        connection: &Connection,
        stage: OutputCheckStage,
        websocket: bool,
    ) -> Result<Self, TransportError> {
        let checked = match self {
            Self::Public if websocket => connection.authorize(Access::ReadOnly),
            Self::Public => Ok(()),
            Self::Content => connection.authorize_content(),
            Self::Review => connection.review_ready(),
            Self::Admin => connection.authorize(Access::ManageService),
        };
        checked
            .map(|()| self)
            .map_err(|reason| TransportError::OutputAuthorization {
                grant: self,
                stage,
                reason,
            })
    }
}

#[derive(Debug, PartialEq, Eq)]
enum FrameWrite {
    Written,
    ExpiredUnwritten,
    RejectedUnwritten(OutputRecipient, OutputRejection),
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputRecipient {
    Chat(CommandId),
    Business(CommandId),
    Command(CommandId),
}

impl OutputRecipient {
    fn for_output(output: &Output) -> Option<Self> {
        match output {
            Output::Ordinary(up, _, _) => match up.as_ref() {
                Up::Chat {
                    subscription_id, ..
                } => Some(Self::Chat(*subscription_id)),
                Up::Reply { id, .. } => Some(Self::Command(*id)),
                _ => None,
            },
            Output::Business(reply) => Some(Self::Business(reply.id)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputRejection {
    TooLarge,
    EncodingFailed,
}

impl OutputRejection {
    fn code(self) -> &'static str {
        match self {
            Self::TooLarge => "output_too_large",
            Self::EncodingFailed => "output_encoding_failed",
        }
    }
}

fn prepare_encoding(
    target: Option<OutputRecipient>,
    encoded: Result<Zeroizing<Vec<u8>>, TransportError>,
) -> Result<Result<Zeroizing<Vec<u8>>, FrameWrite>, TransportError> {
    let rejected = |reason| match target {
        Some(target) => Ok(Err(FrameWrite::RejectedUnwritten(target, reason))),
        None => Err(TransportError::InvalidFrame),
    };
    match encoded {
        Ok(bytes) if bytes.len() <= MAX_FRAME_BYTES => Ok(Ok(bytes)),
        Ok(_) => rejected(OutputRejection::TooLarge),
        Err(TransportError::InvalidFrame) if target.is_some() => {
            rejected(OutputRejection::EncodingFailed)
        }
        Err(error) => Err(error),
    }
}

fn subscription_failure(
    peer: &Peer,
    owner: &Arc<ChatSubscription>,
    code: &'static str,
) -> Result<Option<Output>, TransportError> {
    let mut slot = peer
        .subscription
        .lock()
        .map_err(|_| TransportError::PromptStateUnavailable)?;
    if !slot
        .as_ref()
        .is_some_and(|active| Arc::ptr_eq(active, owner))
    {
        return Ok(None);
    }
    *slot = None;
    peer.output_changed.notify_one();
    Ok(Some(Output::Stream(StreamReply {
        id: owner.id,
        result: StreamResult::Failed { code },
    })))
}

async fn write_control_frame(
    messages: &MainMessages,
    connection: &Connection,
    output: Output,
    stop: &CancellationToken,
    sink: &mut FrameSink<'_>,
) -> Result<FrameWrite, TransportError> {
    match write_frame(messages, connection, output, stop, sink).await? {
        FrameWrite::RejectedUnwritten(recipient, reason) => {
            let failure = rejected_control_output(recipient, reason)?;
            match write_frame(messages, connection, failure, stop, sink).await? {
                FrameWrite::RejectedUnwritten(_, _) => Err(TransportError::InvalidFrame),
                outcome => Ok(outcome),
            }
        }
        outcome => Ok(outcome),
    }
}

fn rejected_control_output(
    recipient: OutputRecipient,
    reason: OutputRejection,
) -> Result<Output, TransportError> {
    match recipient {
        OutputRecipient::Business(id) => Ok(Output::Business(BusinessReply {
            id,
            result: Box::new(BusinessResult::Failed {
                code: FailureCode::Unavailable,
                message: reason.code(),
            }),
        })),
        OutputRecipient::Command(id) => Ok(reply_unavailable(Some(id), reason.code())),
        // The SQL-chat branch must also retire its exact subscription, not only encode a reply.
        OutputRecipient::Chat(_) => Err(TransportError::InvalidFrame),
    }
}

type ChatRead = BoxFuture<
    'static,
    (
        Arc<ChatSubscription>,
        Result<Option<Arc<ChatEnvelope>>, crate::business_log::StoreError>,
    ),
>;

fn current_subscription(peer: &Peer) -> Result<Option<Arc<ChatSubscription>>, TransportError> {
    peer.subscription
        .lock()
        .map(|subscription| subscription.clone())
        .map_err(|_| TransportError::PromptStateUnavailable)
}

fn same_subscription(a: &Option<Arc<ChatSubscription>>, b: &Option<Arc<ChatSubscription>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

fn retire_expired_question(
    messages: &MainMessages,
    connection: &Connection,
    id: as_types::DecisionId,
) -> Option<ModuleError> {
    // Another original may already occupy Main's slot; never retire that one.
    if let Err(error) = messages.retire_expired_question(connection, id)
        && !matches!(error, ModuleError::Busy | ModuleError::Conflict)
    {
        return Some(error);
    }
    None
}

async fn report_retirement(error: Option<ModuleError>) {
    if let Some(error) = error {
        as_types::diagnostics::record_stderr(format!(
            "AS_SCRIPTED_QUESTION_RETIRE_FAILED code={}",
            module_code(error)
        ))
        .await;
    }
}

async fn writer(
    messages: &MainMessages,
    connection: &Connection,
    mut control: mpsc::Receiver<Output>,
    mut events: mpsc::Receiver<ServerEvent>,
    peer: &Peer,
    business: &BusinessLog,
    sink: &mut FrameSink<'_>,
) -> Result<SessionEnd, TransportError> {
    let mut control_open = true;
    let mut events_open = true;
    let mut subscription: Option<Arc<ChatSubscription>> = None;
    let mut after = None;
    let mut reading: Option<ChatRead> = None;
    let mut content: Option<(Arc<ChatSubscription>, Arc<ChatEnvelope>)> = None;
    // Register before querying, including notifications received before first poll.
    let mut changed = business.chat_changed().boxed();
    let mut read_needed = false;
    let mut phase = peer.phase.subscribe();
    let mut committed_tail = None;
    loop {
        if let PeerPhase::Finished(tail) = *phase.borrow_and_update()
            && committed_tail.is_none()
        {
            committed_tail = Some(tail);
            read_needed = true;
        }
        let output_changed = peer.output_changed.notified();
        if peer.stop.is_cancelled() {
            return Ok(SessionEnd::Closed);
        }
        let current = current_subscription(peer)?;
        if !same_subscription(&subscription, &current) {
            after = current.as_ref().and_then(|s| s.start_after);
            read_needed = current.is_some();
            subscription = current;
            content = None;
            // An admitted SQL job survives cancellation of its waiter. Drain the
            // one original future instead of spawning another query on each wake.
        }
        if let Some(prompt) = peer.take_prompt()? {
            let id = prompt.decision_id;
            let deadline = prompt.deadline;
            let ask = match messages.question(connection, prompt) {
                Ok(ask) => Arc::new(ask),
                Err(ModuleError::Stopped) => {
                    report_retirement(retire_expired_question(messages, connection, id)).await;
                    continue;
                }
                Err(error) => {
                    as_types::diagnostics::record_stderr(format!(
                        "AS_SCRIPTED_PROMPT_NOT_DELIVERED code={}",
                        module_code(error)
                    ))
                    .await;
                    continue;
                }
            };
            let admission_failed = match business.record_up(ask.clone()) {
                Ok(completion) => {
                    drop(completion);
                    false
                }
                Err(_) => true,
            };
            let written = write_control_frame(
                messages,
                connection,
                Output::Ordinary(ask, Some(deadline), None),
                &peer.stop,
                sink,
            )
            .await;
            if admission_failed {
                as_types::diagnostics::record_stderr(
                    "AS_SCRIPTED_BUSINESS_STORE_FAILED stage=ask_admission".into(),
                )
                .await;
            }
            if written? == FrameWrite::Stopped {
                return Ok(SessionEnd::Closed);
            }
            continue;
        }
        // Do not start a SQL content frame while an existing control/result is ready.
        match control.try_recv() {
            Ok(output) => {
                if write_control_frame(messages, connection, output, &peer.stop, sink).await?
                    == FrameWrite::Stopped
                {
                    return Ok(SessionEnd::Closed);
                }
                continue;
            }
            Err(mpsc::error::TryRecvError::Disconnected) => control_open = false,
            Err(mpsc::error::TryRecvError::Empty) => {}
        }
        match events.try_recv() {
            Ok(event) => {
                let output = match event {
                    ServerEvent::Notice(up) => Output::Ordinary(up, None, None),
                    ServerEvent::SourceUnavailable => diagnostic("source_unavailable"),
                };
                if write_control_frame(messages, connection, output, &peer.stop, sink).await?
                    == FrameWrite::Stopped
                {
                    return Ok(SessionEnd::Closed);
                }
                continue;
            }
            Err(mpsc::error::TryRecvError::Disconnected) => events_open = false,
            Err(mpsc::error::TryRecvError::Empty) => {}
        }
        if let Some((owner, envelope)) = content.take() {
            if same_subscription(&subscription, &Some(owner.clone())) {
                let cursor = envelope.cursor;
                let output = Output::Ordinary(
                    Arc::new(Up::Chat {
                        subscription_id: owner.id,
                        envelope,
                    }),
                    None,
                    None,
                );
                match write_frame(messages, connection, output, &peer.stop, sink).await? {
                    FrameWrite::Written => {
                        if same_subscription(&current_subscription(peer)?, &Some(owner)) {
                            after = Some(cursor);
                            read_needed = true;
                        }
                    }
                    FrameWrite::Stopped => return Ok(SessionEnd::Closed),
                    FrameWrite::ExpiredUnwritten => return Err(TransportError::InvalidFrame),
                    FrameWrite::RejectedUnwritten(OutputRecipient::Chat(id), reason)
                        if id == owner.id =>
                    {
                        if *phase.borrow() != PeerPhase::Live {
                            return Err(TransportError::ChatDrain);
                        }
                        if let Some(reply) = subscription_failure(peer, &owner, reason.code())?
                            && write_control_frame(messages, connection, reply, &peer.stop, sink)
                                .await?
                                == FrameWrite::Stopped
                        {
                            return Ok(SessionEnd::Closed);
                        }
                    }
                    FrameWrite::RejectedUnwritten(_, _) => {
                        return Err(TransportError::InvalidFrame);
                    }
                }
            }
            continue;
        }
        if let Some(tail) = committed_tail
            && (subscription.is_none() || after == tail)
            && !control_open
            && reading.is_none()
        {
            return Ok(SessionEnd::Drained);
        }
        if read_needed && reading.is_none() {
            read_needed = false;
            if let Some(owner) = subscription.clone() {
                FrameGrant::Content.check(
                    connection,
                    OutputCheckStage::BeforeChatRead,
                    peer.websocket,
                )?;
                let log = business.clone();
                let cursor = after;
                reading = Some(async move { (owner, log.read_next_chat(cursor).await) }.boxed());
            }
        }
        let output = tokio::select! {biased;
            _=peer.stop.cancelled()=>return Ok(SessionEnd::Closed),
            _=output_changed=>continue,
            _=phase.changed(),if committed_tail.is_none()=>continue,
            output=control.recv(),if control_open=>match output{Some(output)=>output,None=>{control_open=false;continue}},
            event=events.recv(),if events_open=>match event{Some(ServerEvent::Notice(up))=>Output::Ordinary(up,None,None),Some(ServerEvent::SourceUnavailable)=>diagnostic("source_unavailable"),None=>{events_open=false;continue}},
            result=async { match reading.as_mut(){Some(read)=>read.await,None=>std::future::pending().await} },if reading.is_some()=>{
                reading=None;
                let(owner,result)=result;
                if same_subscription(&subscription,&Some(owner.clone())) {
                    FrameGrant::Content.check(connection,OutputCheckStage::AfterChatRead,peer.websocket)?;
                    match result {
                        Ok(Some(envelope))=>content=Some((owner,envelope)),
                        Ok(None) if committed_tail.is_some_and(|tail| after != tail) && !read_needed=>return Err(TransportError::ChatDrain),
                        Ok(None)=>{},
                        Err(_) if *phase.borrow() != PeerPhase::Live=>return Err(TransportError::ChatDrain),
                        Err(_)=>{
                            if let Some(reply)=subscription_failure(peer,&owner,"unavailable")?
                                && write_control_frame(messages,connection,reply,&peer.stop,sink).await?==FrameWrite::Stopped
                            {return Ok(SessionEnd::Closed)}
                        },
                    }
                }
                continue;
            },
            _=&mut changed,if subscription.is_some()=>{changed=business.chat_changed().boxed();read_needed=true;continue;},
        };
        if write_control_frame(messages, connection, output, &peer.stop, sink).await?
            == FrameWrite::Stopped
        {
            return Ok(SessionEnd::Closed);
        }
    }
}

async fn write_frame(
    messages: &MainMessages,
    connection: &Connection,
    mut output: Output,
    stop: &CancellationToken,
    sink: &mut FrameSink<'_>,
) -> Result<FrameWrite, TransportError> {
    let stop_reply_write = match &mut output {
        Output::Ordinary(message, deadline, witness) => {
            if witness.is_some()
                && (deadline.is_some()
                    || !matches!(
                        message.as_ref(),
                        Up::Reply {
                            result: CommandResult::Accepted {
                                data: ReplyData::MainStopRequested
                            },
                            ..
                        }
                    ))
            {
                return Err(TransportError::InvalidFrame);
            }
            witness.take()
        }
        _ => None,
    };
    let question = match &output {
        Output::Ordinary(up, Some(deadline), _) => match up.as_ref() {
            Up::Ask { decision_id, .. } => Some((*decision_id, *deadline)),
            _ => return Err(TransportError::InvalidFrame),
        },
        _ => None,
    };
    if let Some((id, deadline)) = question
        && deadline <= tokio::time::Instant::now()
    {
        report_retirement(retire_expired_question(messages, connection, id)).await;
        return Ok(FrameWrite::ExpiredUnwritten);
    }
    let grant = FrameGrant::for_output(&output);
    let target = OutputRecipient::for_output(&output);
    let websocket = matches!(sink, FrameSink::WebSocket(_));
    grant.check(connection, OutputCheckStage::BeforeEncode, websocket)?;
    let encoded = encode(output);
    // A policy change during encoding remains revocation, never a content-size error.
    grant.check(connection, OutputCheckStage::AfterEncode, websocket)?;
    let mut bytes = match prepare_encoding(target, encoded)? {
        Ok(bytes) => bytes,
        Err(outcome) => return Ok(outcome),
    };
    let deadline = question.map(|(_, deadline)| deadline);
    let retirement_error = std::sync::Mutex::new(None);
    let expire = || {
        if let Some((id, _)) = question {
            let mut retained = retirement_error
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            *retained = retire_expired_question(messages, connection, id);
        }
    };
    let authorize = |stage| grant.check(connection, stage, websocket);
    let revoked = async {
        if websocket || grant != FrameGrant::Public {
            connection.revoked().await;
        } else {
            std::future::pending().await
        }
    };
    let outcome = match sink {
        FrameSink::Local(stream) => {
            bytes.push(b'\n');
            write_bytes(stream, &bytes, stop, deadline, expire, authorize, revoked).await
        }
        FrameSink::WebSocket(sink) => {
            websocket::write(sink, bytes, stop, deadline, expire, authorize, revoked).await
        }
    };
    let retirement_error = retirement_error
        .into_inner()
        .unwrap_or_else(|poison| poison.into_inner());
    report_retirement(retirement_error).await;
    let outcome = outcome?;
    if outcome == FrameWrite::Written
        && let Some(witness) = stop_reply_write
    {
        witness.written();
    }
    Ok(outcome)
}

// One in-progress JSONL write; callbacks borrow the original authorization/question owner.
async fn write_bytes(
    socket: &UnixStream,
    bytes: &[u8],
    stop: &CancellationToken,
    deadline: Option<tokio::time::Instant>,
    mut expire: impl FnMut(),
    authorize: impl Fn(OutputCheckStage) -> Result<FrameGrant, TransportError>,
    revoked: impl std::future::Future<Output = ()>,
) -> Result<FrameWrite, TransportError> {
    let mut offset = 0;
    let mut deadline_observed = false;
    tokio::pin!(revoked);
    while offset < bytes.len() {
        authorize(OutputCheckStage::BeforeWrite)?;
        tokio::select! {biased;
            _=stop.cancelled()=>return Ok(FrameWrite::Stopped),
            _=&mut revoked=>{
                let grant=authorize(OutputCheckStage::RevokedWatch)?;
                return Err(TransportError::AuthorizationWatchEnded{grant});
            },
            _=async {match deadline{Some(deadline)=>tokio::time::sleep_until(deadline).await,None=>std::future::pending().await}},if deadline.is_some()&&!deadline_observed=>{
                deadline_observed=true;
                expire();
                if offset==0{return Ok(FrameWrite::ExpiredUnwritten)}
                // The operation expired; only this already-started JSONL write remains.
            },
            ready=socket.writable()=>{
                ready?;
                authorize(OutputCheckStage::WriteReady)?;
                match socket.try_write(&bytes[offset..]){
                    Ok(0)=>return Err(io::Error::from(io::ErrorKind::WriteZero).into()),
                    Ok(n)=>offset+=n,
                    Err(error) if error.kind()==io::ErrorKind::WouldBlock=>{},
                    Err(error)=>return Err(error.into()),
                }
            },
        }
    }
    Ok(FrameWrite::Written)
}
fn encode(output: Output) -> Result<Zeroizing<Vec<u8>>, TransportError> {
    let mut bytes = Zeroizing::new(Vec::new());
    match output {
        Output::Ordinary(up, _, _) => {
            return encode_up(&up)
                .map(Zeroizing::new)
                .map_err(|_| TransportError::InvalidFrame);
        }
        Output::Business(reply) => {
            return encode_lane("business_reply", &reply)
                .map(Zeroizing::new)
                .map_err(|_| TransportError::InvalidFrame);
        }
        Output::Stream(reply) => {
            return encode_lane("stream_reply", &reply)
                .map(Zeroizing::new)
                .map_err(|_| TransportError::InvalidFrame);
        }
        Output::Diagnostic(info) => {
            return encode_lane("diagnostic", &info)
                .map(Zeroizing::new)
                .map_err(|_| TransportError::InvalidFrame);
        }
        Output::Private(reply) => {
            write!(bytes, "{{\"version\":{VERSION},\"private_reply\":{{\"id\":")?;
            json(&mut bytes, &reply.id)?;
            match reply.result {
                Err(error) => {
                    bytes.write_all(b",\"result\":{\"status\":\"failed\",\"code\":")?;
                    json(&mut bytes, &error.to_string())?;
                    bytes.write_all(b"}}}")?;
                }
                Ok(data) => {
                    bytes.write_all(b",\"result\":{\"status\":\"completed\",\"data\":{")?;
                    match data {
                        PrivateData::Ca(info) => {
                            bytes.write_all(b"\"type\":\"ca\",\"info\":")?;
                            json(&mut bytes, &info)?;
                        }
                        PrivateData::Saved(receipt) => {
                            bytes.write_all(b"\"type\":\"secret_saved\",\"receipt\":")?;
                            json(&mut bytes, &receipt)?;
                        }
                        PrivateData::Revoked(revision) => {
                            bytes.write_all(b"\"type\":\"phantom_revoked\",\"revision\":")?;
                            json(&mut bytes, &revision.to_string())?;
                        }
                        PrivateData::Issued(issue) => {
                            bytes.write_all(b"\"type\":\"phantom_issued\",\"token_id\":")?;
                            json(&mut bytes, &issue.token_id)?;
                            bytes.write_all(b",\"binding\":")?;
                            json(&mut bytes, &issue.binding)?;
                            bytes.write_all(b",\"revision\":")?;
                            json(&mut bytes, &issue.revision.to_string())?;
                            bytes.write_all(b",\"value\":")?;
                            let value = std::str::from_utf8(issue.value.expose())
                                .map_err(|_| TransportError::InvalidFrame)?;
                            json(&mut bytes, value)?;
                        }
                    }
                    bytes.write_all(b"}}}}")?;
                }
            }
        }
    }
    Ok(bytes)
}
fn json<T: Serialize + ?Sized>(
    writer: &mut Zeroizing<Vec<u8>>,
    value: &T,
) -> Result<(), TransportError> {
    serde_json::to_writer(&mut **writer, value).map_err(|_| TransportError::InvalidFrame)
}

#[cfg(test)]
mod writer_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn pair() -> (UnixStream, UnixStream) {
        let (write, read) = UnixStream::pair().expect("owned real socket pair");
        write.writable().await.expect("initial readiness");
        (write, read)
    }

    fn partial_payload_bytes(write: &UnixStream) -> usize {
        rustix::net::sockopt::socket_send_buffer_size(write)
            .expect("actual unmodified socket buffer")
            .checked_mul(4)
            .expect("fixture length fits usize")
    }

    #[test]
    fn runtime_reply_encodes_only_the_original_accepted_up() {
        let id = CommandId([3; 16]);
        let output = received_reply(CommandReply {
            message: Up::Reply {
                id,
                result: CommandResult::Accepted {
                    data: ReplyData::MainStopRequested,
                },
            },
            stop_reply_write: None,
        });
        assert!(
            matches!(&output, Output::Ordinary(_, None, None)),
            "no invented capability on a value-only fixture"
        );
        let encoded = encode(output).expect("sole current Up schema");
        let wire: serde_json::Value = serde_json::from_slice(&encoded).expect("JSON");
        assert_eq!(wire["version"], VERSION);
        assert_eq!(
            wire["message"]["id"],
            serde_json::to_value(id).expect("original id")
        );
        assert_eq!(wire["message"]["result"]["status"], "accepted");
        assert_eq!(
            wire["message"]["result"]["data"]["type"],
            "main_stop_requested"
        );
        assert!(wire.get("stop_reply_write").is_none());
        assert!(wire["message"].get("stop_reply_write").is_none());
    }

    #[tokio::test]
    async fn actual_peer_eof_does_not_produce_a_written_stop_reply() {
        let (write, read) = pair().await;
        drop(read);
        let mut bytes = encode(ordinary(
            Up::Reply {
                id: CommandId([3; 16]),
                result: CommandResult::Accepted {
                    data: ReplyData::MainStopRequested,
                },
            },
            None,
        ))
        .expect("stop reply wire only")
        .to_vec();
        bytes.push(b'\n');
        let outcome = write_bytes(
            &write,
            &bytes,
            &CancellationToken::new(),
            None,
            || {},
            |_| Ok(FrameGrant::Public),
            std::future::pending(),
        )
        .await;
        assert!(
            matches!(outcome, Err(TransportError::Io(_))),
            "actual EOF is not Written"
        );
    }

    #[test]
    fn stopping_subscriber_requires_writer_confirmation_before_close() {
        let (events, _receiving) = mpsc::channel(1);
        let peer = Peer {
            events,
            prompt: Mutex::new(None),
            output_changed: Notify::new(),
            stop: CancellationToken::new(),
            review_ready: Box::new(|| true),
            websocket: true,
            subscription: Mutex::new(Some(Arc::new(ChatSubscription {
                id: CommandId([1; 16]),
                start_after: None,
            }))),
            phase: tokio::sync::watch::Sender::new(PeerPhase::Live),
        };
        assert!(peer.check_chat_drain(&Ok(SessionEnd::PeerClose)).is_ok());

        peer.begin_chat_drain();
        for early_end in [
            Ok(SessionEnd::PeerClose),
            Ok(SessionEnd::Closed),
            Err(TransportError::WebSocket),
        ] {
            assert!(matches!(
                peer.check_chat_drain(&early_end),
                Err(TransportError::ChatDrain)
            ));
        }

        peer.finish_chat_drain(Some(ChatCursor {
            service_instance: [4; 16],
            sequence: 7,
        }));
        assert!(matches!(
            peer.check_chat_drain(&Ok(SessionEnd::PeerClose)),
            Err(TransportError::ChatDrain)
        ));
        assert!(peer.check_chat_drain(&Ok(SessionEnd::Drained)).is_ok());

        peer.subscription.lock().expect("subscription state").take();
        assert!(peer.check_chat_drain(&Ok(SessionEnd::PeerClose)).is_ok());
    }

    #[tokio::test]
    async fn oversized_outputs_fail_locally_without_retiring_the_control_peer() {
        let fixture = crate::execution_feedback::tests::Fixture::new();
        let mut message = fixture.message;
        // Valid text grows through ordinary JSON escaping; the source text itself is below 8M.
        message.observed_parts = vec![ChatPart::Text {
            id: 0,
            kind: TextKind::Plain,
            text: "\0".repeat(MAX_FRAME_BYTES / 4),
        }];
        let message = Arc::new(message);
        let id = CommandId([8; 16]);
        let prior = ChatCursor {
            service_instance: [4; 16],
            sequence: 7,
        };
        let owner = Arc::new(ChatSubscription {
            id,
            start_after: Some(prior),
        });
        let (events, _receiving) = mpsc::channel(1);
        let peer = Peer {
            events,
            prompt: Mutex::new(None),
            output_changed: Notify::new(),
            stop: CancellationToken::new(),
            review_ready: Box::new(|| true),
            websocket: false,
            subscription: Mutex::new(Some(owner.clone())),
            phase: tokio::sync::watch::Sender::new(PeerPhase::Live),
        };
        let reject = |output| {
            let target = OutputRecipient::for_output(&output);
            match prepare_encoding(target, encode(output)) {
                Ok(Err(outcome)) => outcome,
                _ => panic!("valid oversized fixture must fail before writing"),
            }
        };
        let output = Output::Ordinary(
            Arc::new(Up::Chat {
                subscription_id: id,
                envelope: Arc::new(ChatEnvelope {
                    cursor: ChatCursor {
                        sequence: 8,
                        ..prior
                    },
                    event: Arc::new(ChatEvent::Finished(message.clone())),
                }),
            }),
            None,
            None,
        );
        assert_eq!(
            reject(output),
            FrameWrite::RejectedUnwritten(OutputRecipient::Chat(id), OutputRejection::TooLarge)
        );
        let failed = subscription_failure(&peer, &owner, OutputRejection::TooLarge.code())
            .expect("exact subscription state")
            .expect("original subscription failure");
        assert!(peer.ready());
        assert!(!peer.stop.is_cancelled());
        assert_eq!(owner.start_after, Some(prior));
        assert!(current_subscription(&peer).expect("state").is_none());
        let failed = encode(failed).expect("small stream failure");
        let value: serde_json::Value = serde_json::from_slice(&failed).expect("finite JSON");
        assert_eq!(
            value["stream_reply"]["id"],
            serde_json::to_value(id).expect("id")
        );
        assert_eq!(value["stream_reply"]["result"]["code"], "output_too_large");

        let business = Output::Business(BusinessReply {
            id,
            result: Box::new(BusinessResult::Completed {
                data: business::BusinessData::ActivityContext {
                    view: crate::context::ContextView {
                        anchor: crate::context::FactReference::Message(message.id),
                        source_records: vec![crate::context::SourceRecord::Message(message)],
                        relations: Vec::new(),
                        gaps: Vec::new(),
                    },
                    attribution: None,
                },
            }),
        });
        assert_eq!(
            reject(business),
            FrameWrite::RejectedUnwritten(OutputRecipient::Business(id), OutputRejection::TooLarge)
        );
        let business_failure =
            rejected_control_output(OutputRecipient::Business(id), OutputRejection::TooLarge)
                .expect("read-only query failure");
        let value: serde_json::Value =
            serde_json::from_slice(&encode(business_failure).expect("finite query reply"))
                .expect("JSON");
        assert_eq!(
            value["business_reply"]["id"],
            serde_json::to_value(id).expect("id")
        );
        assert_eq!(
            value["business_reply"]["result"]["message"],
            "output_too_large"
        );

        let command_target = OutputRecipient::for_output(&Output::Ordinary(
            Arc::new(rejected(id, ModuleError::Busy)),
            None,
            None,
        ));
        assert_eq!(command_target, Some(OutputRecipient::Command(id)));
        let command_failure = rejected_control_output(
            OutputRecipient::Command(id),
            OutputRejection::EncodingFailed,
        )
        .expect("delivery diagnostic");
        let value: serde_json::Value =
            serde_json::from_slice(&encode(command_failure).expect("finite diagnostic"))
                .expect("JSON");
        assert_eq!(
            value["diagnostic"]["id"],
            serde_json::to_value(id).expect("id")
        );
        assert_eq!(value["diagnostic"]["type"], "reply_output_unavailable");
        assert_eq!(value["diagnostic"]["code"], "output_encoding_failed");
        assert!(
            value.get("message").is_none(),
            "no fabricated CommandResult"
        );
        assert!(matches!(
            prepare_encoding(
                Some(OutputRecipient::Chat(id)),
                Err(TransportError::InvalidFrame)
            ),
            Ok(Err(FrameWrite::RejectedUnwritten(
                _,
                OutputRejection::EncodingFailed
            )))
        ));
        assert!(
            matches!(
                prepare_encoding(
                    Some(OutputRecipient::Chat(id)),
                    Err(io::Error::from(io::ErrorKind::BrokenPipe).into())
                ),
                Err(TransportError::Io(_))
            ),
            "real I/O is not rewritten as an output-size diagnostic"
        );

        let replacement = Arc::new(ChatSubscription {
            id,
            start_after: None,
        });
        *peer.subscription.lock().expect("replacement") = Some(replacement.clone());
        assert!(
            subscription_failure(&peer, &owner, "output_too_large")
                .expect("stale result")
                .is_none()
        );
        assert!(Arc::ptr_eq(
            current_subscription(&peer)
                .expect("state")
                .as_ref()
                .expect("new subscription"),
            &replacement
        ));

        let (write, mut read) = pair().await;
        let mut first = failed.to_vec();
        first.push(b'\n');
        let mut next = encode(diagnostic("fixture_control_after_rejection"))
            .expect("original control frame")
            .to_vec();
        next.push(b'\n');
        for bytes in [&first, &next] {
            assert_eq!(
                write_bytes(
                    &write,
                    bytes,
                    &peer.stop,
                    None,
                    || {},
                    |_| Ok(FrameGrant::Public),
                    std::future::pending()
                )
                .await
                .expect("same live transport"),
                FrameWrite::Written
            );
        }
        let expected = [first, next].concat();
        let mut actual = vec![0; expected.len()];
        read.read_exact(&mut actual)
            .await
            .expect("failure then next control frame");
        assert_eq!(actual, expected);
        assert!(peer.ready());
    }

    #[tokio::test]
    async fn expired_unwritten_frame_preserves_socket_for_the_next_control_frame() {
        let (write, mut read) = pair().await;
        let mut occupied = 0;
        loop {
            match write.try_write(&[0; 4096]) {
                Ok(n) => occupied += n,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                other => panic!("fill actual socket: {other:?}"),
            }
        }
        assert!(occupied > 0);
        let expired = AtomicUsize::new(0);
        let stop = CancellationToken::new();
        assert_eq!(
            write_bytes(
                &write,
                b"expired ask\n",
                &stop,
                Some(tokio::time::Instant::now() + std::time::Duration::from_millis(20)),
                || {
                    expired.fetch_add(1, Ordering::Relaxed);
                },
                |_| Ok(FrameGrant::Review),
                std::future::pending(),
            )
            .await
            .expect("not a connection failure"),
            FrameWrite::ExpiredUnwritten,
        );
        assert_eq!(expired.load(Ordering::Relaxed), 1);
        let mut filler = vec![0; occupied];
        read.read_exact(&mut filler)
            .await
            .expect("drain only prior bytes");
        assert_eq!(
            write_bytes(
                &write,
                b"{\"reply\":true}\n",
                &stop,
                None,
                || {},
                |_| Ok(FrameGrant::Public),
                std::future::pending()
            )
            .await
            .expect("same socket remains usable"),
            FrameWrite::Written,
        );
        let mut actual = vec![0; b"{\"reply\":true}\n".len()];
        read.read_exact(&mut actual)
            .await
            .expect("next complete frame");
        assert_eq!(actual, b"{\"reply\":true}\n");
    }

    #[tokio::test]
    async fn partial_frame_expiry_ends_question_but_completes_the_same_jsonl() {
        let (write, mut read) = pair().await;
        let bytes = [
            b"{\"fixture\":\"".as_slice(),
            &vec![b'x'; partial_payload_bytes(&write)],
            b"\"}\n",
        ]
        .concat();
        let expected = bytes.clone();
        let expired = Arc::new(AtomicUsize::new(0));
        let seen = expired.clone();
        let expired_signal = Arc::new(Notify::new());
        let signal = expired_signal.clone();
        let writing = tokio::spawn(async move {
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(25);
            write_bytes(
                &write,
                &bytes,
                &CancellationToken::new(),
                Some(deadline),
                || {
                    seen.fetch_add(1, Ordering::Release);
                    signal.notify_one();
                },
                |_| Ok(FrameGrant::Review),
                std::future::pending(),
            )
            .await
        });
        let mut first = [0];
        read.read_exact(&mut first)
            .await
            .expect("a real prefix was written");
        tokio::time::timeout(std::time::Duration::from_secs(1), expired_signal.notified())
            .await
            .expect("original logical expiry observed");
        assert!(
            !writing.is_finished(),
            "expiry must not discard the partial frame"
        );
        let mut actual = vec![0; expected.len()];
        actual[0] = first[0];
        read.read_exact(&mut actual[1..])
            .await
            .expect("complete the original line");
        assert_eq!(
            writing.await.expect("join").expect("write result"),
            FrameWrite::Written
        );
        assert_eq!(expired.load(Ordering::Acquire), 1);
        assert_eq!(actual, expected);
        assert!(serde_json::from_slice::<serde_json::Value>(&actual).is_ok());
    }

    #[tokio::test]
    async fn partial_frame_revocation_stops_the_write_without_claiming_completion() {
        let (write, mut read) = pair().await;
        let length = partial_payload_bytes(&write);
        let revoke = CancellationToken::new();
        let signal = revoke.clone();
        let checked = revoke.clone();
        let writing = tokio::spawn(async move {
            write_bytes(
                &write,
                &vec![b'x'; length],
                &CancellationToken::new(),
                None,
                || {},
                |stage| {
                    if checked.is_cancelled() {
                        Err(TransportError::OutputAuthorization {
                            grant: FrameGrant::Content,
                            stage,
                            reason: ModuleError::Unauthenticated,
                        })
                    } else {
                        Ok(FrameGrant::Content)
                    }
                },
                signal.cancelled(),
            )
            .await
        });
        let mut first = [0];
        read.read_exact(&mut first)
            .await
            .expect("real partial frame");
        revoke.cancel();
        assert!(matches!(
            writing.await.expect("join"),
            Err(TransportError::OutputAuthorization {
                grant: FrameGrant::Content,
                reason: ModuleError::Unauthenticated,
                ..
            })
        ));
        let mut remaining = Vec::new();
        read.read_to_end(&mut remaining)
            .await
            .expect("owned writer retired");
        assert!(
            remaining.len() + 1 < length,
            "revocation stops unsent content"
        );
        read.shutdown().await.expect("retire fixture socket");
    }

    #[tokio::test]
    async fn ended_revocation_watch_does_not_invent_a_failed_permission_check() {
        let (write, _read) = pair().await;
        let error = write_bytes(
            &write,
            b"sensitive fixture\n",
            &CancellationToken::new(),
            None,
            || {},
            |_| Ok(FrameGrant::Content),
            std::future::ready(()),
        )
        .await
        .expect_err("watch ending stops sensitive output");
        assert!(matches!(
            error,
            TransportError::AuthorizationWatchEnded {
                grant: FrameGrant::Content
            }
        ));
        assert_eq!(
            error.to_string(),
            "output_authorization_watch_ended grant=content stage=revoked_watch"
        );
        let error = TransportError::OutputAuthorization {
            grant: FrameGrant::Admin,
            stage: OutputCheckStage::AfterEncode,
            reason: ModuleError::Forbidden,
        };
        assert_eq!(
            error.to_string(),
            "output_authorization_failed grant=admin stage=after_encode reason=Forbidden"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use as_types::main_ports::Decision;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use tokio::io::AsyncWriteExt;
    #[test]
    fn private_wire_keeps_exact_secret_and_one_dedicated_reply_shape() {
        let request:PrivateRequest=serde_json::from_str(r#"{"id":[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1],"command":{"type":"import_secret","value":" a\"b\\ "}}"#).expect("private frame");
        assert!(
            matches!(request.command,PrivateCommand::ImportSecret{value} if value.0.expose()==b" a\"b\\ ")
        );
        let reply = PrivateReply {
            id: CommandId([1; 16]),
            result: Ok(PrivateData::Issued(PhantomIssue {
                token_id: PhantomTokenId([2; 16]),
                binding: BindingId([3; 16]),
                revision: 4,
                value: PrivateBytes::new(b"as_ph_fixture_only".to_vec()),
            })),
        };
        let bytes = encode(Output::Private(reply)).expect("private encoder");
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("JSON reply");
        assert_eq!(
            v["private_reply"]["result"]["data"]["value"],
            "as_ph_fixture_only"
        );
        assert_eq!(v["private_reply"]["result"]["data"]["revision"], "4");
        assert!(v.get("message").is_none());
    }
    #[tokio::test]
    async fn jsonl_framing_is_bounded_and_rejects_partial_eof() {
        let (mut writer, reader) = UnixStream::pair().expect("pair");
        writer.write_all(b"{}\n{}\n").await.expect("frames");
        let mut lines = Lines {
            pending: Zeroizing::new(Vec::new()),
        };
        assert_eq!(
            &**lines.next(&reader).await.expect("first").expect("frame"),
            b"{}"
        );
        assert_eq!(
            &**lines.next(&reader).await.expect("second").expect("frame"),
            b"{}"
        );
        writer.write_all(b"{").await.expect("partial");
        writer.shutdown().await.expect("EOF");
        assert!(matches!(
            lines.next(&reader).await,
            Err(TransportError::InvalidFrame)
        ));
        let mut lines = Lines {
            pending: Zeroizing::new(vec![b'x'; MAX_FRAME_BYTES + 1]),
        };
        assert!(matches!(
            lines.next(&reader).await,
            Err(TransportError::InvalidFrame)
        ));
    }
    #[tokio::test]
    async fn endpoint_cleanup_preserves_an_unrelated_replacement() {
        let root = tempfile::tempdir().expect("root");
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))
            .expect("root mode");
        let path = root.path().join("run/middleware.sock");
        let uid = rustix::process::geteuid().as_raw();
        let endpoint = Endpoint::bind(&path, uid).expect("bind");
        assert_eq!(
            std::fs::metadata(&path).expect("socket mode").mode() & 0o777,
            0o600
        );
        drop(endpoint);
        assert!(!path.exists());
        let endpoint = Endpoint::bind(&path, uid).expect("second bind");
        std::fs::remove_file(&path).expect("replace fixture inode");
        std::fs::write(&path, b"replacement").expect("other object");
        drop(endpoint);
        assert_eq!(std::fs::read(&path).expect("preserved"), b"replacement");
    }

    #[tokio::test]
    async fn control_backpressure_keeps_identity_and_independent_chat_intake() {
        use as_types::{context::*, main_ports::*};
        let root = tempfile::tempdir().expect("layout");
        let base = root.path();
        let paths = ConfigPaths::from_configuration(ResolvedPaths {
            backend_executable: base.join("bin/as-service"),
            self_protection_root: base.into(),
            configuration_file: base.join("config/as.yaml"),
            rules_file: base.join("config/rules.yaml"),
            driver_assets_directory: base.join("driver"),
            middleware_endpoint: base.join("run/middleware.sock"),
            data_directory: base.join("data"),
            credential_store_file: base.join("data/credentials/store"),
            ca_public_file: base.join("data/credentials/ca.crt"),
            security_log_file: base.join("log/security.sqlite"),
            middleware_database_file: base.join("data/middleware/business.sqlite"),
        });
        let business = BusinessLog::initialize(
            &paths,
            as_types::log_storage::LogStorageBudget::new(Default::default()),
        )
        .await
        .expect("database");
        let media = Arc::new(crate::media::Media::new(business.clone()));
        let (events, mut unread) = mpsc::channel(1);
        events
            .try_send(ServerEvent::SourceUnavailable)
            .expect("occupy peer output");
        let peer = Arc::new(Peer {
            events,
            prompt: Mutex::new(None),
            output_changed: Notify::new(),
            stop: CancellationToken::new(),
            review_ready: Box::new(|| true),
            websocket: false,
            subscription: Mutex::new(None),
            phase: tokio::sync::watch::Sender::new(PeerPhase::Live),
        });
        media.set_peer(Some(peer.clone())).expect("current peer");
        let f = crate::execution_feedback::tests::Fixture::new();
        let (notices, incoming) = mpsc::channel(1);
        notices
            .send(MainMessage::DriverUnavailable)
            .await
            .expect("control input");
        drop(notices);
        let (chat, inputs) = ChatSender::channel(64 * 1024);
        for sequence in 1..=2 {
            let mut message = f.message.clone();
            message.id.sequence = sequence;
            chat.send(
                &PreparedChatEvent::new(ChatEvent::Finished(Arc::new(message)))
                    .expect("source charge")
                    .into_envelope(ChatCursor {
                        service_instance: [1; 16],
                        sequence,
                    }),
            )
            .await
            .expect("source message");
        }
        drop(chat);
        let control_media = media.clone();
        let control = tokio::spawn(async move { control_media.run(incoming).await });
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            media.consume_chat(inputs, None),
        )
        .await
        .expect("frontend cannot hold intake")
        .expect("drained");
        assert!(!peer.stop.is_cancelled());
        assert!(peer.ready());
        assert_eq!(business.counts().await.expect("committed").messages, 2);
        assert_eq!(media.stored_records(), 2);
        unread.try_recv().expect("original occupied control slot");
        tokio::time::timeout(std::time::Duration::from_secs(1), unread.recv())
            .await
            .expect("control slot returned")
            .expect("retained control source");
        control
            .await
            .expect("control task")
            .expect("actual control EOF");
        assert_eq!(media.not_forwarded(), 0);
        business.close().await.expect("close");
    }

    #[tokio::test]
    async fn prompt_slot_retires_only_closed_or_expired_originals() {
        fn prompt(
            id: u8,
            deadline: tokio::time::Instant,
        ) -> (Prompt, tokio::sync::oneshot::Receiver<Decision>) {
            let mut bytes = vec![0u8; 88];
            bytes[0..2].copy_from_slice(&1u16.to_le_bytes());
            bytes[2..4].copy_from_slice(&88u16.to_le_bytes());
            bytes[4..8].copy_from_slice(&88u32.to_le_bytes());
            bytes[8..10].copy_from_slice(&1u16.to_le_bytes());
            bytes[10..12].copy_from_slice(&2u16.to_le_bytes());
            bytes[12..16].copy_from_slice(&1u32.to_le_bytes());
            bytes[16..32].copy_from_slice(&[1; 16]);
            bytes[32..40].copy_from_slice(&1u64.to_le_bytes());
            bytes[40..48].copy_from_slice(&1u64.to_le_bytes());
            let (reply, receiving) = tokio::sync::oneshot::channel();
            (
                Prompt {
                    decision_id: as_types::DecisionId([id; 16]),
                    event: Arc::new(
                        as_types::dispatch_event::decode(&bytes).expect("event fixture"),
                    ),
                    hit: as_types::main_ports::RuleHit {
                        group_id: "ask".into(),
                        group_number: std::num::NonZeroU32::MIN,
                        rule_id: "ask".into(),
                        rule_number: std::num::NonZeroU32::MIN,
                        source: as_types::main_ports::RuleSourceVersion::RulesDocument(
                            as_types::RuleVersion::from_digest([1; 32]),
                        ),
                        log: false,
                        deny_message: None,
                    },
                    deadline,
                    timeout: Decision::Allow,
                    reply,
                },
                receiving,
            )
        }
        let (events, mut receiving) = mpsc::channel(1);
        let peer = Peer {
            events,
            prompt: Mutex::new(None),
            output_changed: Notify::new(),
            stop: CancellationToken::new(),
            review_ready: Box::new(|| true),
            websocket: false,
            subscription: Mutex::new(None),
            phase: tokio::sync::watch::Sender::new(PeerPhase::Live),
        };
        let future = tokio::time::Instant::now() + std::time::Duration::from_secs(60);
        let (first, first_reply) = prompt(1, future);
        assert!(peer.try_prompt(first).is_none());
        let (overlapping, _) = prompt(2, future);
        assert!(
            peer.try_prompt(overlapping)
                .is_some_and(|returned| returned.decision_id == as_types::DecisionId([2; 16])),
            "an active original cannot be replaced"
        );
        drop(first_reply);
        let (next, _next_reply) = prompt(3, future);
        assert!(
            peer.try_prompt(next).is_none(),
            "a closed original cannot occupy the next slot"
        );
        assert_eq!(
            peer.take_prompt()
                .expect("slot")
                .expect("current")
                .decision_id,
            as_types::DecisionId([3; 16])
        );
        let (expired, _expired_reply) = prompt(
            4,
            tokio::time::Instant::now() - std::time::Duration::from_secs(1),
        );
        assert!(peer.try_prompt(expired).is_none());
        let (live, _live_reply) = prompt(5, future);
        assert!(
            peer.try_prompt(live).is_none(),
            "retire only the original expired deadline"
        );
        let live = peer.take_prompt().expect("slot").expect("live");
        assert_eq!(live.decision_id, as_types::DecisionId([5; 16]));
        let original = Arc::new(Up::Ask {
            decision_id: live.decision_id,
            hit: live.hit,
            event: live.event,
            choices: vec![Decision::Allow],
            timeout: Decision::Deny {
                terminate: as_types::main_ports::Termination::Actor,
            },
            remaining_ms: u64::MAX,
        });
        let wire = encode(Output::Ordinary(
            original.clone(),
            Some(live.deadline),
            None,
        ))
        .expect("original Ask encoder");
        let value: serde_json::Value = serde_json::from_slice(&wire).expect("current envelope");
        assert_eq!(
            value["message"],
            serde_json::to_value(original.as_ref()).expect("sole Up schema")
        );
        assert_eq!(
            value["message"]["timeout"],
            serde_json::json!({"type":"deny","terminate":"actor"})
        );
        peer.notice(Arc::new(rejected(CommandId([6; 16]), ModuleError::Busy)))
            .await
            .expect("actual admission");
        let mut waiting =
            Box::pin(peer.notice(Arc::new(rejected(CommandId([7; 16]), ModuleError::Busy))));
        assert!(futures::poll!(&mut waiting).is_pending());
        assert!(
            peer.ready(),
            "ordinary queue pressure is not a channel disconnect"
        );
        assert!(
            receiving.try_recv().is_ok(),
            "cancel does not overwrite an accepted result"
        );
        waiting.await.expect("control retained until actual slot");
        let (control, mut outputs) = mpsc::channel(1);
        enqueue(&control, diagnostic("first"))
            .await
            .expect("first reply admission");
        let mut reply = Box::pin(enqueue(&control, diagnostic("next")));
        assert!(
            futures::poll!(&mut reply).is_pending(),
            "reply Full is backpressure, not a fatal transport error"
        );
        assert!(peer.ready());
        outputs.try_recv().expect("original reply slot");
        reply.await.expect("same bounded control slot returned");
        peer.close();
        assert!(peer.source_unavailable().await.is_err());
    }
}
