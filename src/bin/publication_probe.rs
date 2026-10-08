use rust_dialyzer_poc::{
    bifs::registry::Registry,
    closure_dependencies::{ClosureDependencies, DependencyPolicy},
    compiled_definitions::PreparedDefinitions,
    core::CoreProgram,
    dataflow_nodes::DataflowNodes,
    labels::{CoreLabel, Labels},
    module_publication::PublicationInput,
    program::{ModuleId, Program, TargetContracts},
    source_callgraph::{CallGraphError, SourceCallGraph},
};
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn run(row: &Value) -> Result<Value, String> {
    let core = CoreProgram::parse(
        &std::fs::read_to_string(row["proof_core"].as_str().ok_or("proof core")?)
            .map_err(|e| e.to_string())?,
    )?;
    let program = Program::build(&core)?;
    let library = rust_dialyzer_poc::dependencies::PriorPlt::parse(
        &std::fs::read_to_string(row["environment"].as_str().ok_or("environment")?)
            .map_err(|e| e.to_string())?,
        &core,
        &rust_dialyzer_poc::dependencies::ImportPlan::cold_targets(&core),
    )
    .and_then(|prior| prior.into_cold_store())?;
    let definitions =
        PreparedDefinitions::prepare(&core, &program, &library).map_err(|e| e.to_string())?;
    let labels = Labels::build(&core, CoreLabel(0))?;
    let closures = ClosureDependencies::analyze(
        &core,
        &program,
        &DependencyPolicy::parse(include_str!("../../corpus/dependency_policy.json"))?,
    )?;
    let registry = Registry::load(include_str!("../../corpus/bifs.json"))?;
    let nodes = DataflowNodes::build(&core, &labels)
        .map_err(|error| CallGraphError::Boundary(error).to_string())?;
    let mut graph = SourceCallGraph::build(&nodes, &program, &closures, &registry)
    .map_err(|e| e.to_string())?;
    graph
        .attach_layout(core.atom_layout.clone().ok_or("atom layout")?)
        .map_err(|e| e.to_string())?;
    let call_dependencies = rust_dialyzer_poc::callgraph_cleanup::module_dependencies(&graph)
        .map_err(|e| e.to_string())?;
    let targets = (0..core.modules.len()).map(ModuleId).collect::<Vec<_>>();
    let mut results = serde_json::Map::new();
    for (name, use_contracts) in [("enabled", true), ("disabled", false)] {
        let input = PublicationInput {
            core: &core,
            program: &program,
            definitions: &definitions,
            targets: &targets,
            use_contracts,
        };
        let types = input
            .type_dependencies()
            .map_err(|e| format!("{e:?}"))?
            .named_projection()?;
        let published = input
            .publish(&call_dependencies)
            .map_err(|e| format!("{e:?}"))?
            .named_projection()?;
        let actual = json!({"types":types,"published":published});
        results.insert(name.into(),json!({"exact":actual==row["result"]["publication"][name],"types":types,"published":published}));
    }
    // Explicit Source compiler-false fixture: these target tables are never populated.
    let ignored_program = Program::build_with_contracts(&core, TargetContracts::Ignore)?;
    let ignored_definitions = PreparedDefinitions::prepare(&core, &ignored_program, &library)
        .map_err(|e| e.to_string())?;
    assert!(ignored_definitions.contracts.is_empty() && ignored_definitions.callbacks.is_empty());
    let input = PublicationInput {
        core: &core,
        program: &ignored_program,
        definitions: &ignored_definitions,
        targets: &targets,
        use_contracts: false,
    };
    let types = input
        .type_dependencies()
        .map_err(|e| format!("{e:?}"))?
        .named_projection()?;
    let published = input
        .publish(&call_dependencies)
        .map_err(|e| format!("{e:?}"))?
        .named_projection()?;
    let actual = json!({"types":types,"published":published});
    results.insert("compiler_disabled".into(),json!({"exact":actual==row["result"]["publication"]["compiler_disabled"],"types":types,"published":published}));
    Ok(json!({"id":row["id"],"publication":results}))
}
fn main() {
    for line in io::stdin().lock().lines() {
        let row: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let out = match run(&row) {
            Ok(v) => v,
            Err(e) => json!({"id":row["id"],"error":e}),
        };
        println!("{out}");
    }
}
