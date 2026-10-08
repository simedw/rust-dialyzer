//! Per-module coverage inventory.
//!
//! The ordinary pipeline stops at the first failing module. Inventory mode runs
//! every module through each frontend stage independently, records the first
//! failure (error or panic) per module, then analyzes the modules that lowered.

use rust_dialyzer_poc::{
    constraints::closure_order::ClosureCodeProfile,
    core::{CoreModule, CoreProgram},
    external_term::{Limits, encode},
    native_runtime_hash::NativeAtomLayout,
};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicUsize, Ordering};

thread_local! {
    static PANIC: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Replaces the default panic printer so panics become per-module records.
fn capture_panics() {
    std::panic::set_hook(Box::new(|info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "non-string panic payload".into());
        let location = info
            .location()
            .map(|l| format!(" at {}:{}", l.file(), l.line()))
            .unwrap_or_default();
        PANIC.with(|p| *p.borrow_mut() = Some(format!("{message}{location}")));
    }));
}

/// Runs one stage, turning both `Err` and panics into a failure description.
fn guarded<T>(f: impl FnOnce() -> Result<T, String>) -> Result<T, (&'static str, String)> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(("error", error)),
        Err(_) => Err((
            "panic",
            PANIC.with(|p| p.borrow_mut().take()).unwrap_or_default(),
        )),
    }
}

fn failure(path: &str, stage: &str, (status, detail): (&str, String)) -> Value {
    json!({"beam": path, "stage": stage, "status": status, "detail": detail})
}

pub fn run(
    row: &Value,
    library: &str,
    profile: &ClosureCodeProfile,
    workers: usize,
) -> Result<Value, String> {
    capture_panics();
    let paths = row["beam_paths"]
        .as_array()
        .ok_or("beam_paths list missing")?
        .iter()
        .map(|p| p.as_str().map(str::to_owned).ok_or("BEAM path must be string"))
        .collect::<Result<Vec<_>, _>>()?;
    let layout = NativeAtomLayout::parse(&row["context"]["atom_layout"])
        .map_err(|error| error.to_string())?;
    let compiler = elixir_frontend::CompilerVersion::parse(
        row["context"]["profile"]["elixir"]
            .as_str()
            .ok_or("Elixir compiler version missing")?,
    )?;

    // Stage 1, parallel: BEAM chunks, ETF metadata and Elixir translation.
    let next = AtomicUsize::new(0);
    let mut prepared: Vec<_> = (0..paths.len()).map(|_| None).collect();
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers.max(1))
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(path) = paths.get(index) else { break };
                        let result = guarded(|| {
                            native_beam_seed::prepare(path, Some(&layout), compiler)
                                .map_err(|error| error.to_string())
                        });
                        done.push((index, result));
                    }
                    done
                })
            })
            .collect();
        for handle in handles {
            for (index, result) in handle.join().expect("inventory worker") {
                prepared[index] = Some(result);
            }
        }
    });

    // Stages 2 and 3, sequential: Firefly's interner is process-global.
    let mut records = Vec::new();
    let mut lowered: Vec<(String, CoreModule)> = Vec::new();
    for (path, prepared) in paths.iter().zip(prepared) {
        let prepared = match prepared.expect("every module prepared") {
            Ok(value) => value,
            Err(f) => {
                records.push(failure(path, "beam_preparation", f));
                continue;
            }
        };
        let forms = guarded(|| {
            let bytes = encode(&prepared.forms, Limits::default(), false)
                .map_err(|error| error.to_string())?;
            firefly_beam::AbstractCode::from_abstract_forms(&bytes)
                .map_err(|error| error.to_string())
        });
        let forms = match forms {
            Ok(value) => value,
            Err(f) => {
                records.push(failure(path, "abstract_forms_decoding", f));
                continue;
            }
        };
        let seed = prepared.seed;
        match guarded(|| {
            firefly_core_adapter::frontend::lower(forms, seed).map_err(|error| error.to_string())
        }) {
            Ok(module) => lowered.push((path.clone(), module)),
            Err(f) => records.push(failure(path, "core_lowering", f)),
        }
    }

    // Stage 4: validate each module alone so failures are attributable.
    let mut valid = Vec::new();
    for (path, module) in lowered {
        let check = module.clone();
        match guarded(|| CoreProgram::from_modules(&row["context"], vec![check]).map(drop)) {
            Ok(()) => {
                records.push(json!({"beam": path, "stage": "complete", "status": "ok"}));
                valid.push(module);
            }
            Err(f) => records.push(failure(&path, "core_validation", f)),
        }
    }

    if row["inventory_analysis"].as_bool() == Some(false) {
        return Ok(json!({"id": row["id"], "modules": records}));
    }

    // Stage 5: whole-program analysis of the modules that passed the frontend.
    valid.sort_by(|left, right| left.name.cmp(&right.name));
    let analyzed = valid.len();
    let started = std::time::Instant::now();
    let analysis = guarded(|| {
        let core = CoreProgram::from_modules(&row["context"], valid)?;
        rust_dialyzer_poc::compiled_probe::analyze(&core, row, library, profile)
    });
    let analysis = match analysis {
        Ok(output) => json!({
            "status": "ok",
            "modules": analyzed,
            "seconds": started.elapsed().as_secs_f64(),
            "public_warning_count": output["public_warning_count"],
        }),
        Err((status, detail)) => json!({
            "status": status,
            "modules": analyzed,
            "seconds": started.elapsed().as_secs_f64(),
            "detail": detail,
        }),
    };
    Ok(json!({"id": row["id"], "modules": records, "analysis": analysis}))
}
