use rust_dialyzer_poc::{
    bifs::registry::Registry,
    closure_dependencies::{ClosureDependencies, DependencyPolicy},
    core::CoreProgram,
    dataflow_nodes::DataflowNodes,
    dependencies::{ImportPlan, PriorPlt},
    labels::{CoreLabel, Labels},
    program::Program,
    source_callgraph::{CallGraphError, GraphVertex, SourceCallGraph},
};
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn run(row: &Value, raw: &str) -> Result<Value, String> {
    let core = CoreProgram::parse(&row["core"].to_string())?;
    let labels = Labels::build(&core, CoreLabel(0))?;
    let program = Program::build(&core)?;
    let registry = Registry::load(include_str!("../../corpus/bifs.json"))?;
    let closures = ClosureDependencies::analyze(
        &core,
        &program,
        &DependencyPolicy::parse(include_str!("../../corpus/dependency_policy.json"))?,
    )?;
    let nodes = DataflowNodes::build(&core, &labels)
        .map_err(|error| CallGraphError::Boundary(error).to_string())?;
    let mut graph = SourceCallGraph::build(&nodes, &program, &closures, &registry)
        .map_err(|e| e.to_string())?;
    graph
        .attach_layout(core.atom_layout.clone().ok_or("layout missing")?)
        .map_err(|e| e.to_string())?;
    let purged = row["prior_plt"]["purged_modules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let prior = PriorPlt::parse(raw, &core, &ImportPlan::before_cleanup(purged))?;
    let before_present = graph
        .named_labels
        .keys()
        .filter(|v| match v {
            GraphVertex::Named {
                module,
                function,
                arity,
            } => {
                use num_traits::ToPrimitive;
                prior
                    .original_membership()
                    .contains(&rust_dialyzer_poc::bifs::registry::Mfa {
                        module: module.clone(),
                        function: function.clone(),
                        arity: arity.to_usize().unwrap(),
                    })
            }
            _ => false,
        })
        .count();
    graph.remove_external().map_err(|e| e.to_string())?;
    let library = prior.after_cleanup(&graph)?;
    let names = |values: Vec<Value>| {
        let mut values = values;
        values.sort_by_key(Value::to_string);
        values
    };
    let rust_dialyzer_poc::core::ErlangTerm::Map(tables) =
        &library.type_tables.terms[library.type_tables.root.0]
    else {
        panic!("type table module map")
    };
    let type_modules = names(
        tables
            .iter()
            .map(|(key, _)| match &library.type_tables.terms[key.0] {
                rust_dialyzer_poc::core::ErlangTerm::Atom(module) => json!(module),
                _ => panic!("module atom"),
            })
            .collect(),
    );
    let exports = names(
        rust_dialyzer_poc::environment::exported_type_names(&library.exported_types)?
            .iter()
            .map(|n| json!([n.module, n.name, n.arity]))
            .collect(),
    );
    let info = names(
        library
            .summary_names()
            .map(|m| json!([m.module, m.function, m.arity]))
            .collect(),
    );
    let contracts = names(
        library
            .contracts
            .keys()
            .map(|m| json!([m.module, m.function, m.arity]))
            .collect(),
    );
    let expected = |key: &str| names(row[key].as_array().unwrap().clone());
    let removed = rust_dialyzer_poc::bifs::registry::Mfa {
        module: "lifecycle_beh".into(),
        function: "removed".into(),
        arity: 0,
    };
    let cold = PriorPlt::parse(raw, &core, &ImportPlan::cold_targets(&core))?.into_cold_store()?;
    Ok(
        json!({"id":row["id"],"prior_current_members":before_present,"type_modules_exact":type_modules==expected("library_type_modules"),"exports_exact":exports==expected("library_exports"),"info_exact":info==expected("library_info"),"contracts_exact":contracts==expected("library_contracts"),"removed_result":library.summary(&removed)?.map(|s|s.result.to_value()),"removed_contract":library.contracts.contains_key(&removed),"callbacks_retained":library.callback_rows("lifecycle_beh").map_err(|e|format!("{e:?}"))?.is_some(),"cold_removed_absent":cold.summary(&removed)?.is_none(),"cold_callbacks_absent":cold.callback_rows("lifecycle_beh").map_err(|e|format!("{e:?}"))?.is_none()}),
    )
}
fn main() {
    let raw = std::fs::read_to_string(std::env::args().nth(1).unwrap()).unwrap();
    for line in io::stdin().lock().lines() {
        let row: Value = serde_json::from_str(&line.unwrap()).unwrap();
        println!("{}", run(&row, &raw).unwrap());
    }
}
