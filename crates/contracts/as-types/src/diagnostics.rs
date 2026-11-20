//! Shared asynchronous stdio primitive, not an event queue, history store, or log service.
//! Producers retain unadmitted calls. One admitted line and its accepted byte offset survive
//! cancellation; the next write or explicit flush finishes it without replaying its prefix.
use std::{io, sync::OnceLock};
use tokio::{
    io::{AsyncWrite, AsyncWriteExt},
    sync::Mutex,
};

struct Pending {
    bytes: Vec<u8>,
    accepted: usize,
}
struct State<W> {
    output: W,
    pending: Option<Pending>,
    failure: Option<(io::ErrorKind, String)>,
}
/// One serialized asynchronous output handle with one in-flight diagnostic line.
pub struct DiagnosticWriter<W> {
    state: Mutex<State<W>>,
}
impl<W: AsyncWrite + Unpin> DiagnosticWriter<W> {
    /// Use an existing output; this creates no queue, runtime, or background task.
    pub fn new(output: W) -> Self {
        Self {
            state: Mutex::new(State {
                output,
                pending: None,
                failure: None,
            }),
        }
    }
    /// Admit after the prior line settles. Cancellation retains the admitted line in this writer.
    pub async fn line(&self, mut message: String) -> io::Result<()> {
        let mut state = self.state.lock().await;
        state.finish().await?;
        message.push('\n');
        state.pending = Some(Pending {
            bytes: message.into_bytes(),
            accepted: 0,
        });
        state.finish().await
    }
    /// Confirm actual output completion, or return the original sink failure.
    pub async fn flush(&self) -> io::Result<()> {
        self.state.lock().await.finish().await
    }
}
impl<W: AsyncWrite + Unpin> State<W> {
    async fn finish(&mut self) -> io::Result<()> {
        if let Some((kind, message)) = &self.failure {
            return Err(io::Error::new(*kind, message.clone()));
        }
        let result = async {
            if let Some(pending) = &mut self.pending {
                while pending.accepted < pending.bytes.len() {
                    let count = self
                        .output
                        .write(&pending.bytes[pending.accepted..])
                        .await?;
                    if count == 0 {
                        return Err(io::Error::from(io::ErrorKind::WriteZero));
                    }
                    pending.accepted += count;
                }
            }
            self.output.flush().await
        }
        .await;
        match result {
            Ok(()) => {
                self.pending = None;
                Ok(())
            }
            Err(error) => {
                self.failure = Some((error.kind(), error.to_string()));
                Err(error)
            }
        }
    }
}

fn stderr() -> &'static DiagnosticWriter<tokio::io::Stderr> {
    static STDERR: OnceLock<DiagnosticWriter<tokio::io::Stderr>> = OnceLock::new();
    STDERR.get_or_init(|| DiagnosticWriter::new(tokio::io::stderr()))
}
fn stdout() -> &'static DiagnosticWriter<tokio::io::Stdout> {
    static STDOUT: OnceLock<DiagnosticWriter<tokio::io::Stdout>> = OnceLock::new();
    STDOUT.get_or_init(|| DiagnosticWriter::new(tokio::io::stdout()))
}
/// Write an auxiliary diagnostic after its original result/fact has been handed to its owner.
/// Sink failure stays in the shared writer and is reported by flush_stderr, without replacing
/// the original operation's error. Backpressure yields the calling runtime.
pub async fn record_stderr(message: String) {
    let _result = stderr().line(message).await;
}
/// Drain admitted stderr bytes at the real process shutdown boundary; no timeout or fake success.
pub async fn flush_stderr() -> io::Result<()> {
    stderr().flush().await
}
/// Preserve the original stdout channel for finite host readiness and retirement markers.
pub async fn record_stdout(message: String) {
    let _result = stdout().line(message).await;
}
/// Finish both real stdio handles, preserving an actual failure after both drains execute.
pub async fn flush_stdio() -> io::Result<()> {
    let (error, output) = tokio::join!(stderr().flush(), stdout().flush());
    error.and(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    #[tokio::test(flavor = "current_thread")]
    async fn blocked_sink_keeps_control_live_and_cancelled_prefix_is_not_replayed() {
        let (output, mut input) = tokio::io::duplex(3);
        let writer = DiagnosticWriter::new(output);
        let mut sending = Box::pin(writer.line("abcdef".into()));
        assert!(futures_poll(&mut sending).is_pending());
        assert_eq!(
            tokio::spawn(async { "control-reply" })
                .await
                .expect("same runtime"),
            "control-reply"
        );
        drop(sending);
        let mut received = [0; 7];
        let (written, read) = tokio::join!(writer.flush(), input.read_exact(&mut received));
        written.expect("real drain");
        read.expect("original bytes");
        assert_eq!(&received, b"abcdef\n");
    }
    fn futures_poll<F: std::future::Future>(
        future: &mut std::pin::Pin<Box<F>>,
    ) -> std::task::Poll<F::Output> {
        struct Wake;
        impl std::task::Wake for Wake {
            fn wake(self: std::sync::Arc<Self>) {}
        }
        let waker = std::task::Waker::from(std::sync::Arc::new(Wake));
        future
            .as_mut()
            .poll(&mut std::task::Context::from_waker(&waker))
    }
    #[tokio::test]
    async fn explicit_closed_sink_failure_is_retained_for_final_flush() {
        let (output, input) = tokio::io::duplex(3);
        drop(input);
        let writer = DiagnosticWriter::new(output);
        let first = writer
            .line("actual-error".into())
            .await
            .expect_err("closed sink");
        let final_error = writer.flush().await.expect_err("never claim written");
        assert_eq!(first.kind(), final_error.kind());
    }
}
