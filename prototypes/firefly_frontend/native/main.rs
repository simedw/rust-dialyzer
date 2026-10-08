mod frontend;
mod inventory;

use rust_dialyzer_poc::{
    constraints::closure_order::ClosureCodeProfile, core::CoreProgram,
    native_runtime_hash::NativeAtomLayout, phase_timings::PhaseTimings,
};
use serde_json::{Value, json};
use std::io::{self, BufRead};

fn run(row: &Value, library: &str, profile: &ClosureCodeProfile) -> Result<Value, String> {
    let mut timings = PhaseTimings::new("native_pipeline");
    let context = row
        .get("context")
        .and_then(Value::as_object)
        .ok_or("context object missing")?;
    if context.keys().any(|key| {
        !matches!(
            key.as_str(),
            "version" | "profile" | "atom_layout" | "contract_closure_profile"
        )
    }) {
        return Err("context contains fields outside runtime metadata".into());
    }
    let paths = row["beam_paths"]
        .as_array()
        .ok_or("beam_paths list missing")?;
    let layout = NativeAtomLayout::parse(&row["context"]["atom_layout"])
        .map_err(|error| error.to_string())?;
    let compiler = elixir_frontend::CompilerVersion::parse(
        row["context"]["profile"]["elixir"]
            .as_str()
            .ok_or("Elixir compiler version missing")?,
    )?;
    let workers = std::thread::available_parallelism()
        .map_or(1, |count| count.get())
        .min(8);
    let mut modules = frontend::lower(paths, &layout, compiler, workers)?;
    modules.sort_by(|left, right| left.name.cmp(&right.name));
    timings.mark("native_frontend");
    let core = CoreProgram::from_modules(&row["context"], modules)?;
    timings.mark("core_validation");
    let mut output = rust_dialyzer_poc::compiled_probe::analyze(&core, row, library, profile)?;
    timings.mark("analysis_and_projection");
    if row["emit_core"].as_bool() == Some(true) {
        output["core"] = core.to_wire_value(&row["context"]);
    }
    Ok(output)
}

fn main() {
    let mut startup = PhaseTimings::new("native_startup");
    let mut args: Vec<_> = std::env::args().skip(1).collect();
    let inventory = args.first().is_some_and(|arg| arg == "--inventory");
    if inventory {
        args.remove(0);
    }
    if args.len() != 2 {
        eprintln!("usage: native-elixir-dialyzer [--inventory] PRIOR_LIBRARY.json CLOSURE_PROFILE.json");
        std::process::exit(1);
    }
    let library = match std::fs::read_to_string(&args[0]) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1)
        }
    };
    startup.mark("read_dependency_snapshot");
    let profile = std::fs::read_to_string(&args[1])
        .map_err(|error| error.to_string())
        .and_then(|raw| serde_json::from_str::<Value>(&raw).map_err(|error| error.to_string()))
        .and_then(|raw| ClosureCodeProfile::parse(&raw));
    let profile = match profile {
        Ok(value) => value,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1)
        }
    };
    startup.mark("read_and_parse_runtime_profile");
    drop(startup);
    let mut failed = false;
    for line in io::stdin().lock().lines() {
        let mut timings = PhaseTimings::new("native_row");
        let result = line
            .map_err(|error| error.to_string())
            .and_then(|line| {
                serde_json::from_str::<Value>(&line).map_err(|error| error.to_string())
            })
            .and_then(|row| {
                timings.mark("request_json_parse");
                if inventory {
                    let workers = std::thread::available_parallelism().map_or(1, |n| n.get());
                    inventory::run(&row, &library, &profile, workers)
                } else {
                    run(&row, &library, &profile)
                }
            });
        let output = match result {
            Ok(value) => value,
            Err(error) => {
                failed = true;
                json!({"error":error})
            }
        };
        println!("{output}");
        timings.mark("run_and_serialize_stdout");
    }
    if failed {
        std::process::exit(1)
    }
}
