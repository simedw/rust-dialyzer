//! Frontend benchmark; filesystem reads, output encoding and transport excluded.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
use rust_dialyzer_poc::{
    beam_frontend::{compile_beam, debug, forms},
    external_term::Limits,
};
use std::{env, fs, hint::black_box, process, time::Instant};
fn run() -> Result<(), String> {
    let args: Vec<_> = env::args().collect();
    if args.len() != 4 || !matches!(args[1].as_str(), "forms" | "clean") {
        return Err("usage: beam_core_bench {forms|clean} ITERATIONS MANIFEST".into());
    }
    let count: usize = args[2].parse().map_err(|_| "invalid iterations")?;
    if count == 0 {
        return Err("positive iterations required".into());
    }
    let paths = fs::read_to_string(&args[3]).map_err(|e| e.to_string())?;
    let inputs: Vec<_> = paths
        .lines()
        .map(|path| fs::read(path).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    if inputs.is_empty() {
        return Err("empty manifest".into());
    }
    let compile = |bytes: &[u8]| -> Result<(), String> {
        if args[1] == "clean" {
            black_box(compile_beam(black_box(bytes))?);
        } else {
            let d = debug::read_beam(black_box(bytes), Limits::default())?;
            black_box(forms(&d)?);
        }
        Ok(())
    };
    for bytes in &inputs {
        compile(bytes)?;
    }
    let mut batches = Vec::new();
    for _ in 0..5 {
        let start = Instant::now();
        for _ in 0..count {
            for bytes in &inputs {
                compile(bytes)?;
            }
        }
        batches.push(start.elapsed().as_secs_f64() * 1000.0 / count as f64);
    }
    batches.sort_by(f64::total_cmp);
    println!(
        "{}",
        serde_json::json!({"stage":args[1],"manifest":args[3],"modules":inputs.len(),"beam_bytes":inputs.iter().map(Vec::len).sum::<usize>(),"iterations_per_batch":count,"batches":5,"median_ms":batches[2],"batch_ms":batches,"scope":"BEAM decode + forms, optionally raw Core + clean; filesystem IO and ETF transport excluded"})
    );
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        process::exit(1)
    }
}
