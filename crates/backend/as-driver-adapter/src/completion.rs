//! Wait for the original endpoint's completion readiness without consuming ordinary records.
use async_io::Async;
use rustix::event::epoll::{self, CreateFlags, Event, EventData, EventFlags};
use std::{fs::File, io::Read, os::fd::OwnedFd};

pub(super) fn poll(file: &File) -> std::io::Result<Async<OwnedFd>> {
    let poll = epoll::create(CreateFlags::CLOEXEC)?;
    epoll::add(&poll, file, EventData::new_u64(0), EventFlags::PRI)?;
    // The only operation on this descriptor is epoll_wait with a zero timeout.
    Async::new_nonblocking(poll)
}

pub(super) async fn read(
    poll: &Async<OwnedFd>,
    file: &File,
    bytes: &mut [u8],
) -> std::io::Result<usize> {
    poll.read_with(|poll| {
        let mut events = [Event {
            flags: EventFlags::empty(),
            data: EventData::new_u64(0),
        }];
        let ready = epoll::wait(
            poll,
            &mut events[..],
            Some(&rustix::event::Timespec::default()),
        )?;
        if ready == 0 {
            return Err(std::io::ErrorKind::WouldBlock.into());
        }
        // This pump is the endpoint's sole reader. Its completion list stays first
        // between the actual PRI observation and this read; ERR/HUP expose real I/O errors.
        let mut file = file;
        file.read(bytes)
    })
    .await
}
