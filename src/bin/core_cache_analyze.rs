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
use std::{io::Write, time::Instant};

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
        Ok(Ok(_)) if rust_dialyzer_poc::constraints::generation::generation_parallel_faulted() => json!({
            "status": "panic",
            "stage": "parallel_generation",
            "detail": "a speculative generation panic was retained; this run cannot certify the parallel path",
        }),
        Ok(Ok(mut output)) => {
            let mut outcome = json!({"status": "ok"});
            for key in ["public_warning_count", "warnings", "signatures"] {
                outcome[key] = output[key].take();
            }
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
    merged
        .as_object_mut()
        .expect("object")
        .extend(outcome);
    merged
}

// Match the analysis/setup worker stack for recursive Value serialization.
const SIGNATURE_WRITER_STACK_BYTES: usize = 1 << 30;

fn serialize_signature_parts(rows: &[Value], workers: usize, sequential: bool) -> Vec<Vec<u8>> {
    let serialize = |rows: &[Value]| {
        let mut out = Vec::new();
        for (i, row) in rows.iter().enumerate() {
            if i > 0 {
                out.push(b',');
            }
            #[cfg(test)]
            tests::SERIALIZED_ROWS.with(|count| count.set(count.get() + 1));
            serde_json::to_writer(&mut out, row).expect("serialize signature");
        }
        out
    };
    if rows.is_empty() {
        return Vec::new();
    }
    if sequential || workers <= 1 {
        return vec![serialize(rows)];
    }
    let chunk = rows.len().div_ceil(workers);
    std::thread::scope(|scope| {
        let serialize = &serialize;
        let handles: Vec<_> = rows
            .chunks(chunk)
            .map(|rows| {
                std::thread::Builder::new()
                    .stack_size(SIGNATURE_WRITER_STACK_BYTES)
                    .spawn_scoped(scope, move || serialize(rows))
                    .expect("spawn signature writer")
            })
            .collect();
        handles.into_iter().map(|h| h.join().expect("signature writer")).collect()
    })
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
    if std::env::var_os("RUST_DIALYZER_GENERATION_ADMISSION").is_some_and(|v| v == "1") {
        eprintln!("parallel_generation admitted_sccs={} faulted={}",
            rust_dialyzer_poc::constraints::generation::generation_parallel_admitted_sccs(),
            rust_dialyzer_poc::constraints::generation::generation_parallel_faulted());
        eprintln!("diagnostic_snapshot resumes={}",
            rust_dialyzer_poc::core_phases::diagnostic_snapshot_resumes());
    }
    let mut output = output;
    let mut stdout = std::io::BufWriter::new(std::io::stdout().lock());
    // The signature array is most of the output: serialize its rows on all
    // cores, then splice the bytes where serde_json would have written them.
    const PLACEHOLDER: &str = "\u{0}signatures\u{0}";
    let signatures = match output.get_mut("signatures") {
        Some(slot @ Value::Array(_)) => Some(std::mem::replace(slot, Value::String(PLACEHOLDER.into()))),
        _ => None,
    };
    let text = serde_json::to_string(&output).expect("serialize analysis result");
    match signatures {
        Some(Value::Array(rows)) => {
            let marker = serde_json::to_string(PLACEHOLDER).expect("placeholder");
            let at = text.find(&marker).expect("signature placeholder");
            let workers = std::thread::available_parallelism().map_or(1, |n| n.get());
            let parts = serialize_signature_parts(
                &rows,
                workers,
                rust_dialyzer_poc::ordered_parallel::sequential(),
            );
            stdout.write_all(text[..at].as_bytes()).expect("write analysis result");
            stdout.write_all(b"[").expect("write analysis result");
            for (i, part) in parts.iter().enumerate() {
                if i > 0 {
                    stdout.write_all(b",").expect("write analysis result");
                }
                stdout.write_all(part).expect("write analysis result");
            }
            stdout.write_all(b"]").expect("write analysis result");
            stdout.write_all(text[at + marker.len()..].as_bytes()).expect("write analysis result");
            std::mem::forget(parts);
            std::mem::forget(rows);
        }
        _ => stdout.write_all(text.as_bytes()).expect("write analysis result"),
    }
    std::mem::forget(output);
    stdout.write_all(b"\n").expect("write analysis newline");
    stdout.flush().expect("flush analysis result");
    std::process::exit(if ok { 0 } else { 1 });
}

#[cfg(test)]
mod tests {
    use super::*;

    std::thread_local! {
        pub(super) static SERIALIZED_ROWS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    fn array_bytes(parts: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = vec![b'['];
        for (i, part) in parts.iter().enumerate() {
            if i > 0 {
                bytes.push(b',');
            }
            bytes.extend_from_slice(part);
        }
        bytes.push(b']');
        bytes
    }

    #[test]
    fn signature_parts_empty_and_single_match_native_array() {
        for rows in [vec![], vec![json!({"module":"m","function":"private","arity":0})]] {
            for (workers, sequential) in [(1, false), (4, false), (4, true)] {
                let parts = serialize_signature_parts(&rows, workers, sequential);
                assert_eq!(array_bytes(&parts), serde_json::to_vec(&rows).unwrap());
            }
        }
    }

    #[test]
    fn signature_parts_preserve_multiple_chunk_row_order() {
        let rows: Vec<_> = (0..7).map(|i| json!({"ordinal":i,"arity":i})).collect();
        for workers in [2, 3, 16] {
            let parts = serialize_signature_parts(&rows, workers, false);
            assert!(parts.len() > 1);
            let bytes = array_bytes(&parts);
            assert_eq!(bytes, serde_json::to_vec(&rows).unwrap());
            assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), Value::Array(rows.clone()));
        }
    }

    #[test]
    fn signature_parts_preserve_unicode_marker_and_nested_types() {
        let rows = vec![
            json!({"module":"λ雪","function":"\u{0}signatures\u{0}","arity":1,
                "arguments":[{"kind":"tuple","elements":[{"kind":"atom","value":"é"}]}],
                "result":{"kind":"map","entries":[{"key":{"kind":"atom","value":"key"},
                    "presence":"mandatory","value":{"kind":"list","elements":{"kind":"any"},
                    "tail":{"kind":"nil"},"nonempty":false}}]}}),
            json!({"module":"m","function":"private","arity":0,"arguments":[],
                "result":{"kind":"nominal_raw","name":"quoted\"\\name","inner":{"kind":"none"}}}),
            json!({"module":"m","function":"public","arity":0,"arguments":[],
                "result":{"kind":"product_raw","values":[{"kind":"unit"}]}}),
        ];
        for sequential in [false, true] {
            let parts = serialize_signature_parts(&rows, 2, sequential);
            assert_eq!(array_bytes(&parts), serde_json::to_vec(&rows).unwrap());
        }
    }

    #[test]
    fn signature_parts_sequential_and_one_worker_run_on_caller() {
        let rows = vec![json!({"ordinal":0}), json!({"ordinal":1}), json!({"ordinal":2})];
        for (workers, sequential) in [(4, true), (1, false)] {
            SERIALIZED_ROWS.with(|count| count.set(0));
            let parts = serialize_signature_parts(&rows, workers, sequential);
            assert_eq!(parts.len(), 1);
            SERIALIZED_ROWS.with(|count| assert_eq!(count.get(), rows.len()));
            assert_eq!(array_bytes(&parts), serde_json::to_vec(&rows).unwrap());
        }
        SERIALIZED_ROWS.with(|count| count.set(0));
        let parts = serialize_signature_parts(&rows, 2, false);
        SERIALIZED_ROWS.with(|count| assert_eq!(count.get(), 0));
        assert_eq!(array_bytes(&parts), serde_json::to_vec(&rows).unwrap());
    }
}
