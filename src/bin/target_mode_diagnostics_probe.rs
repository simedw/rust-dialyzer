use rust_dialyzer_poc::{
    bifs::registry::{Mfa, Registry},
    call_transfer::Signature,
    closure_dependencies::{ClosureDependencies, DependencyPolicy},
    compiled_definitions::PreparedDefinitions,
    core::{CoreProgram, ErlangTerms},
    dataflow_nodes::DataflowNodes,
    diagnostic_inputs::{DiagnosticInputError, DiagnosticInputs, TargetCallbackState},
    labels::{CoreLabel, Labels},
    lattice::Type,
    native_runtime_hash::NativeAtomLayout,
    program::{FunctionIdentity, ModuleId, NodeRef, Program},
    source_callgraph::SourceCallGraph,
    term_order::compare_map_keys,
    warning_payload::NativeWarning,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{self, BufRead},
    sync::Arc,
};
fn matches(w: &[NativeWarning], v: &Value) -> bool {
    let raw = NativeWarning::list(w);
    let expected = Arc::new(ErlangTerms::parse(v).unwrap());
    compare_map_keys(raw.arena(), raw.id(), &expected, expected.root)
        .unwrap()
        .is_eq()
}
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let library_data = std::fs::read_to_string(path).unwrap();
    let registry = Registry::load(include_str!("../../corpus/bifs.json")).unwrap();
    let mut schema_checked = false;
    let mut preparations = BTreeMap::<String, Arc<PreparedDefinitions>>::new();
    let mut libraries = BTreeMap::new();
    for line in io::stdin().lock().lines() {
        let row: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let core = CoreProgram::parse(&row["core"].to_string()).unwrap();
        let program = Program::build_with_contracts(
            &core,
            if row["use_contracts"].as_bool() == Some(false) {
                rust_dialyzer_poc::program::TargetContracts::Ignore
            } else {
                rust_dialyzer_poc::program::TargetContracts::Use
            },
        )
        .unwrap();
        let labels = Labels::build(&core, CoreLabel(0)).unwrap();
        let nodes = DataflowNodes::build(&core, &labels).unwrap();
        let deps = ClosureDependencies::analyze(
            &core,
            &program,
            &DependencyPolicy::parse(include_str!("../../corpus/dependency_policy.json")).unwrap(),
        )
        .unwrap();
        let graph = SourceCallGraph::build(&nodes, &program, &deps, &registry).unwrap();
        let library_key = core
            .modules
            .iter()
            .map(|m| m.name.clone())
            .collect::<Vec<_>>();
        let library = libraries.entry(library_key.clone()).or_insert_with(|| {
            rust_dialyzer_poc::dependencies::PriorPlt::parse(
                &library_data,
                &core,
                &rust_dialyzer_poc::dependencies::ImportPlan::cold_targets(&core),
            )
            .and_then(|prior| prior.into_cold_store())
            .unwrap()
        });
        let mut metadata = row["core"].clone();
        metadata.as_object_mut().unwrap().remove("atom_layout");
        let key = format!("{}:{}", row["use_contracts"], metadata);
        if !schema_checked {
            let base: Value = serde_json::from_str(&library_data).unwrap();
            let mut old = base.clone();
            old["version"] = json!(2);
            old.as_object_mut().unwrap().remove("callback_lists");
            let legacy = rust_dialyzer_poc::dependencies::PriorPlt::parse(
                &old.to_string(),
                &core,
                &rust_dialyzer_poc::dependencies::ImportPlan::cold_targets(&core),
            )
            .and_then(|prior| prior.into_cold_store())
            .unwrap();
            assert!(matches!(
                legacy.callback_rows("gen_server"),
                Err(rust_dialyzer_poc::dependencies::CallbackLookupError::OrderingUnavailable)
            ));
            let mut absent = base.clone();
            absent.as_object_mut().unwrap().remove("callback_lists");
            assert!(
                rust_dialyzer_poc::dependencies::PriorPlt::parse(
                    &absent.to_string(),
                    &core,
                    &rust_dialyzer_poc::dependencies::ImportPlan::cold_targets(&core)
                )
                .and_then(|prior| prior.into_cold_store())
                .is_err()
            );
            let mut duplicate = base.clone();
            let first = duplicate["callback_lists"][0].clone();
            duplicate["callback_lists"]
                .as_array_mut()
                .unwrap()
                .push(first);
            assert!(
                rust_dialyzer_poc::dependencies::PriorPlt::parse(
                    &duplicate.to_string(),
                    &core,
                    &rust_dialyzer_poc::dependencies::ImportPlan::cold_targets(&core)
                )
                .and_then(|prior| prior.into_cold_store())
                .is_err()
            );
            assert!(
                library
                    .callback_rows("adapter_unknown_behaviour")
                    .unwrap()
                    .is_none()
            );
            for group in base["callback_lists"].as_array().unwrap() {
                if group["mfas"].as_array().unwrap().is_empty() {
                    assert!(
                        library
                            .callback_rows(group["module"].as_str().unwrap())
                            .unwrap()
                            .is_some_and(|rows| rows.is_empty())
                    );
                }
            }
            schema_checked = true;
        }
        let completion = rust_dialyzer_poc::compiled_definitions::ModuleCompletionOrder::checked(
            &core,
            row["events"]
                .as_array()
                .unwrap()
                .iter()
                .map(|name| {
                    ModuleId(
                        core.modules
                            .iter()
                            .position(|m| m.name == name.as_str().unwrap())
                            .unwrap(),
                    )
                })
                .collect(),
        )
        .unwrap();
        let key = format!("{key}:{}", row["events"]);
        let definitions = if let Some(d) = preparations.get(&key) {
            d.clone()
        } else {
            match PreparedDefinitions::prepare_with_completion(
                &core,
                &program,
                library,
                &completion,
            ) {
                Ok(d) => {
                    let d = Arc::new(d);
                    preparations.insert(key, d.clone());
                    d
                }
                Err(e) => {
                    println!(
                        "{}",
                        json!({"id":row["id"],"preparation_error":e.to_string()})
                    );
                    continue;
                }
            }
        };
        if program.target_contracts() == rust_dialyzer_poc::program::TargetContracts::Ignore {
            assert!(
                definitions.contracts.is_empty()
                    && definitions.callbacks.is_empty()
                    && definitions.forms.contracts.is_empty()
            );
            assert!(
                row["native_temp_contract_modules"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
        }
        for (module, name) in core
            .modules
            .iter()
            .enumerate()
            .map(|(i, m)| (ModuleId(i), &m.name))
        {
            let actual = definitions.module_table(module).unwrap();
            let expected = ErlangTerms::parse(&row["expected_tables"][name]).unwrap();
            assert!(
                compare_map_keys(actual.arena(), actual.root(), &expected, expected.root)
                    .unwrap()
                    .is_eq()
            );
        }
        let exports = definitions
            .exported_types
            .iter()
            .map(|n| json!([n.module, n.name, n.arity]))
            .collect::<Vec<_>>();
        assert_eq!(json!(exports), row["expected_exported_types"]);
        if program.target_contracts() == rust_dialyzer_poc::program::TargetContracts::Ignore {
            assert!(
                definitions.records.is_empty()
                    && definitions.external_types.is_empty()
                    && definitions.external_type_occurrences.is_empty()
            );
            assert_eq!(row["final_record_table_size"], json!(0));
            assert_eq!(definitions.tables.len(), core.modules.len());
        }
        let world = rust_dialyzer_poc::body_calls::CallWorld::from_library(library);
        for imported in row["expected_imported"].as_array().unwrap() {
            let name: Mfa = serde_json::from_value(imported["mfa"].clone()).unwrap();
            let contract = world.active_contract(&name).unwrap();
            if imported["capture"].is_null() {
                assert!(contract.is_none());
            } else {
                let expected = ErlangTerms::parse(&imported["capture"]).unwrap();
                let actual = &contract.as_ref().unwrap().capture;
                assert!(
                    compare_map_keys(actual.arena(), actual.id(), &expected, expected.root)
                        .unwrap()
                        .is_eq()
                );
            }
        }
        let requested = row["signatures"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| {
                (
                    serde_json::from_value::<Mfa>(s["mfa"].clone()).unwrap(),
                    Signature {
                        arguments: s["arguments"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|v| Type::from_value(v).unwrap())
                            .collect(),
                        result: Type::from_value(&s["result"]).unwrap(),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        let signatures = program
            .functions
            .iter()
            .filter_map(|f| {
                let FunctionIdentity::Named(n) = &f.identity else {
                    return None;
                };
                let mfa = Mfa {
                    module: n.module.clone().into(),
                    function: n.name.clone().into(),
                    arity: n.arity,
                };
                requested.get(&mfa).map(|s| {
                    (
                        labels
                            .label(NodeRef {
                                module: f.module,
                                node: f.node.0,
                            })
                            .unwrap(),
                        s.clone(),
                    )
                })
            })
            .collect();
        let layout = NativeAtomLayout::parse(&row["core"]["atom_layout"]).unwrap();
        let mut callbacks = TargetCallbackState::new(&definitions);
        for _ in definitions.callbacks.write_events() {
            callbacks.store_next(&layout).unwrap();
        }
        let repeated_event = matches!(
            callbacks.store_next(&layout),
            Err(DiagnosticInputError::Boundary(_))
        );
        let mut other_scope = layout.clone();
        other_scope.scope.push_str("-other");
        let changed_scope = matches!(
            callbacks.store_next(&other_scope),
            Err(DiagnosticInputError::Boundary(_))
        );
        let unprepared = TargetCallbackState::new(&definitions);
        let incomplete_inputs = DiagnosticInputs {
            nodes: &nodes,
            program: &program,
            callgraph: &graph,
            library,
            signatures: &signatures,
            atom_layout: &layout,
            target_callbacks: &unprepared,
        };
        let has_callbacks = !definitions.callbacks.is_empty();
        let missing_event = !has_callbacks
            || matches!(
                incomplete_inputs
                    .prepare(&(0..core.modules.len()).map(ModuleId).collect::<Vec<_>>()),
                Err(DiagnosticInputError::CallbackPreparationMissing(_))
            );
        assert!(repeated_event && changed_scope && missing_event);
        let actualrows = callbacks
            .rows()
            .into_iter()
            .map(|entry| {
                let n = entry.key.callable_mfa().expect("typed callback fixture");
                json!([n.module, n.function, n.arity])
            })
            .collect::<Vec<_>>();
        let order_matched = json!(actualrows) == row["expected_target_callbacks"];
        let inputs = DiagnosticInputs {
            nodes: &nodes,
            program: &program,
            callgraph: &graph,
            library,
            signatures: &signatures,
            atom_layout: &layout,
            target_callbacks: &callbacks,
        };
        let modules = (0..core.modules.len()).map(ModuleId).collect::<Vec<_>>();
        match inputs.prepare(&modules).and_then(|session| {
            let contracts = session.contracts(&registry)?;
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
        }) {
            Ok(result) => {
                let matched = matches(&result.contracts, &row["expected_invalid"])
                    && result
                        .modules
                        .iter()
                        .zip(row["expected_modules"].as_array().unwrap())
                        .all(|(m, n)| {
                            core.modules[m.module.0].name == n["module"].as_str().unwrap()
                                && matches(&m.callbacks, &n["callbacks"])
                                && matches(&m.missing_contract_functions, &n["missing"])
                        });
                println!(
                    "{}",
                    json!({"id":row["id"],"use_contracts":row["use_contracts"],"imported_contracts_matched":true,"target_contracts":definitions.contracts.len(),"target_callbacks":definitions.callbacks.len(),"matched":matched,"event_boundaries":repeated_event && changed_scope && missing_event,"schema_boundaries":schema_checked,"callback_order_matched":order_matched,"contract_warnings":result.contracts.len(),"callback_warnings":result.modules.iter().map(|m|m.callbacks.len()).sum::<usize>(),"missing_warnings":result.modules.iter().map(|m|m.missing_contract_functions.len()).sum::<usize>()})
                );
            }
            Err(e) => println!("{}", json!({"id":row["id"],"error":format!("{e:?}")})),
        }
    }
}
