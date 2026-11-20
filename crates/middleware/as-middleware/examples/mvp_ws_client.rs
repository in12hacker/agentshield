//! Test-only WebSocket transport for the packaged scripted front end.
use as_main::messages::protocol::MAX_FRAME_BYTES;
use bytes::Bytes;
use serde::Deserialize;
use serde_json::value::RawValue;
use std::{
    io::{self, BufRead, Read, Write},
    net::{SocketAddr, TcpStream},
    process::ExitCode,
    time::{Duration, Instant},
};
use tokio_tungstenite::tungstenite::{
    Error, Message, Utf8Bytes, WebSocket,
    client::{IntoClientRequest, client_with_config},
    protocol::{CloseFrame, WebSocketConfig, frame::coding::CloseCode},
};
use zeroize::Zeroizing;

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Control<'a> {
    Send(#[serde(borrow)] &'a RawValue),
    Receive(u64),
    Close,
}

fn line(out: &mut impl Write, text: &str) -> Result<(), &'static str> {
    writeln!(out, "{text}")
        .and_then(|()| out.flush())
        .map_err(|_| "pipe_write")
}

fn receive(
    socket: &mut WebSocket<TcpStream>,
    out: &mut impl Write,
    timeout: Duration,
    closing: bool,
) -> Result<bool, &'static str> {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("receive_timeout");
        }
        socket
            .get_ref()
            .set_read_timeout(Some(remaining))
            .map_err(|_| "socket_timeout")?;
        match socket.read() {
            Ok(Message::Text(text)) if !closing => {
                if text.contains(['\r', '\n']) {
                    return Err("noncompact_application_frame");
                }
                line(out, text.as_str())?;
                return Ok(true);
            }
            Ok(Message::Text(_) | Message::Pong(_)) => {}
            Ok(Message::Ping(_)) => socket.flush().map_err(|_| "pong_write")?,
            Ok(Message::Close(frame)) => {
                match socket.flush() {
                    Ok(()) | Err(Error::ConnectionClosed) => {}
                    Err(_) => return Err("close_reply_write"),
                }
                let code = frame.map_or_else(|| "null".into(), |frame| u16::from(frame.code).to_string());
                line(out, &format!(r#"{{"transport":"close","code":{code}}}"#))?;
                return Ok(false);
            }
            Err(Error::ConnectionClosed | Error::Protocol(tokio_tungstenite::tungstenite::error::ProtocolError::ResetWithoutClosingHandshake)) => {
                line(out, r#"{"transport":"eof"}"#)?;
                return Ok(false);
            }
            Err(Error::Io(error)) if matches!(error.kind(), io::ErrorKind::ConnectionReset | io::ErrorKind::UnexpectedEof) => {
                line(out, r#"{"transport":"eof"}"#)?;
                return Ok(false);
            }
            Err(Error::Io(error)) if matches!(error.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock) => {
                return Err("receive_timeout");
            }
            Ok(_) => return Err("unexpected_ws_message"),
            Err(_) => return Err("ws_read"),
        }
    }
}

fn run() -> Result<(), &'static str> {
    let mut args = std::env::args().skip(1);
    let endpoint: SocketAddr = args
        .next()
        .ok_or("missing_endpoint")?
        .parse()
        .map_err(|_| "invalid_endpoint")?;
    let origin = args.next().ok_or("missing_origin")?;
    if !endpoint.ip().is_loopback() || endpoint.port() == 0 || args.next().is_some() {
        return Err("invalid_arguments");
    }
    let timeout = Duration::from_secs(5);
    let stream = TcpStream::connect_timeout(&endpoint, timeout).map_err(|_| "connect")?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|_| "socket_timeout")?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|_| "socket_timeout")?;
    let mut request = format!("ws://{endpoint}/")
        .into_client_request()
        .map_err(|_| "request")?;
    request
        .headers_mut()
        .insert("origin", origin.parse().map_err(|_| "invalid_origin")?);
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_FRAME_BYTES))
        .max_frame_size(Some(MAX_FRAME_BYTES));
    let (mut socket, _) =
        client_with_config(request, stream, Some(config)).map_err(|_| "handshake")?;
    let mut out = io::stdout().lock();
    let mut input = io::stdin().lock();
    line(&mut out, r#"{"transport":"open"}"#)?;
    loop {
        let mut command = Zeroizing::new(String::new());
        let count = (&mut input)
            .take((MAX_FRAME_BYTES + 128) as u64)
            .read_line(&mut command)
            .map_err(|_| "pipe_read")?;
        if count == 0 {
            return Ok(());
        }
        if !command.ends_with('\n') {
            return Err("pipe_frame_limit");
        }
        match serde_json::from_str(&command).map_err(|_| "pipe_command")? {
            Control::Send(frame) => {
                if frame.get().len() > MAX_FRAME_BYTES {
                    return Err("application_frame_limit");
                }
                let bytes = Bytes::from_owner(Zeroizing::new(frame.get().as_bytes().to_vec()));
                let text = Utf8Bytes::try_from(bytes).map_err(|_| "invalid_application_frame")?;
                socket.send(Message::Text(text)).map_err(|_| "ws_write")?;
                line(&mut out, r#"{"transport":"sent"}"#)?;
            }
            Control::Receive(ms) if (1..=60_000).contains(&ms) => {
                if !receive(&mut socket, &mut out, Duration::from_millis(ms), false)? {
                    return Ok(());
                }
            }
            Control::Receive(_) => return Err("invalid_receive_timeout"),
            Control::Close => {
                socket
                    .close(Some(CloseFrame {
                        code: CloseCode::Normal,
                        reason: "".into(),
                    }))
                    .map_err(|_| "close_write")?;
                receive(&mut socket, &mut out, timeout, true)?;
                return Ok(());
            }
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(reason) => {
            eprintln!("frontend_ws: {reason}");
            ExitCode::FAILURE
        }
    }
}
