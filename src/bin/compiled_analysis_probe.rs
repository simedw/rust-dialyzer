use rust_dialyzer_poc::{
    constraints::closure_order::ClosureCodeProfile,
    core::{CoreProgram, ErlangTerms},
    term_order::compare_map_keys,
};
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn run(row: &Value, library: &str, profile: &ClosureCodeProfile) -> Result<Value, String> {
    let core = CoreProgram::parse(&row["core"].to_string())?;
    let mut output = rust_dialyzer_poc::compiled_probe::analyze(&core, row, library, profile)?;
    let raw = ErlangTerms::parse(&output["warnings"])?;
    let expected = ErlangTerms::parse(&row["warnings"])?;
    output["public_warnings_match"] =
        json!(compare_map_keys(&raw, raw.root, &expected, expected.root)?.is_eq());
    Ok(output)
}
fn main() {
    let mut startup = rust_dialyzer_poc::phase_timings::PhaseTimings::new("probe_startup");
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let library = std::fs::read_to_string(&args[0]).expect("library snapshot");
    startup.mark("read_dependency_snapshot");
    let profile = ClosureCodeProfile::parse(
        &serde_json::from_str::<Value>(
            &std::fs::read_to_string(&args[1]).expect("closure profile"),
        )
        .expect("profile JSON"),
    )
    .expect("closure profile");
    startup.mark("read_and_parse_runtime_profile");
    drop(startup);
    for line in io::stdin().lock().lines() {
        let mut timings = rust_dialyzer_poc::phase_timings::PhaseTimings::new("probe_row");
        let result = line
            .map_err(|error| error.to_string())
            .and_then(|line| {
                serde_json::from_str::<Value>(&line).map_err(|error| error.to_string())
            })
            .and_then(|row| {
                timings.mark("row_json_parse");
                let result = run(&row, &library, &profile);
                timings.mark("run");
                result
            });
        println!("{}", result.unwrap_or_else(|error| json!({"error":error})));
        timings.mark("serialize_and_write_stdout");
    }
}
