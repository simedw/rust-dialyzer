#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
use rust_dialyzer_poc::{
    beam_frontend::{self, clean, debug, stats as counts},
    external_term::{self, Limits},
};
use std::{env, fs, process, time::Instant};

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 || !matches!(args[1].as_str(), "inspect" | "forms" | "core" | "clean") {
        return Err("usage: beam_core_compile {inspect|forms|core|clean} BEAM [OUTPUT.etf]".into());
    }
    let total = Instant::now();
    let bytes = fs::read(&args[2]).map_err(|e| format!("read: {e}"))?;
    let decode_started = Instant::now();
    let module = debug::read_beam(&bytes, Limits::default())?;
    let decode_ms = decode_started.elapsed().as_secs_f64() * 1000.0;
    let mut stats = serde_json::json!({"module": module.module, "backend": module.backend.name(), "beam_bytes": bytes.len(), "chunks": module.chunk_count, "metadata_arena_slots": module.terms.terms.len(), "decode_ms": decode_ms});
    if args[1] != "inspect" {
        let output = args
            .get(3)
            .ok_or("output path is required for compilation")?;
        let started = Instant::now();
        let mut term =
            beam_frontend::forms(&module).map_err(|e| format!("forms {}: {e}", module.module))?;
        stats["forms_ms"] = (started.elapsed().as_secs_f64() * 1000.0).into();
        stats["forms_arena_slots"] = term.terms.len().into();
        stats["forms_reachable_terms"] = counts::reachable_term_count(&term).into();
        if matches!(args[1].as_str(), "core" | "clean") {
            let started = Instant::now();
            term = beam_frontend::raw_core(&module, &term)
                .map_err(|e| format!("core {}: {e}", module.module))?;
            stats["core_ms"] = (started.elapsed().as_secs_f64() * 1000.0).into();
            stats["core_arena_slots"] = term.terms.len().into();
            stats["core_nodes"] = counts::core_node_count(&term)?.into();
        }
        if args[1] == "clean" {
            let started = Instant::now();
            term = clean::clean(&term).map_err(|e| format!("clean {}: {e}", module.module))?;
            stats["clean_ms"] = (started.elapsed().as_secs_f64() * 1000.0).into();
            stats["cleaned_core_nodes"] = counts::core_node_count(&term)?.into();
        }
        let encoded = external_term::encode(
            &term,
            Limits {
                bytes: 256 * 1024 * 1024,
                nodes: 20_000_000,
                expanded: 20_000_000,
            },
            false,
        )
        .map_err(|e| e.to_string())?;
        fs::write(output, &encoded).map_err(|e| format!("write: {e}"))?;
        stats["output_bytes"] = encoded.len().into();
    }
    stats["total_ms"] = (total.elapsed().as_secs_f64() * 1000.0).into();
    println!("{stats}");
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        process::exit(1);
    }
}
