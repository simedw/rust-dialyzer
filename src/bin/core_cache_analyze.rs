//! Analyzes Core Erlang taken from a per-module cache built by OTP's compiler.
//!
//! Usage: core_cache_analyze PRIOR_LIBRARY.json MANIFEST.json OPTIONS.json
//!
//! MANIFEST comes from `prototypes/core_cache/core_cache.exs`: the analysis
//! context, the Dialyzer closure profile and one cached module file per BEAM.
//! OPTIONS is a harness request row; only its analysis options are used.
//! Prints one JSON object with the outcome and load/analysis timings.

use rust_dialyzer_poc::{
    constraints::closure_order::ClosureCodeProfile,
    core::{CoreModule, CoreProgram, etf::module_from_etf},
};
use serde_json::{Value, json};
use std::panic::{AssertUnwindSafe, catch_unwind};

// Analysis allocates heavily from all cores; the system allocator spends
// about a third of CPU time under that load.
#[cfg_attr(not(dialyzer_tsan), global_allocator)]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
use std::time::Instant;

fn load(manifest: &Value) -> Result<CoreProgram, String> {
    let files = manifest["module_files"]
        .as_array()
        .ok_or("module_files list missing")?
        .iter()
        .map(|f| f.as_str().map(str::to_owned).ok_or("module file must be a string"))
        .collect::<Result<Vec<_>, _>>()?;
    // Modules decode independently; read and decode them on all cores.
    let workers = std::thread::available_parallelism().map_or(1, |n| n.get());
    let next = std::sync::atomic::AtomicUsize::new(0);
    let mut decoded: Vec<Option<Result<CoreModule, String>>> = (0..files.len()).map(|_| None).collect();
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(path) = files.get(i) else { break };
                        let module = std::fs::read(path)
                            .map_err(|e| format!("{path}: {e}"))
                            .and_then(|bytes| module_from_etf(&bytes).map_err(|e| format!("{path}: {e}")));
                        done.push((i, module));
                    }
                    done
                })
            })
            .collect();
        for handle in handles {
            for (i, module) in handle.join().expect("decode worker") {
                decoded[i] = Some(module);
            }
        }
    });
    let modules = decoded
        .into_iter()
        .map(|m| m.expect("every module decoded"))
        .collect::<Result<Vec<_>, _>>()?;
    CoreProgram::from_modules(&manifest["context"], modules)
}

fn run(library: &str, manifest: &Value, options: &Value) -> Value {
    let started = Instant::now();
    let core = match load(manifest) {
        Ok(core) => core,
        Err(error) => return json!({"status": "error", "stage": "load", "detail": error}),
    };
    let load_seconds = started.elapsed().as_secs_f64();
    let profile = match ClosureCodeProfile::parse(&manifest["closure_profile"]) {
        Ok(profile) => profile,
        Err(error) => return json!({"status": "error", "stage": "load", "detail": error}),
    };
    let modules = core.modules.len();
    // One-shot run: the process exits after printing, so skip teardown.
    rust_dialyzer_poc::teardown::skip_drops_at_exit();
    let started = Instant::now();
    let result = catch_unwind(AssertUnwindSafe(|| {
        rust_dialyzer_poc::compiled_probe::analyze(&core, options, library, &profile)
    }));
    let analysis_seconds = started.elapsed().as_secs_f64();
    rust_dialyzer_poc::teardown::release(core);
    let base = json!({
        "modules": modules,
        "load_seconds": load_seconds,
        "analysis_seconds": analysis_seconds,
    });
    let outcome = match result {
        Ok(Ok(output)) => json!({
            "status": "ok",
            "public_warning_count": output["public_warning_count"],
            "warnings": output["warnings"],
            "signatures": output["signatures"],
        }),
        Ok(Err(error)) => json!({"status": "error", "stage": "analysis", "detail": error}),
        Err(payload) => json!({
            "status": "panic",
            "stage": "analysis",
            "detail": payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default(),
        }),
    };
    let mut merged = base;
    merged
        .as_object_mut()
        .expect("object")
        .extend(outcome.as_object().expect("object").clone());
    merged
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 3 {
        eprintln!("usage: core_cache_analyze PRIOR_LIBRARY.json MANIFEST.json OPTIONS.json");
        std::process::exit(2);
    }
    let read = |path: &str| {
        std::fs::read_to_string(path).unwrap_or_else(|error| {
            eprintln!("{path}: {error}");
            std::process::exit(2)
        })
    };
    let parse = |path: &str| -> Value {
        serde_json::from_str(&read(path)).unwrap_or_else(|error| {
            eprintln!("{path}: {error}");
            std::process::exit(2)
        })
    };
    let library = read(&args[0]);
    let manifest = parse(&args[1]);
    let options = parse(&args[2]);
    // Guard analysis recurses up to MAX_GUARD_DEPTH; give it a stack to match.
    let output = std::thread::Builder::new()
        .stack_size(4 << 30)
        .spawn(move || run(&library, &manifest, &options))
        .expect("analysis thread")
        .join()
        .expect("analysis thread panicked outside catch_unwind");
    let ok = output["status"] == "ok";
    println!("{output}");
    std::process::exit(if ok { 0 } else { 1 });
}
