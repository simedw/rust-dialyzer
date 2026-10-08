use rust_dialyzer_poc::{
    core::CoreProgram,
    dataflow_nodes::DataflowNodes,
    labels::{CoreLabel, Labels},
    lattice::Type,
    program::ModuleId,
};
use rust_dialyzer_poc::{
    function_state::FunctionState,
    function_tree::{GraphInputs, build_function_state},
    worklist::FunctionKey,
};
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn wire(k: FunctionKey) -> Value {
    match k {
        FunctionKey::Top => json!("top"),
        FunctionKey::Label(n) => json!(n.0),
    }
}
fn snap(s: &FunctionState) -> Value {
    json!({"entries":s.entries().iter().map(|(k,e)|{let sig=e.signature();(match k{FunctionKey::Top=>"top".into(),FunctionKey::Label(n)=>n.0.to_string()},json!({"called":e.called(),"args":sig.arguments.iter().map(Type::to_value).collect::<Vec<_>>(),"result":sig.result.to_value()}))}).collect::<serde_json::Map<String,Value>>(),"environments":s.environments().iter().map(|(key,map)|(match key{FunctionKey::Top=>"top".into(),FunctionKey::Label(n)=>n.0.to_string()},json!(map.types().iter().map(|(k,t)|(k.native_integer(),t.to_value())).collect::<serde_json::Map<String,Value>>()))).collect::<serde_json::Map<String,Value>>(),"queue":s.work().ordered().into_iter().map(wire).collect::<Vec<_>>(),"reachable":s.reachable().iter().map(|n|n.0).collect::<Vec<_>>(),"warning_mode":s.warning_mode()})
}
fn run(row: &Value, derive_graph: bool) -> Result<Value, String> {
    let core = CoreProgram::parse(
        &std::fs::read_to_string(row["proof_core_json"].as_str().ok_or("proof path")?)
            .map_err(|e| e.to_string())?,
    )?;
    let labels = Labels::build(&core, CoreLabel(0))?;
    let nodes = DataflowNodes::build(&core, &labels)?;
    let source_graph = if derive_graph {
        let program = rust_dialyzer_poc::program::Program::build(&core)?;
        let policy = rust_dialyzer_poc::closure_dependencies::DependencyPolicy::parse(
            include_str!("../../corpus/dependency_policy.json"),
        )?;
        let dependencies = rust_dialyzer_poc::closure_dependencies::ClosureDependencies::analyze(
            &core, &program, &policy,
        )?;
        let graph = rust_dialyzer_poc::source_callgraph::SourceCallGraph::build(&nodes, &program,
            &dependencies,
            &rust_dialyzer_poc::bifs::registry::Registry::load(include_str!(
                "../../corpus/bifs.json"
            ))?,
        )
        .map_err(|e| e.to_string())?;
        Some((program, graph))
    } else {
        None
    };
    let mut modules = Vec::new();
    for (index, module) in core.modules.iter().enumerate() {
        let evidence = &row["modules"][index];
        let mut graph = GraphInputs::empty();
        if let Some((program, source)) = &source_graph {
            graph =
                source.function_inputs(ModuleId(index), program, &labels, Default::default())?;
        } else {
            graph.escaping = evidence["escaping"]
                .as_array()
                .ok_or("escaping labels")?
                .iter()
                .map(|x| x.as_u64().map(CoreLabel).ok_or("invalid escaping label"))
                .collect::<Result<_, _>>()?;
        }
        let mut state = build_function_state(&nodes, ModuleId(index), &graph)?;
        let escaping = state
            .order()
            .iter()
            .filter_map(|key| match key {
                FunctionKey::Label(label) if graph.escaping.contains(label) => Some(json!(label.0)),
                _ => None,
            })
            .collect::<Vec<_>>();
        let order = state.order().iter().copied().map(wire).collect::<Vec<_>>();
        let homes = state
            .homes()
            .iter()
            .map(|(label, name)| {
                (
                    label.0.to_string(),
                    json!([name.module, name.name, name.arity]),
                )
            })
            .collect::<serde_json::Map<String, Value>>();
        let initial = snap(&state);
        state.set_warning_mode();
        modules.push(json!({"module":module.name,"order":order,"escaping":escaping,"homes":homes,"initial":initial,"warning":snap(&state)}));
    }
    Ok(json!({"id":row["id"],"modules":modules,"proof_core_json":row["proof_core_json"]}))
}
fn main() {
    let derive_graph = std::env::args().any(|arg| arg == "--derive-graph");
    for line in io::stdin().lock().lines() {
        let row: Value = serde_json::from_str(&line.unwrap()).unwrap();
        println!("{}", run(&row, derive_graph).unwrap());
    }
}
