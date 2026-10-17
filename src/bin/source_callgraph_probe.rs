use rust_dialyzer_poc::{
    bifs::registry::Registry,
    closure_dependencies::{ClosureDependencies, DependencyPolicy},
    core::CoreProgram,
    dataflow_nodes::DataflowNodes,
    labels::{CoreLabel, Labels},
    program::{ModuleId, Program},
    source_callgraph::{CallGraphError, GraphVertex, SourceCallGraph},
};
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn wire(v: &GraphVertex) -> Value {
    match v {
        GraphVertex::Label(l) => json!(["label", l.0.to_string()]),
        GraphVertex::Top => json!(["top"]),
        GraphVertex::Named {
            module,
            function,
            arity,
        } => json!(["named", module, function, arity.to_string()]),
    }
}
fn run(row: &Value) -> Result<Value, String> {
    let core = CoreProgram::parse(
        &std::fs::read_to_string(row["proof_core_json"].as_str().ok_or("proof Core")?)
            .map_err(|e| e.to_string())?,
    )?;
    let program = Program::build(&core)?;
    let labels = Labels::build(&core, CoreLabel(0))?;
    let deps = ClosureDependencies::analyze(
        &core,
        &program,
        &DependencyPolicy::parse(include_str!("../../corpus/dependency_policy.json"))?,
    )?;
    let registry = Registry::standard()?;
    let nodes = DataflowNodes::build(&core, &labels)
        .map_err(|error| CallGraphError::Boundary(error).to_string())?;
    let graph = match SourceCallGraph::build(&nodes, &program, &deps, &registry) {
        Ok(graph) => graph,
        Err(rust_dialyzer_poc::source_callgraph::CallGraphError::Native(error)) => {
            return Ok(json!({"id":row["id"],"error":error.report()}));
        }
        Err(error) => return Err(error.to_string()),
    };
    let modules=graph.modules.iter().enumerate().map(|(i,m)|Ok(json!({"confirmed":m.confirmed.iter().map(wire).collect::<Vec<_>>(),"edges":m.edges.iter().map(|(a,b)|json!([wire(a),wire(b)])).collect::<Vec<_>>(),"escaping":m.escaping.iter().map(|l|l.0.to_string()).collect::<Vec<_>>(),"self_recursive":m.self_recursive.iter().map(wire).collect::<Vec<_>>(),"incoming":graph.ordered_local_callers(ModuleId(i),&program,&labels)?.iter().map(|(label,callers)|(label.0.to_string(),json!(callers.iter().map(|l|l.0.to_string()).collect::<Vec<_>>()))).collect::<serde_json::Map<_,_>>()}))).collect::<Result<Vec<_>,String>>()?;
    let schedule = graph.schedule()?;
    let mut components = schedule
        .components
        .iter()
        .map(|component| {
            let mut members = component
                .members
                .iter()
                .map(|vertex| wire(&schedule.vertices[vertex.0]))
                .collect::<Vec<_>>();
            members.sort_by_key(|v| v.to_string());
            members
        })
        .collect::<Vec<_>>();
    components.sort_by_key(|v| serde_json::to_string(v).unwrap());
    Ok(json!({"id":row["id"],"modules":modules,"components":components}))
}
fn main() {
    for line in io::stdin().lock().lines() {
        let out = match line {
            Ok(s) => serde_json::from_str::<Value>(&s)
                .map_err(|e| e.to_string())
                .and_then(|r| run(&r)),
            Err(e) => Err(e.to_string()),
        };
        println!("{}", out.unwrap_or_else(|e| json!({"error":e})));
    }
}
