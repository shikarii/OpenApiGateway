use std::sync::atomic::{AtomicU64, Ordering};

/// Monotonic xDS snapshot version counter.
#[derive(Debug, Default)]
pub(crate) struct VersionCounter {
    next: AtomicU64,
}

impl VersionCounter {
    pub fn new(initial: u64) -> Self {
        Self {
            next: AtomicU64::new(initial),
        }
    }

    pub fn increment(&self) -> u64 {
        self.next.fetch_add(1, Ordering::Relaxed) + 1
    }
}
