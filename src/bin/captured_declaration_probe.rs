use rust_dialyzer_poc::{
    compiled_definitions::PreparedDefinitions,
    core::CoreProgram,
    dependencies::{ImportPlan, PriorPlt},
    form_error::FormError,
    program::Program,
    term_order::CaptureEnvironment,
};
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn main() -> Result<(), String> {
    let library = std::fs::read_to_string(std::env::args().nth(1).ok_or("library")?)
        .map_err(|e| e.to_string())?;
    for line in io::stdin().lock().lines() {
        let row: Value =
            serde_json::from_str(&line.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let core = CoreProgram::parse(&row["core"].to_string())?;
        let program = Program::build(&core)?;
        let library = PriorPlt::parse(&library, &core, &ImportPlan::cold_targets(&core))?
            .into_cold_store()?;
        let result = PreparedDefinitions::prepare(&core, &program, &library);
        let matched = match &result {
            Ok(_) => row["expected"]["status"] == "ok",
            Err(e) => match &e.error {
                FormError::CapturedError(actual) => {
                    if !actual
                        .compare(&CaptureEnvironment::parse(&actual.to_value())?)?
                        .is_eq()
                    {
                        return Err("capture serialization roundtrip changed the reason".into());
                    }
                    actual
                        .compare(&CaptureEnvironment::parse(&row["expected"]["value"])?)?
                        .is_eq()
                }
                FormError::MissingClosureProfile(module) => {
                    row["expected"]["missing_profile"] == *module
                }
                _ => {
                    let report = e.error.report();
                    if let Some(reason) = report.get("reason") {
                        let raw = rust_dialyzer_poc::core::ErlangTerms::parse(reason)?;
                        CaptureEnvironment::literal_roots(&raw, &[raw.root])?
                            .compare(&CaptureEnvironment::parse(&row["expected"]["value"])?)?
                            .is_eq()
                    } else {
                        false
                    }
                }
            },
        };
        println!(
            "{}",
            json!({"id":row["id"],"matched":matched,"error":result.err().map(|e|e.error.report())})
        );
    }
    Ok(())
}
