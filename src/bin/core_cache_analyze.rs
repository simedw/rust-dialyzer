//! Analyze BEAM files through native compilation, or compiler-produced cached Core.
//!
//! Usage: core_cache_analyze PRIOR_LIBRARY.json MANIFEST.json OPTIONS.json [--frontend native|compiler]
//!
//! MANIFEST selects native `beam_files` with a runtime context, or cached
//! `module_files` with the compiler context and closure profile.
//! OPTIONS is a harness request row; only its analysis options are used.
//! Prints the complete outcome and frontend/analysis timings as one JSON object.

use rust_dialyzer_poc::{
    constraints::closure_order::ClosureCodeProfile,
    core::{CoreModule, CoreProgram, etf::module_from_etf},
};
use serde_json::{Value, json};
use std::panic::{AssertUnwindSafe, catch_unwind};

// Analysis allocates heavily from all cores; the system allocator spends
// about a third of CPU time under that load.
#[cfg(not(feature = "live_bytes"))]
#[cfg_attr(not(dialyzer_tsan), global_allocator)]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
#[cfg(feature = "live_bytes")]
#[cfg_attr(not(dialyzer_tsan), global_allocator)]
static GLOBAL: rust_dialyzer_poc::live_bytes::Counting<mimalloc::MiMalloc> =
    rust_dialyzer_poc::live_bytes::Counting(mimalloc::MiMalloc);
use std::{io::Write, time::Instant};

struct Loaded {
    core: CoreProgram,
    profile: ClosureCodeProfile,
    frontend: Value,
}
// A Core-file inventory explicitly selects the compiler-produced input format.
// BEAM inventories default to native lifting; neither path silently falls back.
fn select_frontend(manifest: &mut Value, frontend: Option<&str>) -> Result<(), String> {
    match frontend {
        None => Ok(()),
        Some("native") if manifest.get("beam_files").is_some() => {
            manifest.as_object_mut().ok_or("manifest must be an object")?.remove("module_files");
            Ok(())
        }
        Some("compiler") if manifest.get("module_files").is_some() => {
            manifest.as_object_mut().ok_or("manifest must be an object")?.remove("beam_files");
            Ok(())
        }
        Some("native") => Err("native frontend requires a beam_files inventory".into()),
        Some("compiler") => Err("compiler frontend requires compiler-produced module_files; prepare them with prototypes/core_cache/core_cache.exs".into()),
        _ => Err("frontend must be native or compiler".into()),
    }
}

fn load(manifest: &Value) -> Result<Loaded, String> {
    if let Some(files) = manifest.get("beam_files") {
        if manifest.get("module_files").is_some() {
            return Err("choose beam_files or module_files, not both".into());
        }
        let paths = files
            .as_array()
            .ok_or("beam_files must be a list")?
            .iter()
            .map(|f| {
                f.as_str()
                    .map(str::to_owned)
                    .ok_or("BEAM path must be a string")
            })
            .collect::<Result<Vec<_>, _>>()?;
        use rust_dialyzer_poc::beam_frontend::pipeline::{self, ContextSource, Emission};
        let emission = match manifest.get("core_emission").and_then(Value::as_str) {
            Some("reference") => Emission::Reference,
            Some("direct") => Emission::Direct,
            None | Some("fused") => Emission::Fused,
            _ => return Err("core_emission must be reference, direct or fused".into()),
        };
        let loaded = match manifest["runtime_context"].as_str() {
            Some("imported") => pipeline::load_emission(
                &paths,
                ContextSource::Imported {
                    context: &manifest["context"],
                    closure_profile: &manifest["closure_profile"],
                },
                emission,
            )?,
            None | Some("native") => {
                let read = |key: &str| -> Result<Vec<u8>, String> {
                    let path = manifest[key]
                        .as_str()
                        .ok_or_else(|| format!("{key} path missing"))?;
                    std::fs::read(path).map_err(|e| format!("{path}: {e}"))
                };
                let typesig = read("dialyzer_typesig_beam")?;
                let contracts = read("dialyzer_contracts_beam")?;
                pipeline::load_emission(
                    &paths,
                    ContextSource::Native {
                        typesig_beam: &typesig,
                        contracts_beam: &contracts,
                    },
                    emission,
                )?
            }
            _ => {
                return Err(
                    "native BEAM manifest requires runtime_context native or imported".into(),
                );
            }
        };
        return Ok(Loaded {
            core: loaded.core,
            profile: loaded.closure_profile,
            frontend: loaded.timings,
        });
    }
    let files = manifest["module_files"]
        .as_array()
        .ok_or("module_files list missing")?
        .iter()
        .map(|f| {
            f.as_str()
                .map(str::to_owned)
                .ok_or("module file must be a string")
        })
        .collect::<Result<Vec<_>, _>>()?;
    // Modules decode independently; read and decode them on all cores.
    let workers = std::thread::available_parallelism().map_or(1, |n| n.get());
    let next = std::sync::atomic::AtomicUsize::new(0);
    let mut decoded: Vec<Option<Result<CoreModule, String>>> =
        (0..files.len()).map(|_| None).collect();
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
                            .and_then(|bytes| {
                                module_from_etf(&bytes).map_err(|e| format!("{path}: {e}"))
                            });
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
    Ok(Loaded {
        core: CoreProgram::from_modules(&manifest["context"], modules)?,
        profile: ClosureCodeProfile::parse(&manifest["closure_profile"])?,
        frontend: json!({"mode":"cached_core"}),
    })
}

// The result object, plus the serialized signature rows when analysis succeeded.
fn run(library: &str, manifest: &Value, options: &Value) -> (Value, Option<Vec<Vec<u8>>>) {
    let started = Instant::now();
    let Loaded {
        core,
        profile,
        frontend,
    } = match load(manifest) {
        Ok(loaded) => loaded,
        Err(error) => {
            return (json!({"status": "error", "stage": "load", "detail": error}), None);
        }
    };
    let load_seconds = started.elapsed().as_secs_f64();
    let modules = core.modules.len();
    // One-shot run: the process exits after printing, so skip teardown.
    rust_dialyzer_poc::teardown::skip_drops_at_exit();
    let started = Instant::now();
    let result = catch_unwind(AssertUnwindSafe(|| {
        rust_dialyzer_poc::compiled_probe::analyze_projected(&core, options, library, &profile)
    }));
    let analysis_seconds = started.elapsed().as_secs_f64();
    rust_dialyzer_poc::teardown::release(core);
    let base = json!({
        "modules": modules,
        "frontend": frontend,
        "load_seconds": load_seconds,
        "analysis_seconds": analysis_seconds,
    });
    let mut signatures = None;
    let outcome = match result {
        Ok(Ok(_)) if rust_dialyzer_poc::constraints::generation::generation_parallel_faulted() => {
            json!({
                "status": "panic",
                "stage": "parallel_generation",
                "detail": "a speculative generation panic was retained; this run cannot certify the parallel path",
            })
        }
        Ok(Ok(mut output)) => {
            let mut outcome = json!({"status": "ok"});
            for key in ["public_warning_count", "warnings"] {
                outcome[key] = output.result[key].take();
            }
            signatures = Some(output.signatures);
            outcome
        }
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
    let Value::Object(outcome) = outcome else {
        unreachable!("analysis outcome is an object")
    };
    merged.as_object_mut().expect("object").extend(outcome);
    (merged, signatures)
}

// The analysis serialized the signature rows; splice them in where
// serde_json would have written the array, keeping its key order.
fn write_result(
    out: &mut impl Write,
    mut output: Value,
    signatures: Option<Vec<Vec<u8>>>,
) -> std::io::Result<()> {
    const PLACEHOLDER: &str = "\u{0}signatures\u{0}";
    let Some(rows) = signatures else {
        return serde_json::to_writer(&mut *out, &output).map_err(std::io::Error::other);
    };
    output["signatures"] = Value::String(PLACEHOLDER.into());
    let text = serde_json::to_string(&output).map_err(std::io::Error::other)?;
    let marker = serde_json::to_string(PLACEHOLDER).map_err(std::io::Error::other)?;
    let at = text.find(&marker).expect("signature placeholder");
    out.write_all(text[..at].as_bytes())?;
    out.write_all(b"[")?;
    for (i, row) in rows.iter().enumerate() {
        if i > 0 {
            out.write_all(b",")?;
        }
        out.write_all(row)?;
    }
    out.write_all(b"]")?;
    out.write_all(text[at + marker.len()..].as_bytes())?;
    // Exiting right after; the operating system reclaims the rows.
    std::mem::forget(rows);
    Ok(())
}

fn main() {
    #[cfg(feature = "live_bytes")]
    rust_dialyzer_poc::live_bytes::start_sampler();
    let args: Vec<_> = std::env::args().skip(1).collect();
    let frontend = match args.as_slice() {
        [_, _, _] => None,
        [_, _, _, flag, value]
            if flag == "--frontend" && matches!(value.as_str(), "native" | "compiler") =>
        {
            Some(value.as_str())
        }
        _ => {
            eprintln!(
                "usage: core_cache_analyze PRIOR_LIBRARY.json MANIFEST.json OPTIONS.json [--frontend native|compiler]"
            );
            std::process::exit(2);
        }
    };
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
    let mut manifest = parse(&args[1]);
    if let Err(error) = select_frontend(&mut manifest, frontend) {
        eprintln!("{error}");
        std::process::exit(2);
    }
    let options = parse(&args[2]);
    // Guard analysis recurses up to MAX_GUARD_DEPTH; give it a stack to match.
    let (output, signatures) = std::thread::Builder::new()
        .stack_size(4 << 30)
        .spawn(move || run(&library, &manifest, &options))
        .expect("analysis thread")
        .join()
        .expect("analysis thread panicked outside catch_unwind");
    let ok = output["status"] == "ok";
    if std::env::var_os("RUST_DIALYZER_GENERATION_ADMISSION").is_some_and(|v| v == "1") {
        eprintln!(
            "parallel_generation admitted_sccs={} faulted={}",
            rust_dialyzer_poc::constraints::generation::generation_parallel_admitted_sccs(),
            rust_dialyzer_poc::constraints::generation::generation_parallel_faulted()
        );
        eprintln!(
            "diagnostic_snapshot resumes={}",
            rust_dialyzer_poc::core_phases::diagnostic_snapshot_resumes()
        );
    }
    let mut stdout = std::io::BufWriter::new(std::io::stdout().lock());
    write_result(&mut stdout, output, signatures).expect("write analysis result");
    stdout.write_all(b"\n").expect("write analysis newline");
    stdout.flush().expect("flush analysis result");
    std::process::exit(if ok { 0 } else { 1 });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontend_selection_preserves_explicit_inputs_and_never_falls_back() {
        let mut native = json!({"beam_files": []});
        select_frontend(&mut native, None).unwrap();
        select_frontend(&mut native, Some("native")).unwrap();
        assert!(select_frontend(&mut native, Some("compiler")).is_err());
        let mut compiler = json!({"module_files": []});
        select_frontend(&mut compiler, None).unwrap();
        assert!(select_frontend(&mut compiler, Some("native")).is_err());
        let mut both = json!({"beam_files": [], "module_files": []});
        select_frontend(&mut both, Some("native")).unwrap();
        assert!(both.get("module_files").is_none());
        let mut both = json!({"beam_files": [], "module_files": []});
        select_frontend(&mut both, Some("compiler")).unwrap();
        assert!(both.get("beam_files").is_none());
    }

    fn written(output: &Value, rows: &[Value]) -> Vec<u8> {
        let serialized = rows.iter().map(|row| serde_json::to_vec(row).unwrap()).collect();
        let mut bytes = Vec::new();
        write_result(&mut bytes, output.clone(), Some(serialized)).unwrap();
        bytes
    }

    #[test]
    fn spliced_rows_match_native_serialization() {
        let output = json!({"status":"ok","modules":2,"warnings":[],"public_warning_count":0});
        let cases = [
            vec![],
            vec![json!({"module":"m","function":"private","arity":0})],
            (0..7).map(|i| json!({"ordinal":i,"arity":i})).collect(),
            vec![
                json!({"module":"λ雪","function":"\u{0}signatures\u{0}","arity":1,
                    "arguments":[{"kind":"tuple","elements":[{"kind":"atom","value":"é"}]}],
                    "result":{"kind":"map","entries":[{"key":{"kind":"atom","value":"key"},
                        "presence":"mandatory","value":{"kind":"list","elements":{"kind":"any"},
                        "tail":{"kind":"nil"},"nonempty":false}}]}}),
                json!({"module":"m","function":"private","arity":0,"arguments":[],
                    "result":{"kind":"nominal_raw","name":"quoted\"\\name","inner":{"kind":"none"}}}),
            ],
        ];
        for rows in cases {
            let mut expected = output.clone();
            expected["signatures"] = Value::Array(rows.clone());
            assert_eq!(written(&output, &rows), serde_json::to_vec(&expected).unwrap());
        }
    }

    #[test]
    fn failed_runs_write_without_signatures() {
        let output = json!({"status":"error","stage":"analysis","detail":"boom"});
        let mut bytes = Vec::new();
        write_result(&mut bytes, output.clone(), None).unwrap();
        assert_eq!(bytes, serde_json::to_vec(&output).unwrap());
    }
}
