//! The WebSocket framing and first-authentication boundary.
use super::{
    FrameGrant, FrameWrite, InputFrame, OutputCheckStage, Peer, TransportError, session_on,
};
use crate::{business_log::BusinessLog, codec::encode_lane};
use as_main::messages::{
    MainMessages,
    protocol::{MAX_FRAME_BYTES, VERSION},
};
use as_types::configuration::Access;
use bytes::Bytes;
use futures::{
    SinkExt, StreamExt,
    stream::{SplitSink, SplitStream},
};
use serde::{Deserialize, Serialize};
use std::{future::Future, io, net::SocketAddr, sync::Arc};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{
    WebSocketStream, accept_hdr_async_with_config,
    tungstenite::{
        Message, Utf8Bytes,
        handshake::server::{ErrorResponse, Request, Response},
        protocol::WebSocketConfig,
    },
};
use tokio_util::sync::CancellationToken;
use zeroize::{Zeroize, Zeroizing};

type Socket = WebSocketStream<TcpStream>;
pub(super) type WsReader = SplitStream<Socket>;
pub(super) struct WsWriter {
    sink: SplitSink<Socket, Message>,
    buffered: bool,
}

/// Borrow the library's actual input and clear it if the allocation is no longer shared.
pub(super) struct WsText(Utf8Bytes);

impl WsText {
    pub(super) fn as_bytes(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl Drop for WsText {
    fn drop(&mut self) {
        if let Ok(mut bytes) = Bytes::from(std::mem::take(&mut self.0)).try_into_mut() {
            bytes.as_mut().zeroize();
        }
    }
}

/// Explicit local WebSocket listener. Main owns the live admission policy.
pub struct WebSocketEndpoint(TcpListener);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FirstMessage {
    version: u16,
    authenticate: Authentication,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Authentication {
    token: super::SecretInput,
}

#[derive(Serialize)]
struct Authenticated {
    access: Access,
    chat_content: bool,
}

impl WebSocketEndpoint {
    /// Bind the explicitly configured local endpoint.
    pub async fn bind(address: SocketAddr) -> io::Result<Self> {
        if !address.ip().is_loopback() || address.port() == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid WebSocket listener",
            ));
        }
        TcpListener::bind(address).await.map(Self)
    }

    /// Accept one transport before its first-message authentication.
    pub async fn accept(&self) -> io::Result<TcpStream> {
        self.0.accept().await.map(|(stream, _)| stream)
    }

    /// Verify the first application frame and return the shared middleware session.
    pub async fn authenticate(
        stream: TcpStream,
        origin: Option<String>,
        messages: Arc<MainMessages>,
        business: BusinessLog,
    ) -> Result<
        (
            Arc<Peer>,
            impl Future<Output = Result<(), TransportError>> + Send,
        ),
        TransportError,
    > {
        let local = stream.local_addr()?;
        let config = WebSocketConfig::default()
            .max_message_size(Some(MAX_FRAME_BYTES))
            .max_frame_size(Some(MAX_FRAME_BYTES))
            .max_write_buffer_size(MAX_FRAME_BYTES + 256 * 1024);
        let mut accepted_origin = None;
        let mut socket = accept_hdr_async_with_config(
            stream,
            |request: &Request, response: Response| {
                let mut headers = request.headers().get_all("origin").iter();
                if let Some(value) = headers.next() {
                    let value = value.to_str().map_err(|_| forbidden())?;
                    if origin.as_deref() != Some(value) || headers.next().is_some() {
                        return Err(forbidden());
                    }
                    accepted_origin = Some(value.to_owned());
                }
                if request
                    .uri()
                    .path_and_query()
                    .is_none_or(|value| value.as_str() != "/")
                {
                    return Err(forbidden());
                }
                Ok(response)
            },
            Some(config),
        )
        .await
        .map_err(|_| TransportError::WebSocket)?;

        let frame = match read(&mut socket).await? {
            Some(InputFrame::WebSocket(frame)) => frame,
            Some(InputFrame::Close) => {
                socket
                    .flush()
                    .await
                    .map_err(|_| TransportError::WebSocket)?;
                return Err(TransportError::Authentication);
            }
            _ => return Err(TransportError::Authentication),
        };
        let input: FirstMessage =
            serde_json::from_slice(frame.as_bytes()).map_err(|_| TransportError::Authentication)?;
        if input.version != VERSION {
            return Err(TransportError::Authentication);
        }
        let connection = messages
            .connect_websocket(
                local,
                accepted_origin.as_deref(),
                input.authenticate.token.0.expose(),
            )
            .map_err(|_| TransportError::Authentication)?;
        drop(input);
        drop(frame);
        let (access, chat_content) = connection
            .permissions()
            .map_err(|_| TransportError::Authentication)?;
        let reply = encode_lane(
            "authenticated",
            &Authenticated {
                access,
                chat_content,
            },
        )
        .map_err(|_| TransportError::InvalidFrame)?
        .try_into()
        .map_err(|_| TransportError::InvalidFrame)?;
        tokio::select! {biased;
            _=connection.revoked()=>return Err(TransportError::Authentication),
            sent=socket.send(Message::Text(reply))=>sent.map_err(|_|TransportError::WebSocket)?,
        }
        let (writer, reader) = socket.split();
        Ok(session_on(
            messages,
            connection,
            business,
            Some((
                reader,
                WsWriter {
                    sink: writer,
                    buffered: false,
                },
            )),
        ))
    }
}

fn forbidden() -> ErrorResponse {
    let mut response = ErrorResponse::new(None);
    *response.status_mut() = tokio_tungstenite::tungstenite::http::StatusCode::FORBIDDEN;
    response
}

async fn read(
    stream: &mut (
             impl futures::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin
         ),
) -> Result<Option<InputFrame>, TransportError> {
    while let Some(message) = stream.next().await {
        match message.map_err(|_| TransportError::WebSocket)? {
            Message::Text(text) => return Ok(Some(InputFrame::WebSocket(WsText(text)))),
            Message::Close(_) => return Ok(Some(InputFrame::Close)),
            Message::Ping(_) | Message::Pong(_) => {}
            _ => return Err(TransportError::InvalidFrame),
        }
    }
    Ok(None)
}

pub(super) async fn receive(reader: &mut WsReader) -> Result<Option<InputFrame>, TransportError> {
    read(reader).await
}

pub(super) async fn finish_close(
    sink: &mut WsWriter,
    stop: &CancellationToken,
) -> Result<(), TransportError> {
    // After the grant is gone, flush only the library's Close reply.
    // An unfinished application write may still have bytes in the same buffer.
    if sink.buffered {
        return Ok(());
    }
    tokio::select! {biased;
        _=stop.cancelled()=>Err(TransportError::OutboundClosed),
        result=sink.sink.flush()=>result.map_err(|_|TransportError::WebSocket),
    }
}

pub(super) async fn close_drained(
    reader: &mut WsReader,
    writer: &mut WsWriter,
    stop: &CancellationToken,
) -> Result<(), TransportError> {
    use tokio_tungstenite::tungstenite::protocol::{CloseFrame, frame::coding::CloseCode};
    tokio::select! {biased;
        _=stop.cancelled()=>return Err(TransportError::OutboundClosed),
        result=writer.sink.send(Message::Close(Some(CloseFrame {
            code: CloseCode::Normal,
            reason: "".into(),
        })))=>result.map_err(|_|TransportError::WebSocket)?,
    }
    // The tail and Close are written. Retain this transport until its actual peer result.
    loop {
        tokio::select! {biased;
            _=stop.cancelled()=>return Err(TransportError::OutboundClosed),
            result=receive(reader)=>match result? {
                Some(InputFrame::WebSocket(_))=>{},
                _=>return Ok(()),
            },
        }
    }
}

pub(super) async fn write(
    sink: &mut WsWriter,
    mut bytes: Zeroizing<Vec<u8>>,
    stop: &CancellationToken,
    deadline: Option<tokio::time::Instant>,
    mut expire: impl FnMut(),
    authorize: impl Fn(OutputCheckStage) -> Result<FrameGrant, TransportError>,
    revoked: impl Future<Output = ()>,
) -> Result<FrameWrite, TransportError> {
    use futures::Sink;
    use std::{future::poll_fn, pin::Pin};

    let expiry = async {
        match deadline {
            Some(deadline) => tokio::time::sleep_until(deadline).await,
            None => std::future::pending().await,
        }
    };
    tokio::pin!(expiry, revoked);
    let grant = authorize(OutputCheckStage::BeforeWrite)?;
    let text = if grant == FrameGrant::Admin {
        Utf8Bytes::try_from(Bytes::from_owner(bytes))
    } else {
        Utf8Bytes::try_from(std::mem::take(&mut *bytes))
    }
    .map_err(|_| TransportError::InvalidFrame)?;
    tokio::select! {biased;
        _=stop.cancelled()=>return Ok(FrameWrite::Stopped),
        _=&mut revoked=>{
            let grant=authorize(OutputCheckStage::RevokedWatch)?;
            return Err(TransportError::AuthorizationWatchEnded{grant});
        },
        _=&mut expiry=>{expire();return Ok(FrameWrite::ExpiredUnwritten);},
        ready=poll_fn(|cx| Pin::new(&mut sink.sink).poll_ready(cx))=>ready.map_err(|_|TransportError::WebSocket)?,
    }
    authorize(OutputCheckStage::WriteReady)?;
    sink.buffered = true;
    Pin::new(&mut sink.sink)
        .start_send(Message::Text(text))
        .map_err(|_| TransportError::WebSocket)?;
    let mut expired = false;
    loop {
        tokio::select! {biased;
            _=stop.cancelled()=>return Ok(FrameWrite::Stopped),
            _=&mut revoked=>{
                let grant=authorize(OutputCheckStage::RevokedWatch)?;
                return Err(TransportError::AuthorizationWatchEnded{grant});
            },
            _=&mut expiry, if !expired=>{expire();expired=true;},
            result=poll_fn(|cx| {
                if let Err(error) = authorize(OutputCheckStage::WriteReady) {
                    return std::task::Poll::Ready(Err(error));
                }
                Pin::new(&mut sink.sink).poll_flush(cx).map_err(|_|TransportError::WebSocket)
            })=>{result?;sink.buffered=false;return Ok(FrameWrite::Written);},
        }
    }
}
