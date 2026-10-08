use rust_dialyzer_poc::{
    bifs::registry::Registry,
    closure_dependencies::{ClosureDependencies, DependencyPolicy},
    compiled_definitions::PreparedDefinitions,
    core::{CoreProgram, ErlangTerms},
    dataflow_nodes::DataflowNodes,
    dependencies::{ImportPlan, PriorPlt},
    diagnostic_inputs::TargetCallbackState,
    labels::{CoreLabel, Labels},
    native_runtime_hash::NativeAtomLayout,
    program::Program,
    source_callgraph::SourceCallGraph,
    term_order::compare_map_keys,
};
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn main() -> Result<(), String> {
    let library = std::fs::read_to_string(std::env::args().nth(1).ok_or("library")?)
        .map_err(|e| e.to_string())?;
    let bifs = Registry::load(include_str!("../../corpus/bifs.json"))?;
    let dependencies =
        DependencyPolicy::parse(include_str!("../../corpus/dependency_policy.json"))?;
    for line in io::stdin().lock().lines() {
        let row: Value =
            serde_json::from_str(&line.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let core = CoreProgram::parse(&row["core"].to_string())?;
        let program = Program::build(&core)?;
        let library = PriorPlt::parse(&library, &core, &ImportPlan::cold_targets(&core))?
            .into_cold_store()?;
        let definitions = PreparedDefinitions::prepare(&core, &program, &library)
            .map_err(|e| e.error.to_string())?;
        let labels = Labels::build(&core, CoreLabel(0))?;
        let nodes = DataflowNodes::build(&core, &labels)?;
        let deps = ClosureDependencies::analyze(&core, &program, &dependencies)?;
        let _graph = SourceCallGraph::build(&nodes, &program, &deps, &bifs)
            .map_err(|e| format!("{e:?}"))?;
        let layout =
            NativeAtomLayout::parse(&row["core"]["atom_layout"]).map_err(|e| e.to_string())?;
        let callbacks = TargetCallbackState::from_definitions(&definitions, &layout)
            .map_err(|error| format!("{error:?}"))?;
        let actual = callbacks
            .persistence_groups()
            .map_err(|e| format!("{e:?}"))?
            .raw();
        let expected = ErlangTerms::parse(&row["expected_groups"])?;
        let matched =
            compare_map_keys(actual.arena(), actual.id(), &expected, expected.root)?.is_eq();
        println!(
            "{}",
            json!({"id":row["id"],"matched":matched,"actual":rust_dialyzer_poc::type_exports::arena_to_value(&actual.arena().terms,actual.id())})
        );
    }
    Ok(())
}
