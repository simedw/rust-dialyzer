use rust_dialyzer_poc::{
    compiled_definitions::PreparedDefinitions,
    core::CoreProgram,
    program::{Program, TargetContracts},
};
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn main() {
    let library_data = std::fs::read_to_string(std::env::args().nth(1).unwrap()).unwrap();
    for l in io::stdin().lock().lines() {
        let r: Value = serde_json::from_str(&l.unwrap()).unwrap();
        let core = CoreProgram::parse(&r["core"].to_string()).unwrap();
        let mode = if r["use_contracts"] == false {
            TargetContracts::Ignore
        } else {
            TargetContracts::Use
        };
        let mut error = None;
        let outcome = match Program::build_with_contracts(&core, mode) {
            Err(e) => {
                error = Some(e);
                false
            }
            Ok(p) => {
                let library = rust_dialyzer_poc::dependencies::PriorPlt::parse(
                    &library_data,
                    &core,
                    &rust_dialyzer_poc::dependencies::ImportPlan::cold_targets(&core),
                )
                .and_then(|prior| prior.into_cold_store())
                .unwrap();
                match PreparedDefinitions::prepare(&core, &p, &library) {
                    Err(e) => {
                        error = Some(e.error.report().to_string());
                        false
                    }
                    Ok(d) => {
                        if mode == TargetContracts::Ignore {
                            assert!(
                                d.records.is_empty()
                                    && d.contracts.is_empty()
                                    && d.callbacks.is_empty()
                                    && d.external_types.is_empty()
                            );
                            assert!(p.metadata.iter().all(|m| !matches!(
                                m.kind,
                                rust_dialyzer_poc::program::MetadataKind::Spec
                                    | rust_dialyzer_poc::program::MetadataKind::Callback
                                    | rust_dialyzer_poc::program::MetadataKind::OptionalCallbacks
                            )));
                        }
                        true
                    }
                }
            }
        };
        println!(
            "{}",
            json!({"id":r["id"],"use_contracts":r["use_contracts"],"expected":r["expected"],"status":if outcome{"ok"}else{"error"},"error":error,"matched_status":outcome==(r["expected"]["status"]=="ok")})
        );
    }
}
