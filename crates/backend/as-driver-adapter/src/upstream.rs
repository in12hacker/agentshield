//! Upstream socket creation in the network domain of the original admission.
use as_types::flow::{FlowIo, UpstreamConnect};
use rustix::{net, thread};
use std::{
    fs::File,
    io,
    net::{SocketAddr, TcpStream},
    os::fd::{AsFd, OwnedFd},
    os::unix::fs::MetadataExt,
    pin::Pin,
    sync::Arc,
};
use tokio::{
    net::TcpSocket,
    sync::{mpsc, oneshot},
};

type SocketReply = oneshot::Sender<io::Result<OwnedFd>>;
type SocketRequest = (net::AddressFamily, SocketReply);

pub(crate) struct Network {
    queue: Option<mpsc::Sender<SocketRequest>>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Network {
    pub(crate) async fn start(
        namespace: File,
        capacity: usize,
        lifetime: tokio_util::task::task_tracker::TaskTrackerToken,
    ) -> io::Result<Arc<Self>> {
        let (queue, mut requests) = mpsc::channel::<SocketRequest>(capacity);
        let (ready, started) = oneshot::channel();
        let worker = std::thread::Builder::new()
            .name("as-egress-netns".into())
            .spawn(move || {
                let _lifetime = lifetime;
                let joined = (|| {
                    thread::move_into_link_name_space(
                        namespace.as_fd(),
                        Some(thread::LinkNameSpaceType::Network),
                    )?;
                    let expected = namespace.metadata()?;
                    let actual = std::fs::metadata("/proc/thread-self/ns/net")?;
                    if (expected.dev(), expected.ino()) != (actual.dev(), actual.ino()) {
                        return Err(io::ErrorKind::PermissionDenied.into());
                    }
                    Ok::<_, io::Error>(())
                })();
                let ok = joined.is_ok();
                if ready.send(joined).is_err() || !ok {
                    return;
                }
                while let Some((family, reply)) = requests.blocking_recv() {
                    if !reply.is_closed() {
                        let _ = reply.send(
                            net::socket_with(
                                family,
                                net::SocketType::STREAM,
                                net::SocketFlags::CLOEXEC | net::SocketFlags::NONBLOCK,
                                None,
                            )
                            .map_err(io::Error::from),
                        );
                    }
                }
            })?;
        let network = Arc::new(Self {
            queue: Some(queue),
            worker: Some(worker),
        });
        started.await.map_err(|_| io::ErrorKind::BrokenPipe)??;
        Ok(network)
    }
}
impl Drop for Network {
    fn drop(&mut self) {
        self.queue.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub(crate) struct Connector {
    pub peer: SocketAddr,
    pub network: Option<Arc<Network>>,
}
#[async_trait::async_trait]
impl UpstreamConnect for Connector {
    async fn connect(&self) -> io::Result<Pin<Box<dyn FlowIo>>> {
        let stream = if let Some(network) = &self.network {
            let family = if self.peer.is_ipv4() {
                net::AddressFamily::INET
            } else {
                net::AddressFamily::INET6
            };
            let (reply, socket) = oneshot::channel();
            network
                .queue
                .as_ref()
                .ok_or(io::ErrorKind::BrokenPipe)?
                .send((family, reply))
                .await
                .map_err(|_| io::ErrorKind::BrokenPipe)?;
            let fd = socket.await.map_err(|_| io::ErrorKind::BrokenPipe)??;
            TcpSocket::from_std_stream(TcpStream::from(fd))
                .connect(self.peer)
                .await?
        } else {
            tokio::net::TcpStream::connect(self.peer).await?
        };
        Ok(Box::pin(stream))
    }
}
