mod engine;
mod ir;
mod types;

use engine::Output;
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    hash::{Hash, Hasher},
    path::PathBuf,
    time::Instant,
};

fn cache_version() -> String {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    concat!(
        include_str!("engine.rs"),
        include_str!("ir.rs"),
        include_str!("types.rs")
    )
    .hash(&mut hash);
    format!("{}-{:016x}", env!("CARGO_PKG_VERSION"), hash.finish())
}

#[derive(Serialize, Deserialize)]
struct Cache {
    version: String,
    input: String,
    worklist: bool,
    output: Output,
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let input_path = args.next().ok_or(
        "usage: rust-dialyzer-poc INPUT.json [--cache FILE] [--solver sweep|worklist] [--repeat N]",
    )?;
    let mut cache_path = None;
    let mut worklist = true;
    let mut repeat = 1;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--cache" => cache_path = Some(PathBuf::from(args.next().ok_or("missing cache path")?)),
            "--solver" => {
                worklist = match args.next().as_deref() {
                    Some("sweep") => false,
                    Some("worklist") => true,
                    _ => return Err("solver must be sweep or worklist".into()),
                }
            }
            "--repeat" => {
                repeat = args
                    .next()
                    .ok_or("missing repeat count")?
                    .parse::<usize>()
                    .map_err(|e| e.to_string())?;
                if repeat == 0 {
                    return Err("repeat must be positive".into());
                }
            }
            _ => return Err(format!("unknown argument {arg}")),
        }
    }
    let started = Instant::now();
    let input = fs::read_to_string(input_path).map_err(|e| e.to_string())?;
    let version = cache_version();
    let cached = cache_path
        .as_ref()
        .and_then(|p| fs::read(p).ok())
        .and_then(|b| serde_json::from_slice::<Cache>(&b).ok())
        .filter(|c| c.version == version && c.input == input && c.worklist == worklist);
    let hit = cached.is_some();
    let mut inference_us = 0;
    let output = if let Some(cache) = cached {
        cache.output
    } else {
        let program = ir::Program::parse(&input)?;
        let inference_started = Instant::now();
        let mut output = engine::analyze(&program, worklist)?;
        for _ in 1..repeat {
            output = engine::analyze(&program, worklist)?;
        }
        inference_us = inference_started.elapsed().as_micros();
        if let Some(path) = &cache_path {
            if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let cache = Cache {
                version,
                worklist,
                input,
                output: output.clone(),
            };
            let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
            fs::write(&tmp, serde_json::to_vec(&cache).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            fs::rename(tmp, path).map_err(|e| e.to_string())?;
        }
        output
    };
    let result = serde_json::json!({"version":1,"warnings":output.warnings,"signatures":output.signatures,"evaluations":if hit {0} else {output.evaluations},"cache_hit":hit,"elapsed_us":started.elapsed().as_micros(),"inference_us":inference_us,"repeat":repeat,"solver":if worklist {"worklist"} else {"sweep"}});
    println!(
        "{}",
        serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?
    );
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
