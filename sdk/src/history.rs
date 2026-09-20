//! Bounded rolling history for plugin-side trend graphs.
//!
//! Two host constraints drive this design:
//!
//! * There is **no tick callback**. A plugin only runs inside `render_widget`,
//!   which the host calls every frame (~30 fps). Pushing one sample per call
//!   would fill a buffer in seconds and make every graph a flatline of the
//!   last second, so samples are gated on the wall clock (which *is*
//!   available in the sandbox).
//! * Linear memory persists between calls but is never reclaimed, so history
//!   must be **fixed capacity**, never a growing Vec.

/// Fixed-capacity ring buffer with wall-clock rate limiting.
pub struct History<const N: usize> {
    buf: [f64; N],
    head: usize,
    len: usize,
    last_ms: u64,
    interval_ms: u64,
}

impl<const N: usize> History<N> {
    /// `interval_ms` is the minimum spacing between retained samples.
    pub const fn new(interval_ms: u64) -> Self {
        History {
            buf: [0.0; N],
            head: 0,
            len: 0,
            last_ms: 0,
            interval_ms,
        }
    }

    /// Record `value` if the interval has elapsed. Returns true when stored.
    pub fn push(&mut self, value: f64) -> bool {
        let now = now_ms();
        if self.len > 0 && now.saturating_sub(self.last_ms) < self.interval_ms {
            return false;
        }
        self.last_ms = now;
        self.buf[self.head] = value;
        self.head = (self.head + 1) % N;
        self.len = (self.len + 1).min(N);
        true
    }

    /// Up to `n` most recent samples, oldest first. Only real samples are
    /// returned, so a young history renders as a short trace rather than a
    /// long run of zeros.
    pub fn recent(&self, n: usize) -> Vec<f64> {
        let take = n.min(self.len);
        (0..take)
            .map(|i| self.buf[(self.head + N - take + i) % N])
            .collect()
    }

    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn capacity(&self) -> usize {
        N
    }
    pub fn last(&self) -> Option<f64> {
        (self.len > 0).then(|| self.buf[(self.head + N - 1) % N])
    }
    pub fn max(&self) -> f64 {
        self.recent(N).into_iter().fold(0.0, f64::max)
    }
    pub fn mean(&self) -> f64 {
        if self.len == 0 {
            return 0.0;
        }
        self.recent(N).iter().sum::<f64>() / self.len as f64
    }

    /// Test seam: push without consulting the clock.
    #[doc(hidden)]
    pub fn push_unthrottled(&mut self, value: f64) {
        self.buf[self.head] = value;
        self.head = (self.head + 1) % N;
        self.len = (self.len + 1).min(N);
    }
}

/// Milliseconds since the epoch. `SystemTime` is one of the few host
/// facilities that works inside the sandbox (verified by capability probe).
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_keeps_newest_and_is_bounded() {
        let mut h = History::<4>::new(0);
        assert!(h.is_empty());
        for i in 1..=6 {
            h.push_unthrottled(i as f64);
        }
        assert_eq!(h.len(), 4);
        assert_eq!(h.capacity(), 4);
        assert_eq!(h.recent(10), vec![3.0, 4.0, 5.0, 6.0]);
        assert_eq!(h.recent(2), vec![5.0, 6.0]);
        assert_eq!(h.last(), Some(6.0));
        assert_eq!(h.max(), 6.0);
    }

    #[test]
    fn young_history_returns_only_real_samples() {
        let mut h = History::<8>::new(0);
        h.push_unthrottled(1.0);
        h.push_unthrottled(2.0);
        assert_eq!(h.recent(8), vec![1.0, 2.0]);
        assert_eq!(h.mean(), 1.5);
    }

    #[test]
    fn interval_throttles_writes() {
        // A long interval means the second push within the same instant is
        // dropped — this is what stops a 30 fps render loop flooding history.
        let mut h = History::<8>::new(60_000);
        assert!(h.push(1.0), "first sample always stored");
        assert!(!h.push(2.0), "second sample inside the interval is dropped");
        assert_eq!(h.recent(8), vec![1.0]);
    }

    #[test]
    fn zero_interval_always_stores() {
        let mut h = History::<4>::new(0);
        assert!(h.push(1.0));
        assert!(h.push(2.0));
        assert_eq!(h.len(), 2);
    }
}
