//! Dirty page tracking helper trait
//!
//! For now this provides a placeholder interface used by the live-migration
//! layer to query and clear per-VM dirty pages.  A future update will wire
//! this to architecture-specific Accessed/Dirty bits or hardware-assisted
//! logging buffers (e.g. Intel DIRTYLOG or ARM HCR_EL2.FWB).
//!
//! The default no-op implementation makes integration non-intrusive: existing
//! Stage-2 page-table managers may simply rely on the provided default methods
//! until proper tracking is implemented.

extern crate alloc;
use alloc::collections::BTreeSet;
use spin::Mutex;

/// Global dirty bitmap per-VM (keyed by VM handle).
static DIRTY_MAP: Mutex<BTreeSet<u64>> = Mutex::new(BTreeSet::new());

/// Public API called from Stage-2 fault handler to record a dirty page.
pub fn mark_dirty(gpa: u64) {
    let page = gpa & !0xFFFu64;
    DIRTY_MAP.lock().insert(page);
}

/// Software tracker that consults global bitmap.
pub struct SoftDirtyTracker;

impl DirtyPageTracker for SoftDirtyTracker {
    fn collect_dirty_ranges(&mut self, out: &mut [DirtyRange]) -> usize {
        let mut map = DIRTY_MAP.lock();
        if map.is_empty() { return 0; }

        let mut count = 0;
        let mut iter = map.iter().copied();
        if let Some(mut cur_start) = iter.next() {
            let mut cur_pages = 1u64;
            let mut prev = cur_start;
            for addr in iter {
                if addr == prev + 0x1000 {
                    cur_pages += 1;
                } else {
                    if count < out.len() { out[count] = DirtyRange { gpa_start: cur_start, pages: cur_pages }; }
                    count += 1;
                    cur_start = addr; cur_pages = 1;
                }
                prev = addr;
            }
            if count < out.len() { out[count] = DirtyRange { gpa_start: cur_start, pages: cur_pages }; }
            count += 1;
        }
        map.clear();
        count.min(out.len())
    }
}

#[allow(dead_code)]

/// A single dirty page range (start GPA and page count).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirtyRange {
    pub gpa_start: u64,
    pub pages:     u64,
}

/// Trait implemented by architectures that support page-dirty tracking.
/// Returns a *compact* list of ranges to minimise copy overhead during
/// pre-copy live migration.
pub trait DirtyPageTracker {
    /// Fill `out` with dirty ranges, returning the number of valid entries.
    /// Implementations should clear the corresponding hardware bits so that
    /// subsequent calls return *new* dirtied pages only.
    fn collect_dirty_ranges(&mut self, _out: &mut [DirtyRange]) -> usize { 0 }

    /// Convenience helper that returns `true` if no dirty pages detected.
    fn is_clean(&mut self) -> bool {
        let mut buf = [DirtyRange { gpa_start: 0, pages: 0 }; 1];
        self.collect_dirty_ranges(&mut buf) == 0
    }
} 