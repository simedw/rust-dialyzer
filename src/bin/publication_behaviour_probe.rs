use rust_dialyzer_poc::{
    compiled_definitions::PreparedDefinitions,
    core::CoreProgram,
    module_publication::{PublicationError, PublicationInput},
    program::{ModuleId, Program},
};
use std::io::{self, BufRead};
fn main() {
    for line in io::stdin().lock().lines() {
        let row: serde_json::Value = serde_json::from_str(&line.unwrap()).unwrap();
        let raw = std::fs::read_to_string(row["proof_core"].as_str().unwrap()).unwrap();
        let core = match CoreProgram::parse(&raw) {
            Ok(c) => c,
            Err(e) => {
                assert!(["nonlist_ann", "improper_ann"].contains(&row["id"].as_str().unwrap()));
                assert!(e.contains("expected proper Erlang list"));
                println!(
                    "{}",
                    serde_json::json!({"id":row["id"],"compiler_boundary":true})
                );
                continue;
            }
        };
        let program = Program::build(&core).unwrap();
        let library = rust_dialyzer_poc::dependencies::PriorPlt::parse(
            &std::fs::read_to_string(row["environment"].as_str().unwrap()).unwrap(),
            &core,
            &rust_dialyzer_poc::dependencies::ImportPlan::cold_targets(&core),
        )
        .and_then(|prior| prior.into_cold_store())
        .unwrap();
        let defs = PreparedDefinitions::prepare(&core, &program, &library).unwrap();
        let result = PublicationInput {
            core: &core,
            program: &program,
            definitions: &defs,
            targets: &[ModuleId(0)],
            use_contracts: false,
        }
        .type_dependencies();
        let actual = match result {
            Ok(v) => {
                serde_json::json!({"ok":v.named_projection().expect("named behaviour fixture")})
            }
            Err(PublicationError::Native(v)) => serde_json::json!({"error":format!("{v:?}")}),
            Err(e) => panic!("{e:?}"),
        };
        assert_eq!(actual, row["expected"]);
        println!("{}", serde_json::json!({"id":row["id"],"exact":true}));
    }
}
