//! Dropping the analysis structures takes seconds at the end of a run. A
//! one-shot command line run exits right after, so it can opt to leave them
//! for the operating system to reclaim. Library callers keep normal drops.

use std::sync::atomic::{AtomicBool, Ordering};

static SKIP: AtomicBool = AtomicBool::new(false);

/// Called once by a process that exits after its analysis.
pub fn skip_drops_at_exit() {
    SKIP.store(true, Ordering::Relaxed);
}

/// Drops `value`, unless the process opted to skip teardown.
pub fn release<T>(value: T) {
    if SKIP.load(Ordering::Relaxed) {
        std::mem::forget(value);
    } else {
        drop(value);
    }
}
