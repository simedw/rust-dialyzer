//! Diagnostic job timeline (`RUST_DIALYZER_JOBTRACE=PATH`): one JSON line per
//! SCC/module job and per pass schedule, for offline critical-path analysis.
use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

struct Trace {
    origin: Instant,
    out: Mutex<std::io::BufWriter<std::fs::File>>,
}
fn trace() -> Option<&'static Trace> {
    static TRACE: OnceLock<Option<Trace>> = OnceLock::new();
    TRACE
        .get_or_init(|| {
            let path = std::env::var_os("RUST_DIALYZER_JOBTRACE")?;
            let file = std::fs::File::create(path).ok()?;
            Some(Trace { origin: Instant::now(), out: Mutex::new(std::io::BufWriter::new(file)) })
        })
        .as_ref()
}
pub fn enabled() -> bool {
    trace().is_some()
}
pub fn now() -> u64 {
    trace().map_or(0, |t| t.origin.elapsed().as_nanos() as u64)
}
pub fn record(value: impl FnOnce() -> serde_json::Value) {
    if let Some(t) = trace() {
        let mut out = t.out.lock().unwrap();
        let _ = writeln!(out, "{}", value());
    }
}
pub fn flush() {
    if let Some(t) = trace() {
        let _ = t.out.lock().unwrap().flush();
    }
}
pub static PASS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
pub fn pass() -> usize {
    PASS.load(std::sync::atomic::Ordering::Relaxed)
}
thread_local! {
    static NOTE: std::cell::RefCell<serde_json::Value> = const { std::cell::RefCell::new(serde_json::Value::Null) };
}
pub fn set_note(value: serde_json::Value) {
    NOTE.with(|n| *n.borrow_mut() = value);
}
pub fn add_note(key: &str, value: serde_json::Value) {
    NOTE.with(|n| {
        if let serde_json::Value::Object(map) = &mut *n.borrow_mut() {
            map.insert(key.into(), value);
        }
    });
}
pub fn take_note() -> serde_json::Value {
    NOTE.with(|n| std::mem::take(&mut *n.borrow_mut()))
}
