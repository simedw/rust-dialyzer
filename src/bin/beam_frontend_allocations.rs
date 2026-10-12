//! Allocation diagnostics only: counters perturb timings, so benchmark separately.
use rust_dialyzer_poc::beam_frontend::pipeline::{ContextSource, Emission, load_emission};
use serde_json::{Value, json};
use std::{
    alloc::{GlobalAlloc, Layout},
    sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed},
};
struct Counted;
static ENABLED: AtomicBool = AtomicBool::new(false);
static ALLOCS: AtomicU64 = AtomicU64::new(0);
static REALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
fn record(size: usize, realloc: bool) {
    if ENABLED.load(Relaxed) {
        if realloc {
            REALLOCS.fetch_add(1, Relaxed);
        } else {
            ALLOCS.fetch_add(1, Relaxed);
        }
        BYTES.fetch_add(size as u64, Relaxed);
    }
}
unsafe impl GlobalAlloc for Counted {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        record(l.size(), false);
        unsafe { mimalloc::MiMalloc.alloc(l) }
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        record(l.size(), false);
        unsafe { mimalloc::MiMalloc.alloc_zeroed(l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        record(n, true);
        unsafe { mimalloc::MiMalloc.realloc(p, l, n) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { mimalloc::MiMalloc.dealloc(p, l) }
    }
}
#[global_allocator]
static ALLOCATOR: Counted = Counted;
fn run() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 2 {
        return Err("usage: beam_frontend_allocations MANIFEST.json reference|direct|fused".into());
    }
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(&args[0]).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let paths = manifest["beam_files"]
        .as_array()
        .ok_or("beam_files missing")?
        .iter()
        .map(|x| {
            x.as_str()
                .map(str::to_owned)
                .ok_or("BEAM path must be string")
        })
        .collect::<Result<Vec<_>, _>>()?;
    let read = |key: &str| {
        std::fs::read(
            manifest[key]
                .as_str()
                .ok_or_else(|| format!("{key} missing"))?,
        )
        .map_err(|e| e.to_string())
    };
    let typesig = read("dialyzer_typesig_beam")?;
    let contracts = read("dialyzer_contracts_beam")?;
    let emission = match args[1].as_str() {
        "reference" => Emission::Reference,
        "direct" => Emission::Direct,
        "fused" => Emission::Fused,
        _ => return Err("unknown emission mode".into()),
    };
    ENABLED.store(true, Relaxed);
    let loaded = load_emission(
        &paths,
        ContextSource::Native {
            typesig_beam: &typesig,
            contracts_beam: &contracts,
        },
        emission,
    );
    ENABLED.store(false, Relaxed);
    let allocations = ALLOCS.load(Relaxed);
    let reallocations = REALLOCS.load(Relaxed);
    let requested_bytes = BYTES.load(Relaxed);
    let loaded = loaded?;
    println!(
        "{}",
        json!({"mode":emission.name(),"modules":loaded.core.modules.len(),"allocation_calls":allocations,"reallocation_calls":reallocations,"cumulative_requested_bytes":requested_bytes,"frontend":loaded.timings,"scope":loaded.core.atom_layout.as_ref().map(|layout|layout.scope.as_str())})
    );
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
