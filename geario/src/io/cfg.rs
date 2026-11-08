use std::cell::UnsafeCell;

use crate::bytes::{BytePageSize, BytesMut, buf::BufMut};
use crate::service::cfg::{CfgContext, Configuration};
use crate::util::{time::Millis, time::Seconds};

const DEFAULT_CACHE_SIZE: usize = 128;
// Keep space for a full 16 KiB of payload in the default read allocation.
const DEFAULT_READ_HIGH: usize = BytePageSize::Size16.capacity();
const DEFAULT_HIGH: usize = 16 * 1024 - 24;
const DEFAULT_LOW: usize = 512 + 24;
const DEFAULT_HALF: usize = (16 * 1024 - 24) / 2;

thread_local! {
    static CACHE: LocalCache = LocalCache::new();
}

#[derive(Debug)]
/// Base io configuration
pub struct IoConfig {
    connect_timeout: Millis,
    keepalive_timeout: Seconds,
    disconnect_timeout: Seconds,
    frame_read_rate: Option<FrameReadRate>,

    // io read/write cache and params
    read_buf: BufConfig,
    write_buf: BufConfig,
    write_page_size: BytePageSize,
    write_buf_threshold: usize,

    // shared config
    pub(crate) config: CfgContext,
}

impl Default for IoConfig {
    fn default() -> Self {
        IoConfig::new()
    }
}

impl Configuration for IoConfig {
    const NAME: &str = "IO Configuration";

    fn ctx(&self) -> &CfgContext {
        &self.config
    }

    fn set_ctx(&mut self, ctx: CfgContext) {
        self.read_buf.idx = ctx.id();
        self.write_buf.idx = ctx.id();
        self.config = ctx;
    }
}

#[derive(Copy, Clone, Debug)]
pub struct FrameReadRate {
    pub timeout: Seconds,
    pub max_timeout: Seconds,
    pub rate: u32,
}

#[derive(Copy, Clone, Debug)]
pub struct BufConfig {
    pub high: usize,
    pub low: usize,
    pub half: usize,
    idx: usize,
    first: bool,
    cache_size: usize,
    recycle_read_page: bool,
}

impl IoConfig {
    #[inline]
    #[must_use]
    /// Create new config object
    pub fn new() -> IoConfig {
        let config = CfgContext::default();
        let idx = config.id();

        IoConfig {
            config,
            connect_timeout: Millis::ZERO,
            keepalive_timeout: Seconds(0),
            disconnect_timeout: Seconds(1),
            frame_read_rate: None,

            read_buf: BufConfig {
                idx,
                high: DEFAULT_READ_HIGH,
                low: DEFAULT_LOW,
                half: DEFAULT_READ_HIGH / 2,
                first: true,
                cache_size: DEFAULT_CACHE_SIZE,
                recycle_read_page: true,
            },
            write_buf: BufConfig {
                idx,
                high: DEFAULT_HIGH,
                low: DEFAULT_LOW,
                half: DEFAULT_HALF,
                first: false,
                cache_size: DEFAULT_CACHE_SIZE,
                recycle_read_page: false,
            },
            write_page_size: BytePageSize::Size16,
            write_buf_threshold: BytePageSize::Size16.half_capacity(),
        }
    }

    #[inline]
    /// Get tag
    pub fn tag(&self) -> &str {
        self.config.tag()
    }

    #[inline]
    /// Get connect timeout
    pub fn connect_timeout(&self) -> Millis {
        self.connect_timeout
    }

    #[inline]
    /// Get keep-alive timeout
    pub fn keepalive_timeout(&self) -> Seconds {
        self.keepalive_timeout
    }

    #[inline]
    /// Get disconnect timeout
    pub fn disconnect_timeout(&self) -> Seconds {
        self.disconnect_timeout
    }

    #[inline]
    /// Get frame read params
    pub fn frame_read_rate(&self) -> Option<&FrameReadRate> {
        self.frame_read_rate.as_ref()
    }

    #[inline]
    /// Get read buffer parameters
    pub fn read_buf(&self) -> &BufConfig {
        &self.read_buf
    }

    #[inline]
    /// Get write buffer parameters
    pub fn write_buf(&self) -> &BufConfig {
        &self.write_buf
    }

    #[inline]
    /// Get write page size
    pub fn write_page_size(&self) -> BytePageSize {
        self.write_page_size
    }

    #[inline]
    /// The write buffer threshold that triggers earlier sending.
    pub fn write_buf_threshold(&self) -> usize {
        self.write_buf_threshold
    }

    /// Set connect timeout in seconds.
    ///
    /// To disable timeout set value to 0.
    ///
    /// By default connect timeout is disabled.
    #[must_use]
    pub fn set_connect_timeout<T: Into<Millis>>(mut self, timeout: T) -> Self {
        self.connect_timeout = timeout.into();
        self
    }

    /// Set keep-alive timeout in seconds.
    ///
    /// To disable timeout set value to 0.
    ///
    /// By default keep-alive timeout is disabled.
    #[must_use]
    pub fn set_keepalive_timeout<T: Into<Seconds>>(mut self, timeout: T) -> Self {
        self.keepalive_timeout = timeout.into();
        self
    }

    /// Set connection disconnect timeout.
    ///
    /// Defines a timeout for disconnect connection. If a disconnect procedure does not complete
    /// within this time, the connection get dropped.
    ///
    /// To disable timeout set value to 0.
    ///
    /// By default disconnect timeout is set to 1 seconds.
    #[must_use]
    pub fn set_disconnect_timeout<T: Into<Seconds>>(mut self, timeout: T) -> Self {
        self.disconnect_timeout = timeout.into();
        self
    }

    /// Set read rate parameters for single frame.
    ///
    /// Set read timeout, max timeout and rate for reading payload. If the client
    /// sends `rate` amount of data within `timeout` period of time, extend timeout by `timeout` seconds.
    /// But no more than `max_timeout` timeout.
    ///
    /// By default frame read rate is disabled.
    #[must_use]
    pub fn set_frame_read_rate(
        mut self,
        timeout: Seconds,
        max_timeout: Seconds,
        rate: u32,
    ) -> Self {
        self.frame_read_rate = Some(FrameReadRate {
            timeout,
            max_timeout,
            rate,
        });
        self
    }

    /// Set read buffer parameters.
    ///
    /// By default high watermark is set to 16Kb, low watermark 1kb.
    #[must_use]
    pub fn set_read_buf(
        mut self,
        high_watermark: usize,
        low_watermark: usize,
        cache_size: usize,
    ) -> Self {
        // Explicit read-buffer sizing retains the per-config cache policy.
        self.read_buf.recycle_read_page = false;
        self.read_buf.cache_size = cache_size;
        self.read_buf.high = high_watermark;
        self.read_buf.low = low_watermark;
        self.read_buf.half = high_watermark >> 1;
        self
    }

    /// Set write buffer page size.
    ///
    /// By default page size is set to 16kb.
    #[must_use]
    pub fn set_write_page_size(mut self, size: BytePageSize) -> Self {
        self.write_page_size = size;
        self
    }

    /// Sets the write buffer threshold.
    ///
    /// The app encodes data in response to incoming data,
    /// continuing to fill the write buffer until all data
    /// has been processed. Only then can the runtime wake
    /// the write task to send the buffered data.
    ///
    /// By that time, the buffer may have accumulated a large
    /// amount of data, causing it to be sent in large bursts,
    /// which introduces latency. To prevent this behavior and
    /// flatten data delivery to the peer, geario's io can initiate
    /// out-of-order writes based on a configured threshold.
    ///
    /// Set `0` to disable send-buf.
    #[must_use]
    pub fn set_write_buf_threshold(mut self, size: usize) -> Self {
        self.write_buf_threshold = size;
        self
    }

    /// Set write buffer parameters.
    ///
    /// By default high watermark is set to 16Kb, low watermark 1kb.
    #[must_use]
    pub fn set_write_buf(
        mut self,
        high_watermark: usize,
        low_watermark: usize,
        cache_size: usize,
    ) -> Self {
        self.write_buf.cache_size = cache_size;
        self.write_buf.high = high_watermark;
        self.write_buf.low = low_watermark;
        self.write_buf.half = high_watermark >> 1;
        self
    }
}

impl BufConfig {
    #[inline]
    /// Get buffer
    pub fn get(&self) -> BytesMut {
        if self.recycle_read_page {
            // A codec can take the entire read page. Its original capacity
            // then disappears from the mutable tail, so only the backing
            // allocation can return to the pool after its final reference.
            BytesMut::with_page_size(BytePageSize::Size16)
        } else if let Some(buf) =
            CACHE.with(|c| c.with(self.idx, self.first, |c: &mut Vec<_>| c.pop()))
        {
            buf
        } else {
            BytesMut::with_capacity(self.high)
        }
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn take_cached_read_burst(&self) -> Option<BytesMut> {
        self.recycle_read_page
            .then(|| BytesMut::take_cached_page(BytePageSize::Size64))
            .flatten()
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn resize_read_burst(&self, buf: &mut BytesMut) {
        if self.recycle_read_page
            && buf.capacity() == DEFAULT_READ_HIGH
            && buf.remaining_mut() < self.low
        {
            // The socket still has data and the first page is nearly full.
            // Grow once instead of copying the accumulated prefix at each
            // 16 KiB boundary. The page is reclaimed after its last slice.
            let mut burst = BytesMut::with_page_size(BytePageSize::Size64);
            burst.extend_from_slice(buf);
            *buf = burst;
        } else {
            self.resize(buf);
        }
    }

    #[inline]
    /// Resize buffer
    pub fn resize(&self, buf: &mut BytesMut) {
        if buf.remaining_mut() < self.low {
            self.resize_min(buf, self.high);
        }
    }

    #[inline]
    /// Resize buffer
    pub fn resize_min(&self, buf: &mut BytesMut, size: usize) {
        let mut avail = buf.remaining_mut();
        if avail < size {
            let mut new_cap = buf.capacity();
            while avail < size {
                avail += self.high;
                new_cap += self.high;
            }
            buf.reserve_capacity(new_cap);
        }
    }

    #[inline]
    /// Release buffer, buf must be allocated from this pool
    pub fn release(&self, mut buf: BytesMut) {
        if self.recycle_read_page {
            // The page pool owns recycling, including when a slice outlives
            // this mutable tail.
            return;
        }
        let cap = buf.capacity();
        if cap > self.low && cap <= self.high {
            CACHE.with(|c| {
                c.with(self.idx, self.first, |v: &mut Vec<_>| {
                    if v.len() < self.cache_size {
                        buf.clear();
                        v.push(buf);
                    }
                });
            });
        }
    }
}

struct LocalCache {
    cache: UnsafeCell<Vec<(Vec<BytesMut>, Vec<BytesMut>)>>,
}

impl LocalCache {
    fn new() -> Self {
        Self {
            cache: UnsafeCell::new(Vec::with_capacity(16)),
        }
    }

    fn with<F, R>(&self, idx: usize, first: bool, f: F) -> R
    where
        F: FnOnce(&mut Vec<BytesMut>) -> R,
    {
        let cache = unsafe { &mut *self.cache.get() };

        while cache.len() <= idx {
            cache.push((Vec::new(), Vec::new()));
        }
        if first {
            f(&mut cache[idx].0)
        } else {
            f(&mut cache[idx].1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn read_ahead_borrows_only_a_released_default_page() {
        std::thread::spawn(|| {
            let default = IoConfig::default();
            let read = default.read_buf();
            assert!(read.take_cached_read_burst().is_none());

            let mut page = BytesMut::with_page_size(BytePageSize::Size64);
            page.resize(BytePageSize::Size64.capacity(), 1);
            let ptr = page.as_ptr();
            let slice = page.split_to(page.len());
            read.release(page);
            assert!(read.take_cached_read_burst().is_none());
            drop(slice);

            let custom = IoConfig::default().set_read_buf(DEFAULT_READ_HIGH, 536, 128);
            assert!(custom.read_buf().take_cached_read_burst().is_none());

            let borrowed = read.take_cached_read_burst().expect("released page");
            assert_eq!(borrowed.as_ptr(), ptr);
            assert_eq!(borrowed.capacity(), BytePageSize::Size64.capacity());
            assert!(borrowed.is_empty());
        })
        .join()
        .unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn burst_growth_preserves_the_prefix_and_custom_capacity_policy() {
        let default = IoConfig::default();
        let cfg = default.read_buf();
        let mut page = cfg.get();
        page.resize(DEFAULT_READ_HIGH, 0x5a);
        cfg.resize_read_burst(&mut page);
        assert_eq!(page.capacity(), BytePageSize::Size64.capacity());
        assert_eq!(page.len(), DEFAULT_READ_HIGH);
        assert!(page.iter().all(|&byte| byte == 0x5a));

        let custom = IoConfig::default().set_read_buf(DEFAULT_READ_HIGH, 536, 0);
        let cfg = custom.read_buf();
        let mut page = cfg.get();
        page.resize(DEFAULT_READ_HIGH, 0xa5);
        cfg.resize_read_burst(&mut page);
        assert_eq!(page.capacity(), DEFAULT_READ_HIGH * 2);
        assert_eq!(page.len(), DEFAULT_READ_HIGH);
        assert!(page.iter().all(|&byte| byte == 0xa5));
    }

    #[test]
    fn default_read_page_waits_for_its_last_slice_before_recycling() {
        let config = IoConfig::default();
        let read = config.read_buf();
        let mut page = read.get();
        page.resize(DEFAULT_READ_HIGH, 0x31);
        let original = page.as_ptr();
        let slice = page.split_to(page.len());
        read.release(page);

        let other = read.get();
        assert_ne!(original, other.as_ptr());
        assert!(slice.iter().all(|&byte| byte == 0x31));
        drop(slice);

        let recycled = read.get();
        assert_eq!(recycled.as_ptr(), original);
        assert!(recycled.is_empty());
        assert_eq!(recycled.capacity(), DEFAULT_READ_HIGH);
    }
}
