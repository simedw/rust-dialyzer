use rust_dialyzer_poc::{
    bifs::registry::Registry,
    closure_dependencies::{ClosureDependencies, DependencyPolicy},
    compiled_definitions::PreparedDefinitions,
    core::{CoreProgram, ErlangTerms},
    dataflow_nodes::DataflowNodes,
    dependencies::{ImportPlan, PriorPlt},
    diagnostic_inputs::{DiagnosticInputError, DiagnosticInputs, TargetCallbackState},
    labels::{CoreLabel, Labels},
    native_runtime_hash::NativeAtomLayout,
    program::{ModuleId, Program},
    source_callgraph::SourceCallGraph,
    term_order::compare_map_keys,
    warning_payload::NativeWarning,
    warning_policy::Policy,
    warning_publication,
    warning_render::LocationOption,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{self, BufRead},
};
fn main() -> Result<(), String> {
    let library = std::fs::read_to_string(std::env::args().nth(1).ok_or("library")?)
        .map_err(|e| e.to_string())?;
    let bifs = Registry::load(include_str!("../../corpus/bifs.json"))?;
    let warnings =
        rust_dialyzer_poc::warnings::Registry::load(include_str!("../../corpus/warnings.json"))?;
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
        let graph = SourceCallGraph::build(&nodes, &program, &deps, &bifs)
            .map_err(|e| format!("{e:?}"))?;
        let layout =
            NativeAtomLayout::parse(&row["core"]["atom_layout"]).map_err(|e| e.to_string())?;
        let callbacks = TargetCallbackState::from_definitions(&definitions, &layout)
            .map_err(|error| format!("{error:?}"))?;
        let signatures = BTreeMap::new();
        let inputs = DiagnosticInputs {
            nodes: &nodes,
            program: &program,
            callgraph: &graph,
            library: &library,
            signatures: &signatures,
            atom_layout: &layout,
            target_callbacks: &callbacks,
        };
        let expected = if row.get("analysis").is_some() {
            &row["analysis"]
        } else {
            &row
        };
        let modules = (0..core.modules.len()).map(ModuleId).collect::<Vec<_>>();
        let result = inputs.prepare(&modules).and_then(|session| {
            let contracts = session.contracts(&bifs)?;
            let modules = modules
                .into_iter()
                .map(|module| {
                    Ok(
                        rust_dialyzer_poc::diagnostic_inputs::ModuleDiagnosticWarnings {
                            module,
                            missing_contract_functions: session
                                .missing_contract_functions(module)?,
                            callbacks: session.callbacks(module)?,
                        },
                    )
                })
                .collect::<Result<Vec<_>, DiagnosticInputError>>()?;
            Ok(rust_dialyzer_poc::diagnostic_inputs::DiagnosticWarnings { contracts, modules })
        });
        let output = match result {
            Err(DiagnosticInputError::Form(e)) => {
                json!({"id":row["id"],"status":"error","error":e.report(),"matched":expected["status"]=="error" && expected["reason"].as_str().is_some_and(|s|s.contains("function_clause")&&s.contains("dialyzer_plt"))})
            }
            Err(e) => return Err(format!("{e:?}")),
            Ok(diag) => {
                let mut analysis = diag.contracts;
                for module in diag.modules {
                    analysis.extend(module.missing_contract_functions);
                    analysis.extend(module.callbacks);
                }
                let policy = Policy::build(&core, &warnings, &warnings.default_profile())
                    .map_err(|e| format!("{e:?}"))?;
                let published = warning_publication::publish(
                    &analysis,
                    &[],
                    &[],
                    &definitions,
                    &policy,
                    LocationOption::Column,
                )
                .map_err(|e| format!("{e:?}"))?;
                let events = published
                    .raw
                    .into_iter()
                    .map(|raw| NativeWarning { raw })
                    .collect::<Vec<_>>();
                let actual = NativeWarning::list(&events);
                let original = ErlangTerms::parse(&expected["warnings"])?;
                let matched =
                    compare_map_keys(actual.arena(), actual.id(), &original, original.root)?
                        .is_eq();
                json!({"id":row["id"],"status":"ok","events":events.len(),"matched":matched,"actual":rust_dialyzer_poc::type_exports::arena_to_value(&actual.arena().terms,actual.id())})
            }
        };
        println!("{output}");
    }
    Ok(())
}
