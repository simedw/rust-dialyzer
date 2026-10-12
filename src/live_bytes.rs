//! Live heap bytes for memory audits. Built with `--features live_bytes`,
//! the analyzer counts every allocation and samples the live total into the
//! job trace (`RUST_DIALYZER_JOBTRACE`) as `{"ev":"mem"}` records.
use std::{
    alloc::{GlobalAlloc, Layout},
    cell::Cell,
    sync::atomic::{AtomicIsize, Ordering::Relaxed},
};

static LIVE: AtomicIsize = AtomicIsize::new(0);
static PEAK: AtomicIsize = AtomicIsize::new(0);
/// Per-thread deltas are flushed once they exceed this, so threads rarely
/// touch the shared counters. Totals are accurate to threads × FLUSH.
const FLUSH: isize = 1 << 20;

thread_local! {
    static DELTA: Cell<isize> = const { Cell::new(0) };
}

fn add(bytes: isize) {
    let _ = DELTA.try_with(|delta| {
        let d = delta.get() + bytes;
        if d.abs() < FLUSH {
            delta.set(d);
        } else {
            delta.set(0);
            let live = LIVE.fetch_add(d, Relaxed) + d;
            PEAK.fetch_max(live, Relaxed);
        }
    });
}

/// Live bytes now (0 unless the counting allocator is installed).
pub fn live() -> isize {
    LIVE.load(Relaxed)
}

/// Live bytes now, and the peak since the previous call.
pub fn sample() -> (isize, isize) {
    let live = LIVE.load(Relaxed);
    (live, PEAK.swap(live, Relaxed))
}

pub struct Counting<A>(pub A);
unsafe impl<A: GlobalAlloc> GlobalAlloc for Counting<A> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        add(layout.size() as isize);
        unsafe { self.0.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        add(layout.size() as isize);
        unsafe { self.0.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        add(-(layout.size() as isize));
        unsafe { self.0.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        add(new_size as isize - layout.size() as isize);
        unsafe { self.0.realloc(ptr, layout, new_size) }
    }
}

/// Samples live bytes into the job trace every 250 ms until the process exits.
pub fn start_sampler() {
    if !crate::job_trace::enabled() {
        return;
    }
    std::thread::spawn(|| loop {
        let (live, peak) = sample();
        let t = crate::job_trace::now();
        crate::job_trace::record(|| serde_json::json!({"ev":"mem","t":t,"live":live,"peak":peak}));
        crate::job_trace::flush();
        std::thread::sleep(std::time::Duration::from_millis(250));
    });
}
