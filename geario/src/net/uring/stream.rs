use std::{cell::Cell, cmp, io, mem, num::NonZeroU32, os::fd::AsRawFd, rc::Rc};

use crate::bytes::{BufMut, BytePage, BytePages, BytesMut};
use crate::io::{IoContext, IoTaskStatus};
use crate::rt::Arbiter;
use crate::util::channel::pool;
use ntex_io_uring::{cqueue, opcode, opcode2, types::Fd};
use slab::Slab;
use socket2::Socket;

use super::reactor::{Handler, Reactor, ReactorApi};
use crate::net::helpers::Queue;

/// Worker-local diagnostic counters. Keeping these local avoids a shared
/// atomic cache line on every vectored send across all reactor threads.
/// Snapshots describe the calling worker, not a process-wide aggregate.
pub mod stats {
    use std::cell::Cell;

    std::thread_local! {
        pub(super) static WRITEV_SUBMITS: Cell<u64> = const { Cell::new(0) };
        pub(super) static WRITEV_SHORT: Cell<u64> = const { Cell::new(0) };
        pub(super) static WRITE_CANCELS: Cell<u64> = const { Cell::new(0) };
    }

    /// This worker's (vectored submissions, short writes, write cancels).
    pub fn write_path() -> (u64, u64, u64) {
        (
            WRITEV_SUBMITS.get(),
            WRITEV_SHORT.get(),
            WRITE_CANCELS.get(),
        )
    }

    pub(super) fn inc(c: &'static std::thread::LocalKey<Cell<u64>>) {
        c.set(c.get().wrapping_add(1));
    }
}

#[derive(Clone)]
pub(crate) struct StreamOps(Rc<StreamOpsInner>);

pub(crate) struct StreamCtl {
    id: usize,
    inner: Rc<StreamOpsInner>,
}

pub(crate) struct WeakStreamCtl {
    id: usize,
    inner: Rc<StreamOpsInner>,
}

enum IdType {
    Stream(u32),
    Weak(u32),
}

bitflags::bitflags! {
    #[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
    struct Flags: u8 {
        const RD_CANCELING = 0b0000_0001;
        const RD_REISSUE   = 0b0000_0010;
        const RD_MORE      = 0b0000_0100;
        const WR_CANCELING = 0b0000_1000;
        const WR_REISSUE   = 0b0001_0000;
        const NO_ZC        = 0b0010_0000;
        const DROPPED_PRI  = 0b0100_0000;
        const DROPPED_SEC  = 0b1000_0000;
    }
}

/// Process-wide experimental settings, copied into each worker once. Keep
/// environment parsing and OnceLock checks out of the per-send path.
#[derive(Clone, Copy)]
struct SendConfig {
    zc_size: u32,
    max_write_items: usize,
}

impl SendConfig {
    fn load() -> Self {
        static CONFIG: std::sync::OnceLock<SendConfig> = std::sync::OnceLock::new();
        *CONFIG.get_or_init(|| {
            Self::parse(
                std::env::var("GEARIO_URING_ZC_SIZE").ok().as_deref(),
                std::env::var("GEARIO_URING_MAX_WRITE_ITEMS")
                    .ok()
                    .as_deref(),
            )
        })
    }

    fn parse(zc_size: Option<&str>, max_write_items: Option<&str>) -> Self {
        Self {
            // u32::MAX disables zero-copy for every supported buffer size.
            zc_size: zc_size.and_then(|s| s.parse().ok()).unwrap_or(1536),
            // One page remains useful for attribution experiments. Reject
            // zero and cap overrides at the driver's supported batch size.
            max_write_items: max_write_items
                .and_then(|s| s.parse::<usize>().ok())
                .filter(|&n| n > 0)
                .unwrap_or(MAX_WRITE_ITEMS)
                .min(MAX_WRITE_ITEMS),
        }
    }
}
const IORING_RECVSEND_POLL_FIRST: u16 = 1;

/// Most pages gathered into one vectored send, and the byte ceiling for it.
/// Mirrors the polling driver: one `writev` covers a whole response instead
/// of one `send` per page, which is what let a multi-page response serialize
/// into several round-trips on io_uring.
const MAX_WRITE_ITEMS: usize = 16;
const MAX_WRITE_SIZE: usize = 64 * 1024;

// A bounded synchronous attempt avoids a send SQE and completion for a
// medium single page. Keep small writes and larger batches on their paths.
const DIRECT_SEND_MIN: usize = 8 * 1024;
const DIRECT_SEND_MAX: usize = 16 * 1024;

// Build the final owning container before any iovec pointers are taken.
// The byte budget is a stopping threshold, not a hard cap: the final whole
// page can cross it, preserving the existing page and short-write semantics.
fn gather_pages(wrt: &mut BytePages, max_items: usize) -> (Option<BytePage>, Vec<BytePage>) {
    let Some(first) = wrt.take() else {
        return (None, Vec::new());
    };
    let mut size = first.len();
    if size >= MAX_WRITE_SIZE || max_items == 1 {
        return (Some(first), Vec::new());
    }
    let Some(second) = wrt.take() else {
        return (Some(first), Vec::new());
    };
    let mut pages = Vec::with_capacity(max_items.min(wrt.num_pages() + 2));
    size += second.len();
    pages.push(first);
    pages.push(second);
    while size < MAX_WRITE_SIZE && pages.len() < max_items {
        match wrt.take() {
            Some(page) => {
                size += page.len();
                pages.push(page);
            }
            None => break,
        }
    }
    (None, pages)
}

#[derive(Debug)]
struct StreamItem {
    io: Socket,
    flags: Flags,
    rd_op: Option<NonZeroU32>,
    wr_op: Option<NonZeroU32>,
    ctx: IoContext,
}

#[derive(Debug)]
enum Operation {
    Recv {
        id: usize,
        buf: BytesMut,
    },
    Send {
        id: usize,
        buf: BytePage,
        result: Option<io::Result<usize>>,
    },
    /// A vectored send of several queued pages in one submission. `iovecs`
    /// points into `pages`, so both are held here until the completion; the
    /// Vec owns stable storage for the pages (including inline bytes). Neither
    /// Vec may grow or mutate its elements before the kernel releases it.
    Writev {
        id: usize,
        pages: Vec<BytePage>,
        iovecs: Vec<libc::iovec>,
    },
    Poll {
        id: usize,
    },
    Shutdown {
        tx: Option<pool::Sender<io::Result<()>>>,
    },
    Close {
        id: usize,
    },
    Nop,
}

struct StreamOpsHandler {
    inner: Rc<StreamOpsInner>,
}

#[allow(clippy::box_collection)]
struct StreamOpsInner {
    api: ReactorApi,
    delayed_feed: Queue<IdType>,
    storage: Cell<Option<Box<StreamOpsStorage>>>,
    pool: pool::Pool<io::Result<()>>,
    default_flags: Flags,
}

struct StreamOpsStorage {
    send_config: SendConfig,
    ops: Slab<Option<Operation>>,
    streams: Slab<StreamItem>,
}

impl StreamOps {
    /// Get `StreamOps` instance from the current runtime, or create new one
    pub(crate) fn get(reactor: &Reactor) -> Self {
        Arbiter::get_value(|| {
            let mut inner = None;
            reactor.register(|api| {
                let default_flags = if api.is_supported(opcode::SendZc::CODE) {
                    Flags::empty()
                } else {
                    Flags::NO_ZC
                };
                assert!(
                    api.is_supported(opcode::Close::CODE),
                    "opcode::Close is required for io-uring support"
                );
                assert!(
                    api.is_supported(opcode::Shutdown::CODE),
                    "opcode::Shutdown is required for io-uring support"
                );

                let mut ops = Slab::new();
                ops.insert(Some(Operation::Nop));

                let ops = Rc::new(StreamOpsInner {
                    api,
                    default_flags,
                    delayed_feed: Queue::new(),
                    pool: pool::new(),
                    storage: Cell::new(Some(Box::new(StreamOpsStorage {
                        send_config: SendConfig::load(),
                        ops,
                        streams: Slab::new(),
                    }))),
                });
                inner = Some(ops.clone());
                Box::new(StreamOpsHandler { inner: ops })
            });

            StreamOps(inner.unwrap())
        })
    }

    pub(crate) fn register(
        self,
        io: Socket,
        ctx: IoContext,
        zc: bool,
    ) -> (StreamCtl, WeakStreamCtl) {
        let item = StreamItem {
            io,
            ctx,
            rd_op: None,
            wr_op: None,
            flags: if zc {
                self.0.default_flags
            } else {
                Flags::NO_ZC
            },
        };

        let id = self.0.with(|st| {
            // handle RDHUP event
            let op = opcode::PollAdd::new(item.fd(), libc::POLLRDHUP as u32).build();
            let id = st.streams.insert(item);
            let op_id = st.ops.insert(Some(Operation::Poll { id })) as u32;
            self.0.api.submit(op_id, op);
            id
        });
        (
            StreamCtl {
                id,
                inner: self.0.clone(),
            },
            WeakStreamCtl {
                id,
                inner: self.0.clone(),
            },
        )
    }
}

impl Operation {
    fn shutdown(tx: pool::Sender<io::Result<()>>) -> Self {
        Operation::Shutdown { tx: Some(tx) }
    }
}

/// Put back whatever the socket did not take.
///
/// A send reports how many bytes it accepted, and on a socket that is nearly
/// full that is fewer than were offered. The page was taken out of the write
/// buffer to be submitted, so dropping it here loses the tail: the peer waits
/// forever for bytes the sender believes it has written.
fn requeue_unsent(item: &mut StreamItem, mut buf: BytePage, res: &io::Result<usize>) {
    let sent = cmp::min(res.as_ref().copied().unwrap_or(0), buf.len());
    if sent == buf.len() {
        return;
    }
    buf.advance_to(sent);
    item.ctx.with_write_buf(|pages| pages.prepend(buf));
}

impl Handler for StreamOpsHandler {
    fn canceled(&mut self, user_data: usize) {
        self.inner
            .with(|st| match st.ops.remove(user_data).unwrap() {
                Operation::Recv { id, buf } => {
                    if let Some(item) = st.streams.get_mut(id) {
                        #[cfg(feature = "trace")]
                        log::trace!("{}: Recv canceled {:?}", item.tag(), item.fd());
                        item.rd_op.take();
                        item.flags.remove(Flags::RD_CANCELING);

                        let res = item.ctx.update_read_status(buf, Ok(0));
                        if item.flags.contains(Flags::RD_REISSUE) || res == IoTaskStatus::Io {
                            item.flags.remove(Flags::RD_REISSUE);
                            st.recv(id, false, &self.inner.api);
                        }
                    }
                }
                Operation::Send { id, buf, .. } => {
                    if let Some(item) = st.streams.get_mut(id) {
                        #[cfg(feature = "trace")]
                        log::trace!("{}: Send canceled: {:?}", item.tag(), item.fd());
                        stats::inc(&stats::WRITE_CANCELS);
                        item.ctx.with_write_buf(|pages| pages.prepend(buf));
                        item.wr_op.take();
                        item.flags.remove(Flags::WR_CANCELING);

                        let res = item.ctx.update_write_status(Ok(false));
                        if item.flags.contains(Flags::WR_REISSUE) || res == IoTaskStatus::Io {
                            item.flags.remove(Flags::WR_REISSUE);
                            st.send(id, &self.inner.api);
                        }
                    }
                }
                Operation::Writev {
                    id,
                    pages,
                    iovecs: _,
                } => {
                    if let Some(item) = st.streams.get_mut(id) {
                        stats::inc(&stats::WRITE_CANCELS);
                        item.ctx.with_write_buf(|wrt| {
                            for page in pages.into_iter().rev() {
                                if !page.is_empty() {
                                    wrt.prepend(page);
                                }
                            }
                        });
                        item.wr_op.take();
                        item.flags.remove(Flags::WR_CANCELING);

                        let res = item.ctx.update_write_status(Ok(false));
                        if item.flags.contains(Flags::WR_REISSUE) || res == IoTaskStatus::Io {
                            item.flags.remove(Flags::WR_REISSUE);
                            st.send(id, &self.inner.api);
                        }
                    }
                }
                Operation::Nop
                | Operation::Poll { .. }
                | Operation::Close { .. }
                | Operation::Shutdown { .. } => {}
            });
    }

    #[allow(clippy::too_many_lines)]
    fn completed(&mut self, user_data: usize, flags: u32, res: io::Result<usize>) {
        self.inner.with(|st| {
            match st.ops[user_data].take().unwrap() {
                Operation::Recv { id, mut buf, } => {
                    if let Some(item) = st.streams.get_mut(id) {
                        #[cfg(feature = "trace")]
                        log::trace!(
                            "{}: RcvDone({id}) {res:?} non-empty:{}",
                            item.ctx.tag(),
                            cqueue::sock_nonempty(flags));

                        // reset op reference
                        let _ = item.rd_op.take();

                        // handle WouldBlock
                        if matches!(res, Err(ref e) if e.kind() == io::ErrorKind::WouldBlock || e.raw_os_error() == Some(::libc::EINPROGRESS)) {
                            log::error!("{}: Received WouldBlock {:?}, id: {:?}", item.tag(), res, item.ctx.id());
                            st.recv_more(id, buf, &self.inner.api);
                        } else {
                            if let Ok(size) = res {
                                if size > 0 {
                                    // SAFETY: kernel tells us how many bytes it read
                                    unsafe { buf.advance_mut(size) };
                                } else {
                                    item.ctx.stop(None);
                                }
                            }

                            // handle IORING_CQE_F_SOCK_NONEMPTY flag
                            if cqueue::sock_nonempty(flags) && !(matches!(res, Ok(0) | Err(_))) {
                                // In case of disconnect, sock_nonempty is set to true.
                                // First completion contains data, second Recv(0)
                                // Before receiving Recv(0), POLLRDHUP is triggered
                                // Reactor must read all recv() call before handling
                                // disconnects
                                item.flags.insert(Flags::RD_MORE);
                                st.recv_more(id, buf, &self.inner.api);
                            } else {
                                item.flags.remove(Flags::RD_MORE);
                                if item.ctx.update_read_status(buf, res) == IoTaskStatus::Io {
                                    st.recv(id, self.inner.api.is_new(), &self.inner.api);
                                }
                            }
                        }
                    }
                }
                Operation::Send { id, buf, result } => {
                    if let Some(item) = st.streams.get_mut(id) {
                        #[cfg(feature = "trace")]
                        log::trace!(
                            "{}: Sent({id}) res:{res:?} notif:{:?} more:{:?}",
                            item.ctx.tag(),
                            cqueue::notif(flags),
                            cqueue::more(flags),
                        );

                        if cqueue::notif(flags) {
                            let res = result.unwrap_or(res);
                            requeue_unsent(item, buf, &res);
                            let res = res.map(|n| {
                                if n == 0 {
                                    item.ctx.stop(None);
                                }
                                n > 0
                            });
                            if item.ctx.update_write_status(res) == IoTaskStatus::Io {
                                st.send(id, &self.inner.api);
                            }
                        } else if cqueue::more(flags) {
                            // reset op reference
                            item.wr_op.take();

                            // Only move on when the socket took the whole
                            // page. On a short send the tail has to go back
                            // to the front of the write buffer, and it cannot
                            // until the notify arrives and the kernel is done
                            // with it. Sending the next page first would put
                            // the tail behind it.
                            let complete = matches!(res, Ok(n) if n >= buf.len());
                            if res.is_ok() && complete {
                                st.send(id, &self.inner.api);
                            }
                            // insert op back for "notify" handling
                            st.ops[user_data] = Some(Operation::Send {
                                id,
                                buf,
                                result: Some(res) });
                            // we reuse same op id
                            return
                        } else {
                            // reset op reference
                            item.wr_op.take();

                            requeue_unsent(item, buf, &res);

                            // release buffer and try to send next chunk
                            let res = res.map(|n| {
                                if n == 0 {
                                    item.ctx.stop(None);
                                }
                                n > 0
                            });
                            if item.ctx.update_write_status(res) == IoTaskStatus::Io {
                                st.send(id, &self.inner.api);
                            }
                        }
                    }
                }
                Operation::Writev { id, mut pages, iovecs: _ } => {
                    if let Some(item) = st.streams.get_mut(id) {
                        item.wr_op.take();

                        // Advance the pages by however many bytes went out and
                        // put the rest back at the front of the write buffer,
                        // exactly as the polling driver does after a short
                        // writev.
                        if let Ok(written) = res {
                            let mut left = written;
                            for page in pages.iter_mut() {
                                let n = cmp::min(page.len(), left);
                                page.advance_to(n);
                                left -= n;
                                if left == 0 {
                                    break;
                                }
                            }
                        }
                        let mut requeued = false;
                        item.ctx.with_write_buf(|wrt| {
                            for page in pages.into_iter().rev() {
                                if !page.is_empty() {
                                    wrt.prepend(page);
                                    requeued = true;
                                }
                            }
                        });
                        if requeued {
                            stats::inc(&stats::WRITEV_SHORT);
                        }

                        let res = res.map(|n| {
                            if n == 0 {
                                item.ctx.stop(None);
                            }
                            n > 0
                        });
                        if item.ctx.update_write_status(res) == IoTaskStatus::Io {
                            st.send(id, &self.inner.api);
                        }
                    }
                }
                Operation::Poll { id } => {
                    if let Some(item) = st.streams.get_mut(id)
                        && !item.flags.contains(Flags::RD_MORE) && !item.ctx.is_stopped() {
                            item.ctx.stop(res.err());
                        }
                }
                Operation::Shutdown { tx } => {
                    if let Some(tx) = tx {
                        let _ = tx.send(res.map(|_| ()));
                    }
                }
                Operation::Close { id } => {
                    if st.streams[id].flags.contains(Flags::DROPPED_SEC) {
                        let item = st.streams.remove(id);
                        #[cfg(feature = "trace")]
                        log::trace!("{}: Close({id})", item.ctx.tag());
                        mem::forget(item.io);
                    } else {
                        st.streams[id].flags.insert(Flags::DROPPED_PRI);
                    }
                }
                Operation::Nop => {}
            }
            let _ = st.ops.remove(user_data);
        });
    }

    fn tick(&mut self) {
        self.inner.check_delayed_feed();
    }

    fn cleanup(&mut self) {
        if let Some(v) = self.inner.storage.take() {
            for (_, val) in &v.streams {
                if !val.flags.contains(Flags::DROPPED_PRI) {
                    log::trace!(
                        "{}: Unclosed sockets {:?}",
                        val.ctx.tag(),
                        val.io.peer_addr()
                    );
                }
                let _ = self.inner.api.cancel_all_sync(val.fd());
            }
            self.inner.storage.set(Some(v));
        }
        self.inner.delayed_feed.clear();
    }
}

impl StreamOpsStorage {
    fn recv(&mut self, id: usize, poll_first: bool, api: &ReactorApi) {
        if let Some(item) = self.streams.get_mut(id) {
            if item.rd_op.is_none() {
                #[cfg(feature = "trace")]
                log::trace!("{}: Rcv({id})", item.ctx.tag());

                let mut buf = item.ctx.get_read_buf();
                if !buf.is_empty() {
                    // These are bytes the consumer has not taken yet. This
                    // driver holds the buffer it submits until the read
                    // completes, so handing this one to the kernel would make
                    // those bytes invisible for as long as that takes -- and
                    // when the peer is waiting on a response that depends on
                    // them, that is forever.
                    //
                    // Leave them where the consumer can reach them and read
                    // into a fresh buffer. What comes back is appended after
                    // them, so the order is kept.
                    item.ctx.update_read_status(buf, Ok(0));
                    buf = BytesMut::new();
                    item.ctx.resize_read_buf(&mut buf);
                }
                let s = buf.chunk_mut();
                let buf_ptr = s.as_mut_ptr();
                let buf_len = s.len() as u32;
                let op_id = self.ops.insert(Some(Operation::Recv { id, buf })) as u32;
                item.rd_op = NonZeroU32::new(op_id);

                api.submit_inline(op_id, move |entry| {
                    let op = opcode2::Recv::with(entry, item.fd()).buffer(buf_ptr, buf_len);
                    if poll_first {
                        op.ioprio(IORING_RECVSEND_POLL_FIRST);
                    }
                });
            } else if item.flags.contains(Flags::RD_CANCELING) {
                item.flags.insert(Flags::RD_REISSUE);
            }
        }
    }

    fn recv_more(&mut self, id: usize, mut buf: BytesMut, api: &ReactorApi) {
        if let Some(item) = self.streams.get_mut(id) {
            item.ctx.resize_read_buf(&mut buf);

            let slice = buf.chunk_mut();
            let buf_ptr = slice.as_mut_ptr();
            let buf_len = slice.len() as u32;
            let op_id = self.ops.insert(Some(Operation::Recv { id, buf })) as u32;
            item.rd_op = NonZeroU32::new(op_id);

            api.submit_inline(op_id, move |entry| {
                opcode2::Recv::with(entry, item.fd()).buffer(buf_ptr, buf_len);
            });
        }
    }

    fn send(&mut self, id: usize, api: &ReactorApi) {
        let Some(item) = self.streams.get_mut(id) else {
            return;
        };
        if item.wr_op.is_some() {
            if item.flags.contains(Flags::WR_CANCELING) {
                item.flags.insert(Flags::WR_REISSUE);
            }
            return;
        }

        let config = self.send_config;
        // No allocation for a single page. Once a second page exists, build
        // the final owning Vec directly; its heap storage stays stable while
        // the operation is in flight, even if the operation slab moves.
        let (single, pages, queued) = item.ctx.with_write_buf(|wrt| {
            let (single, pages) = gather_pages(wrt, config.max_write_items);
            let queued = single.is_some() && !wrt.is_empty();
            (single, pages, queued)
        });

        match single {
            None if pages.is_empty() => {}
            Some(mut buf) => {
                let direct = (DIRECT_SEND_MIN..=DIRECT_SEND_MAX).contains(&buf.len()) && !queued;
                if direct {
                    match item.send_nonblocking(&buf) {
                        Ok(written) if written == buf.len() => {
                            item.ctx.update_write_status(Ok(true));
                            return;
                        }
                        Ok(written) => buf.advance_to(written),
                        Err(err) => {
                            item.ctx.update_write_status(Err(err));
                            return;
                        }
                    }
                }
                // A blocked or partial direct send leaves exactly the unsent
                // suffix for the ring. Keep it plain to avoid an extra CQE.
                #[cfg(feature = "trace")]
                log::trace!("{}: Snd({id}) size:{:?}", item.ctx.tag(), buf.len());
                let op_id = self.ops.insert(Some(Operation::Send {
                    id,
                    buf,
                    result: None,
                })) as u32;
                item.wr_op = NonZeroU32::new(op_id);
                let (buf_ptr, buf_len) =
                    if let Some(Operation::Send { buf, .. }) = &self.ops[op_id as usize] {
                        // Safety: `buf` is stored in `self.ops`, which is heap.
                        (unsafe { buf.as_ptr() }, buf.len() as u32)
                    } else {
                        unreachable!()
                    };
                api.submit_inline(op_id, move |entry| {
                    if direct || item.flags.contains(Flags::NO_ZC) || buf_len <= config.zc_size {
                        opcode2::Send::with(entry, item.fd()).buffer(buf_ptr, buf_len);
                    } else {
                        opcode2::SendZc::with(entry, item.fd()).buffer(buf_ptr, buf_len);
                    }
                });
            }
            None => {
                // Several pages go out in one vectored send. Plain writev, not
                // zero-copy: SendZc is single-buffer and does not pay at these
                // sizes anyway.
                stats::inc(&stats::WRITEV_SUBMITS);
                #[cfg(feature = "trace")]
                log::trace!("{}: SndV({id}) pages:{}", item.ctx.tag(), pages.len());
                let iovecs: Vec<libc::iovec> = pages
                    .iter()
                    .map(|p| libc::iovec {
                        // Safety: the page owns its buffer for the op's life.
                        iov_base: unsafe { p.as_ptr() } as *mut libc::c_void,
                        iov_len: p.len(),
                    })
                    .collect();
                let fd = item.fd();
                let op_id = self
                    .ops
                    .insert(Some(Operation::Writev { id, pages, iovecs }))
                    as u32;
                item.wr_op = NonZeroU32::new(op_id);
                let (iov_ptr, iov_len) =
                    if let Some(Operation::Writev { iovecs, .. }) = &self.ops[op_id as usize] {
                        (iovecs.as_ptr(), iovecs.len() as u32)
                    } else {
                        unreachable!()
                    };
                api.submit(op_id, opcode::Writev::new(fd, iov_ptr, iov_len).build());
            }
        }
    }

    fn add_operation(&mut self, op: Operation) -> u32 {
        self.ops.insert(Some(op)) as u32
    }

    fn pause_read(&mut self, id: usize, api: &ReactorApi) {
        let item = &mut self.streams[id];
        if let Some(rd_op) = item.rd_op
            && !item.flags.contains(Flags::RD_CANCELING)
        {
            item.flags.insert(Flags::RD_CANCELING);
            api.cancel(rd_op.get());
            log::trace!("{}: Recv to pause ({:?})", item.tag(), item.fd());
        }
    }

    fn drop_stream(&mut self, id: usize, api: &ReactorApi) {
        // Dropping while `StreamOps` handling event
        let item = &mut self.streams[id];
        log::trace!("{}: Close ({:?})", item.tag(), item.fd());

        let entry = opcode::Close::new(item.fd()).build();
        let op_id = self.add_operation(Operation::Close { id });
        api.submit(op_id, entry);
    }

    fn drop_weak_stream(&mut self, id: usize) {
        // Dropping while `StreamOps` handling event
        let item = &mut self.streams[id];
        if item.flags.contains(Flags::DROPPED_PRI) {
            // io is closed already, remove from storage
            let item = self.streams.remove(id);
            mem::forget(item.io);
        } else {
            item.flags.insert(Flags::DROPPED_SEC);
        }
    }
}

impl StreamOpsInner {
    fn with<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut StreamOpsStorage) -> R,
    {
        let mut storage = self.storage.take().unwrap();
        let result = f(&mut storage);
        self.storage.set(Some(storage));
        result
    }

    fn check_delayed_feed(&self) {
        if let Some(mut storage) = self.storage.take() {
            while let Some(id) = self.delayed_feed.pop() {
                match id {
                    IdType::Stream(id) => storage.drop_stream(id as usize, &self.api),
                    IdType::Weak(id) => storage.drop_weak_stream(id as usize),
                }
            }
            self.storage.set(Some(storage));
        }
    }
}

impl StreamItem {
    fn fd(&self) -> Fd {
        Fd(self.io.as_raw_fd())
    }

    fn tag(&self) -> &'static str {
        self.ctx.tag()
    }

    fn send_nonblocking(&self, buf: &[u8]) -> io::Result<usize> {
        let result = crate::syscall!(
            break libc::send(
                self.io.as_raw_fd(),
                buf.as_ptr().cast(),
                buf.len(),
                libc::MSG_DONTWAIT | libc::MSG_NOSIGNAL,
            )
        )?;
        match result {
            std::task::Poll::Ready(0) => Err(io::Error::from(io::ErrorKind::WriteZero)),
            std::task::Poll::Ready(n) => Ok(n),
            std::task::Poll::Pending => Ok(0),
        }
    }

    /// Write `bufs` to the socket now, outside the ring.
    ///
    /// The caller has established that nothing is queued or in flight ahead
    /// of `bufs`, so a short write is reported as such and the caller buffers
    /// the rest through the ordinary path.
    fn write_bufs(&mut self, bufs: &[io::IoSlice<'_>]) -> io::Result<usize> {
        if bufs.is_empty() {
            return Ok(0);
        }

        let fd = self.io.as_raw_fd();
        let res = if bufs.len() == 1 {
            crate::syscall!(break libc::write(fd, bufs[0].as_ptr().cast(), bufs[0].len()))
        } else {
            let len = i32::try_from(bufs.len()).unwrap_or(i32::MAX);
            crate::syscall!(break libc::writev(fd, bufs.as_ptr().cast(), len))
        }?;

        match res {
            std::task::Poll::Ready(0) => {
                self.ctx.stop(None);
                Err(io::Error::from(io::ErrorKind::WriteZero))
            }
            std::task::Poll::Ready(n) => Ok(n),
            // Full socket; the caller buffers, which queues a send.
            std::task::Poll::Pending => Ok(0),
        }
    }
}

impl StreamCtl {
    pub(crate) async fn shutdown(&self) -> io::Result<()> {
        self.inner
            .with(|storage| {
                storage.pause_read(self.id, &self.inner.api);
                #[cfg(feature = "trace")]
                log::trace!(
                    "{}: Shutdown ({:?})",
                    storage.streams[self.id].ctx.tag(),
                    self.id
                );
                let fd = storage.streams[self.id].fd();
                let (tx, rx) = self.inner.pool.channel();
                let op_id = storage.add_operation(Operation::shutdown(tx));
                self.inner
                    .api
                    .submit(op_id, opcode::Shutdown::new(fd, libc::SHUT_RDWR).build());
                rx
            })
            .await
            .map_err(|_| io::Error::other("gone"))
            .and_then(|item| item)
    }

    pub(crate) fn resume_read(&self) {
        self.inner
            .with(|st| st.recv(self.id, false, &self.inner.api));
    }

    pub(crate) fn resume_write(&self) {
        self.inner.with(|st| st.send(self.id, &self.inner.api));
    }

    pub(crate) fn pause_read(&self) {
        self.inner
            .with(|storage| storage.pause_read(self.id, &self.inner.api));
    }
}

impl Drop for StreamCtl {
    fn drop(&mut self) {
        if let Some(mut storage) = self.inner.storage.take() {
            storage.drop_stream(self.id, &self.inner.api);
            self.inner.storage.set(Some(storage));
        } else {
            self.inner.delayed_feed.push(IdType::Stream(self.id as u32));
        }
    }
}

impl WeakStreamCtl {
    pub(crate) fn with_io<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&Socket) -> R,
    {
        self.inner.with(|storage| f(&storage.streams[self.id].io))
    }

    /// Write `bufs` straight to the socket.
    ///
    /// `None` when the storage is busy, or when a send is already in flight:
    /// the driver hands the write buffer to the ring, so an in-flight send
    /// holds bytes that are no longer in the buffer and must go out first.
    pub(crate) fn write_bufs(&self, bufs: &[io::IoSlice<'_>]) -> Option<io::Result<usize>> {
        let mut storage = self.inner.storage.take()?;
        let res = storage
            .streams
            .get_mut(self.id)
            .filter(|item| {
                item.wr_op.is_none()
                    && !item.flags.contains(Flags::WR_CANCELING)
                    && !item.ctx.is_stopped()
            })
            .map(|item| item.write_bufs(bufs));
        self.inner.storage.set(Some(storage));
        res
    }
}

impl Drop for WeakStreamCtl {
    fn drop(&mut self) {
        if let Some(mut storage) = self.inner.storage.take() {
            storage.drop_weak_stream(self.id);
            self.inner.storage.set(Some(storage));
        } else {
            self.inner.delayed_feed.push(IdType::Weak(self.id as u32));
        }
    }
}

#[cfg(test)]
mod send_config_tests {
    use super::{MAX_WRITE_ITEMS, MAX_WRITE_SIZE, SendConfig, gather_pages};
    use crate::bytes::BytePages;

    #[test]
    fn diagnostic_counters_do_not_contend_or_mix_workers() {
        let before = super::stats::write_path();
        std::thread::spawn(|| {
            assert_eq!(super::stats::write_path(), (0, 0, 0));
            super::stats::inc(&super::stats::WRITEV_SUBMITS);
            assert_eq!(super::stats::write_path(), (1, 0, 0));
        })
        .join()
        .unwrap();
        assert_eq!(super::stats::write_path(), before);
    }

    #[test]
    fn gather_keeps_single_page_allocation_free_and_preserves_tail_order() {
        let mut queue = BytePages::default();
        let (single, pages) = gather_pages(&mut queue, 16);
        assert!(single.is_none());
        assert_eq!(pages.capacity(), 0);
        queue.append(vec![1u8; 4]);
        let (single, pages) = gather_pages(&mut queue, 16);
        assert_eq!(single.unwrap().as_ref(), &[1; 4]);
        assert_eq!(pages.capacity(), 0);
        for byte in 1..=4u8 {
            queue.append(vec![byte; 4]);
        }
        let (single, pages) = gather_pages(&mut queue, 1);
        assert_eq!(single.unwrap().as_ref(), &[1; 4]);
        assert_eq!(pages.capacity(), 0);
        let (single, pages) = gather_pages(&mut queue, 2);
        assert!(single.is_none());
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].as_ref(), &[2; 4]);
        assert_eq!(pages[1].as_ref(), &[3; 4]);
        assert_eq!(queue.take().unwrap().as_ref(), &[4; 4]);
        assert!(queue.is_empty());
    }

    #[test]
    fn gather_stops_at_the_byte_budget_without_consuming_the_tail() {
        let mut queue = BytePages::default();
        for _ in 0..3 {
            queue.append(vec![7u8; MAX_WRITE_SIZE / 2]);
        }
        let (single, pages) = gather_pages(&mut queue, 16);
        assert!(single.is_none());
        assert_eq!(pages.len(), 2);
        assert_eq!(queue.len(), MAX_WRITE_SIZE / 2);
        let (single, pages) = gather_pages(&mut queue, 16);
        assert_eq!(single.unwrap().len(), MAX_WRITE_SIZE / 2);
        assert_eq!(pages.capacity(), 0);
    }

    #[test]
    fn experimental_settings_preserve_safe_batch_limits() {
        for value in [None, Some("0"), Some("invalid"), Some("-1")] {
            assert_eq!(
                SendConfig::parse(None, value).max_write_items,
                MAX_WRITE_ITEMS
            );
        }
        assert_eq!(SendConfig::parse(None, Some("1")).max_write_items, 1);
        assert_eq!(SendConfig::parse(None, Some("4")).max_write_items, 4);
        assert_eq!(
            SendConfig::parse(None, Some("99999")).max_write_items,
            MAX_WRITE_ITEMS
        );
        assert_eq!(
            SendConfig::parse(Some("4294967295"), None).zc_size,
            u32::MAX
        );
        assert_eq!(SendConfig::parse(Some("0"), None).zc_size, 0);
        assert_eq!(SendConfig::parse(Some("invalid"), None).zc_size, 1536);
    }
}

#[cfg(all(test, feature = "neon-uring", not(feature = "neon-polling")))]
mod write_path_tests {
    use super::*;
    use crate::service::cfg::SharedCfg;
    use crate::util::time::{Millis, sleep, timeout};
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    use std::time::Duration;

    #[crate::test]
    async fn short_direct_send_submits_only_the_unsent_suffix() {
        let (writer, mut reader) = UnixStream::pair().unwrap();
        socket2::SockRef::from(&writer)
            .set_send_buffer_size(4096)
            .unwrap();
        let io = crate::net::from_unix_stream(writer, SharedCfg::new("DIRECT").into()).unwrap();
        let ops: StreamOps = Arbiter::get_value(|| panic!("missing io_uring stream handler"));
        let bytes: Vec<_> = (0..DIRECT_SEND_MAX).map(|n| (n ^ (n >> 8)) as u8).collect();
        io.get_ref()
            .encode_bytes(crate::bytes::Bytes::copy_from_slice(&bytes))
            .unwrap();

        ops.0.with(|st| {
            let id = st.streams.iter().next().unwrap().0;
            st.send(id, &ops.0.api);
            let op = st.streams[id].wr_op.unwrap();
            let Some(Operation::Send { buf, .. }) = &st.ops[op.get() as usize] else {
                panic!("expected a single send for the remaining bytes");
            };
            assert!(
                !buf.is_empty() && buf.len() < bytes.len(),
                "expected a short write"
            );
            assert_eq!(&buf[..], &bytes[bytes.len() - buf.len()..]);
        });

        io.get_ref().encode_bytes(&b"after"[..]).unwrap();
        let mut expected = bytes;
        expected.extend_from_slice(b"after");
        reader
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let receive = crate::rt::spawn_blocking(move || {
            let mut actual = vec![0; expected.len()];
            reader.read_exact(&mut actual).unwrap();
            assert!(
                actual == expected,
                "short write lost, reordered or duplicated bytes"
            );
        });
        timeout(Millis(4000), async {
            io.flush(true).await.unwrap();
            io.shutdown().await.unwrap();
            receive.await.unwrap();
        })
        .await
        .expect("buffered suffix did not finish");
    }

    #[crate::test]
    async fn canceled_writev_requeues_every_byte_before_later_writes() {
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        writer.set_nonblocking(true).unwrap();
        socket2::SockRef::from(&writer)
            .set_send_buffer_size(8192)
            .unwrap();
        // Fill the socket before handing it to the reactor. The peer cannot
        // drain it until the actual cancellation CQE has been observed.
        let mut filled = 0;
        loop {
            match writer.write(&[0x55; 4096]) {
                Ok(n) => filled += n,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => panic!("{e}"),
            }
        }
        let io = crate::net::from_unix_stream(writer, SharedCfg::new("CANCEL").into()).unwrap();
        let ops: StreamOps = Arbiter::get_value(|| panic!("missing io_uring stream handler"));
        let bytes: Vec<_> = (0..32768_u32).map(|n| (n ^ (n >> 8)) as u8).collect();
        io.get_ref()
            .with_write_buf(|buf| buf.extend_from_slice(&bytes))
            .unwrap();
        let (id, op) = ops.0.with(|st| {
            let id = st.streams.iter().next().unwrap().0;
            st.send(id, &ops.0.api);
            let op = st.streams[id].wr_op.unwrap();
            assert!(matches!(
                st.ops[op.get() as usize],
                Some(Operation::Writev { .. })
            ));
            st.streams[id].flags.insert(Flags::WR_CANCELING);
            (id, op)
        });
        io.get_ref()
            .with_write_buf(|buf| buf.extend_from_slice(b"after"))
            .unwrap();

        let before = stats::write_path().2;
        timeout(Millis(2000), async {
            loop {
                if stats::write_path().2 != before {
                    break;
                }
                ops.0.with(|st| assert_eq!(st.streams[id].wr_op, Some(op)));
                ops.0.api.cancel(op.get());
                sleep(Millis(10)).await;
            }
        })
        .await
        .expect("Writev did not return ECANCELED");

        let mut expected = vec![0x55; filled];
        expected.extend_from_slice(&bytes);
        expected.extend_from_slice(b"after");
        reader
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let receive = crate::rt::spawn_blocking(move || {
            let mut actual = vec![0; expected.len()];
            reader.read_exact(&mut actual).unwrap();
            assert!(
                actual == expected,
                "canceled bytes were lost, reordered or duplicated"
            );
        });
        timeout(Millis(4000), async {
            io.flush(true).await.unwrap();
            io.shutdown().await.unwrap();
            receive.await.unwrap();
        })
        .await
        .expect("reissued write did not finish");
    }
}
