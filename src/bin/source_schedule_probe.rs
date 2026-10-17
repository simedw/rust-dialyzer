use rust_dialyzer_poc::{
    bifs::registry::Registry,
    closure_dependencies::{ClosureDependencies, DependencyPolicy},
    constraints::generation_capture_headers::{
        ActiveDigraphCapture, CallGraphCapture, DigraphCapture, GenerationRuntimeCaptures,
        SourceReference,
    },
    core::CoreProgram,
    dataflow_nodes::DataflowNodes,
    labels::{CoreLabel, Labels},
    program::Program,
    source_callgraph::{CallGraphError, GraphVertex, SourceCallGraph},
    source_schedule::{ActiveGraph, ReferenceRole, RuntimeReferenceAllocator, SourceSchedule},
    term_order::RuntimeIdentity,
};
use serde_json::{Value, json};
use std::io::{self, BufRead};
struct FixtureReferences;
fn reference(name: &str) -> SourceReference {
    SourceReference(RuntimeIdentity {
        scope: "explicit-reference-fixture".into(),
        object: name.into(),
    })
}
impl RuntimeReferenceAllocator for FixtureReferences {
    type Error = std::convert::Infallible;
    fn allocate(
        &mut self,
        role: ReferenceRole,
        epoch: u64,
    ) -> Result<SourceReference, Self::Error> {
        Ok(reference(&format!("active/{epoch}/{role:?}")))
    }
}
fn runtime() -> GenerationRuntimeCaptures {
    GenerationRuntimeCaptures {
        callgraph: Some(CallGraphCapture {
            digraph: DigraphCapture {
                vertices: reference("base/v"),
                edges: reference("base/e"),
                neighbours: reference("base/n"),
                cyclic: true,
            },
            active: ActiveDigraphCapture::Undefined,
            escaping: reference("escaping"),
            letrec: reference("letrec"),
            names: reference("names"),
            reverse_names: reference("reverse_names"),
            recursive_variables: reference("recursive_variables"),
            self_recursive: reference("self_recursive"),
            calls: reference("calls"),
        }),
        codeserver: None,
        plt: None,
    }
}
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
    let mut graph = match SourceCallGraph::build(&nodes, &program, &deps, &registry) {
        Ok(g) => g,
        Err(rust_dialyzer_poc::source_callgraph::CallGraphError::Native(e)) => {
            return Ok(json!({"id":row["id"],"error":e.report()}));
        }
        Err(e) => return Err(e.to_string()),
    };
    graph
        .attach_layout(
            core.atom_layout
                .clone()
                .ok_or("sameVM primitive layout required")?,
        )
        .map_err(|e| e.to_string())?;
    graph.remove_external().map_err(|e| e.to_string())?;
    let mut schedule =
        SourceSchedule::new(runtime(), FixtureReferences).map_err(|e| format!("{e:?}"))?;
    let mut differences = Vec::new();
    for (index, step) in row["steps"].as_array().ok_or("steps")?.iter().enumerate() {
        let changed = step
            .get("changed")
            .and_then(Value::as_array)
            .map(|vs| {
                vs.iter()
                    .map(|v| {
                        v.as_str()
                            .and_then(|v| v.parse::<u64>().ok())
                            .map(CoreLabel)
                            .ok_or("label")
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?
            .unwrap_or_default();
        let kind = step["kind"].as_str().ok_or("step kind")?;
        let actual = match kind {
            "finalize" | "reset" => {
                let sccs = if kind == "finalize" {
                    rust_dialyzer_poc::core_analysis::PhaseSchedule::finalize(&mut schedule, &graph)
                } else {
                    rust_dialyzer_poc::core_analysis::PhaseSchedule::reset_from_functions(
                        &mut schedule,
                        &graph,
                        &changed,
                    )
                }
                .map_err(|e| format!("{e:?}"))?;
                let ActiveGraph::Outgoing(out) = schedule.active() else {
                    return Err("active outgoing".into());
                };
                json!({"kind":kind,"order":sccs.iter().map(|s|json!({"identity":s.identity,"members":s.members.iter().map(wire).collect::<Vec<_>>()})).collect::<Vec<_>>(),"depends":sccs.iter().map(|s|(s.identity.to_string(),json!(out.get(&s.identity).cloned().unwrap_or_default()))).collect::<serde_json::Map<_,_>>()})
            }
            "modules" => {
                let modules =
                    rust_dialyzer_poc::core_analysis::PhaseSchedule::modules_from_functions(
                        &mut schedule,
                        &graph,
                        &changed,
                    )
                    .map_err(|e| format!("{e:?}"))?;
                let ActiveGraph::Modules(active) = schedule.active() else {
                    return Err("active modules".into());
                };
                let names = modules
                    .iter()
                    .map(|m| graph.module_names[m.0].clone())
                    .collect::<Vec<_>>();
                let mut depends = serde_json::Map::new();
                for name in &names {
                    let neighbours = active
                        .outgoing(
                            &rust_dialyzer_poc::native_runtime_hash::NativeHashKey::Atom(
                                name.clone(),
                            ),
                        )
                        .unwrap_or_default()
                        .iter()
                        .map(|key| match key {
                            rust_dialyzer_poc::native_runtime_hash::NativeHashKey::Atom(name) => {
                                Ok(name.clone())
                            }
                            _ => Err("module atom"),
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    depends.insert(name.clone(), json!(neighbours));
                }
                json!({"kind":kind,"order":names,"depends":depends})
            }
            _ => return Err("operation".into()),
        };
        let mut expected = step.clone();
        expected
            .as_object_mut()
            .ok_or("step object")?
            .remove("changed");
        if actual != expected {
            differences.push(json!({"step":index,"expected":expected,"actual":actual}));
        }
    }
    Ok(json!({"id":row["id"],"matches":differences.is_empty(),"differences":differences}))
}
fn main() {
    for line in io::stdin().lock().lines() {
        let out = line
            .map_err(|e| e.to_string())
            .and_then(|s| serde_json::from_str::<Value>(&s).map_err(|e| e.to_string()))
            .and_then(|r| run(&r));
        println!("{}", out.unwrap_or_else(|e| json!({"error":e})));
    }
}
