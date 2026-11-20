//! Transparent write completion witness over the mature HTTP writer and TLS stream.
//! One upstream connection carries exactly one exchange; no HTTP decoding or rewriting.
use crate::feedback::WriteGuard;
use as_types::flow::RuleFeedbackFailure;
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_util::sync::CancellationToken;

pub(crate) struct Witness<S> {
    io: S,
    guard: Option<WriteGuard>,
    framing: Framing,
    expected: usize,
    written: usize,
    complete: bool,
}
enum Framing {
    H1 {
        tail: u32,
        headers: bool,
    },
    H2 {
        preface: usize,
        header: [u8; 9],
        header_len: usize,
        remaining: usize,
        data: bool,
        end: bool,
    },
}
impl<S> Witness<S> {
    pub fn new(io: S, guard: Option<WriteGuard>, expected: usize, h2: bool) -> Self {
        Self {
            io,
            guard,
            expected,
            written: 0,
            complete: false,
            framing: if h2 {
                Framing::H2 {
                    preface: 24,
                    header: [0; 9],
                    header_len: 0,
                    remaining: 0,
                    data: false,
                    end: false,
                }
            } else {
                Framing::H1 {
                    tail: 0,
                    headers: false,
                }
            },
        }
    }
    pub fn cancellations(&self) -> Option<(Vec<CancellationToken>, CancellationToken)> {
        self.guard.as_ref().map(WriteGuard::cancellations)
    }
    fn check(&mut self) -> io::Result<()> {
        if self.guard.as_ref().is_some_and(WriteGuard::cancelled) {
            if let Some(g) = &mut self.guard {
                g.failed(RuleFeedbackFailure::Cancelled)
            }
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        Ok(())
    }
    fn observe(&mut self, mut bytes: &[u8]) -> io::Result<()> {
        if self.guard.is_none() || self.complete {
            return Ok(());
        }
        match &mut self.framing {
            Framing::H1 { tail, headers } => {
                for b in bytes {
                    if *headers {
                        self.written += 1;
                    } else {
                        *tail = (*tail << 8) | u32::from(*b);
                        if *tail == 0x0d0a0d0a {
                            *headers = true;
                        }
                    }
                }
                self.complete = *headers && self.written == self.expected;
            }
            Framing::H2 {
                preface,
                header,
                header_len,
                remaining,
                data,
                end,
            } => {
                let skip = (*preface).min(bytes.len());
                *preface -= skip;
                bytes = &bytes[skip..];
                while !bytes.is_empty() {
                    if *remaining == 0 {
                        let n = (9 - *header_len).min(bytes.len());
                        header[*header_len..*header_len + n].copy_from_slice(&bytes[..n]);
                        *header_len += n;
                        bytes = &bytes[n..];
                        if *header_len < 9 {
                            break;
                        }
                        *header_len = 0;
                        *remaining = (usize::from(header[0]) << 16)
                            | (usize::from(header[1]) << 8)
                            | usize::from(header[2]);
                        let stream =
                            u32::from_be_bytes([header[5], header[6], header[7], header[8]])
                                & 0x7fff_ffff;
                        *data = header[3] == 0 && stream == 1;
                        *end = *data && header[4] & 1 != 0;
                        if *data && header[4] & 8 != 0 {
                            return Err(io::Error::from(io::ErrorKind::InvalidData));
                        }
                        if header[3] == 3 && stream == 1 {
                            return Err(io::Error::from(io::ErrorKind::ConnectionAborted));
                        }
                        if *remaining == 0 {
                            if *end {
                                self.complete = self.written == self.expected;
                            }
                            continue;
                        }
                    }
                    let n = (*remaining).min(bytes.len());
                    if *data {
                        self.written += n;
                    }
                    *remaining -= n;
                    bytes = &bytes[n..];
                    if *remaining == 0 && *end {
                        self.complete = self.written == self.expected;
                    }
                }
            }
        }
        if self.written > self.expected {
            return Err(io::Error::from(io::ErrorKind::InvalidData));
        }
        Ok(())
    }
}
impl<S: AsyncRead + Unpin> AsyncRead for Witness<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_read(cx, buf)
    }
}
impl<S: AsyncWrite + Unpin> AsyncWrite for Witness<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if let Err(e) = self.check() {
            return Poll::Ready(Err(e));
        }
        match Pin::new(&mut self.io).poll_write(cx, buf) {
            Poll::Ready(Ok(n)) => match self.observe(&buf[..n]) {
                Ok(()) => Poll::Ready(Ok(n)),
                Err(e) => Poll::Ready(Err(e)),
            },
            result => result,
        }
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if let Err(e) = self.check() {
            return Poll::Ready(Err(e));
        }
        match Pin::new(&mut self.io).poll_flush(cx) {
            Poll::Ready(Ok(())) => {
                if self.complete
                    && let Some(g) = &mut self.guard
                {
                    g.submitted()
                }
                Poll::Ready(Ok(()))
            }
            result => result,
        }
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_shutdown(cx)
    }
}
pub(crate) async fn cancelled(watch: Option<(Vec<CancellationToken>, CancellationToken)>) {
    let Some((tokens, finished)) = watch else {
        std::future::pending::<()>().await;
        return;
    };
    tokio::select! {biased;
        _=finished.cancelled()=>{},
        _=futures::future::select_all(tokens.iter().map(|t|Box::pin(t.cancelled())))=>{if !finished.is_cancelled(){return}},
    }
    // After the write completes, cancellation cannot interrupt the response transport.
    std::future::pending::<()>().await;
}
