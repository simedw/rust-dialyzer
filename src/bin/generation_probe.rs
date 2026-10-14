use rust_dialyzer_poc::{
    bifs::registry::Registry,
    constraints::atomic::{AtomicOutcome, BindingKey, BindingMap},
    constraints::closure_order::{ClosureCodeProfile, ClosureOrderKey},
    constraints::generation::{
        ActiveContract, CallSignature, GenerationCallGraph, GenerationCaptureInputs,
        GenerationOutput, GenerationPhase, Generator, LibraryTypes, PropagatedTypes, RecordTableCaptures,
    },
    constraints::generation_capture_headers::GenerationRuntimeCaptures,
    constraints::normalize::NativeDerivedId,
    core::{CoreExpr, CoreProgram},
    labels::{CoreLabel, Labels},
    lattice::{Type, VariableId},
    program::ModuleId,
};
use serde_json::{Value, json};
use std::io::{self, BufRead};
#[path = "support/generation_headers.rs"]
mod header_boundary;
struct ProbeRecordTables(
    std::collections::BTreeMap<ModuleId, rust_dialyzer_poc::term_order::CaptureEnvironment>,
);
impl RecordTableCaptures for ProbeRecordTables {
    fn module_table(
        &self,
        module: ModuleId,
    ) -> Result<&rust_dialyzer_poc::term_order::CaptureEnvironment, String> {
        self.0
            .get(&module)
            .ok_or_else(|| format!("missing original table for module {}", module.0))
    }
}
struct ProbeLibrary {
    signatures: std::collections::BTreeMap<rust_dialyzer_poc::bifs::registry::Mfa, CallSignature>,
    contracts: std::collections::BTreeMap<
        rust_dialyzer_poc::bifs::registry::Mfa,
        (
            std::sync::Arc<rust_dialyzer_poc::contract_runtime::ContractSummary>,
            rust_dialyzer_poc::term_order::CaptureEnvironment,
        ),
    >,
}
impl LibraryTypes for ProbeLibrary {
    fn signature(
        &self,
        mfa: &rust_dialyzer_poc::bifs::registry::Mfa,
    ) -> Result<Option<CallSignature>, String> {
        Ok(self.signatures.get(mfa).cloned())
    }
    fn active_contract(
        &self,
        mfa: &rust_dialyzer_poc::bifs::registry::Mfa,
    ) -> Result<Option<ActiveContract>, String> {
        self.contracts
            .get(mfa)
            .map(|(summary, capture)| ActiveContract::new(summary.clone(), capture.clone()))
            .transpose()
    }
}
fn identifier(v: &VariableId) -> Value {
    match v {
        VariableId::Integer(n) => json!({"integer":n.to_string()}),
        VariableId::Atom(a) => json!({"atom":a}),
    }
}
fn run(v: &Value) -> Result<Value, String> {
    let core = CoreProgram::parse(&v["program"].to_string())?;
    let labels = Labels::build(&core, CoreLabel(0))?;
    let profile = ClosureCodeProfile::parse(&v["profile"])?;
    let registry = Registry::load(include_str!(
        "/Users/simedw/Documents/ChatGPT/rust-dialyzer/corpus/bifs.json"
    ))
    .map_err(|e| format!("{e:?}"))?;
    let propagated = PropagatedTypes::default();
    let mut records = std::collections::BTreeMap::new();
    for record in v["record_prototypes"].as_array().into_iter().flatten() {
        let module = ModuleId(record["module"].as_u64().ok_or("record module missing")? as usize);
        let tag = record["tag"]
            .as_str()
            .ok_or("record tag missing")?
            .to_owned();
        let arity = record["arity"]
            .as_u64()
            .ok_or("record field arity missing")? as usize;
        let prototype = Type::from_value(&record["type"])?;
        if records.insert((module, tag, arity), prototype).is_some() {
            return Err("duplicate record prototype".into());
        }
    }
    for declaration in v["record_declarations"].as_array().into_iter().flatten() {
        let module = ModuleId(
            declaration["module"]
                .as_u64()
                .ok_or("record declaration module missing")? as usize,
        );
        let tag = declaration["tag"]
            .as_str()
            .ok_or("record declaration tag missing")?
            .to_owned();
        let fields = declaration["fields"]
            .as_array()
            .ok_or("record declaration fields missing")?
            .iter()
            .map(Type::from_value)
            .collect::<Result<Vec<_>, _>>()?;
        let arity = fields.len();
        let mut elements = vec![Type::atom(&tag)];
        elements.extend(fields);
        let prototype = Type::tuple(elements).map_err(|error| error.to_string())?;
        if records.insert((module, tag, arity), prototype).is_some() {
            return Err("duplicate record declaration".into());
        }
    }
    let mut library = ProbeLibrary {
        signatures: std::collections::BTreeMap::new(),
        contracts: std::collections::BTreeMap::new(),
    };
    for signature in v["prior_signatures"].as_array().into_iter().flatten() {
        let mfa = serde_json::from_value(signature["mfa"].clone()).map_err(|e| e.to_string())?;
        let arguments = signature["arguments"]
            .as_array()
            .ok_or("signature arguments missing")?
            .iter()
            .map(Type::from_value)
            .collect::<Result<Vec<_>, _>>()?;
        let returns = Type::from_value(&signature["return"])?;
        if library
            .signatures
            .insert(mfa, CallSignature { arguments, returns })
            .is_some()
        {
            return Err("duplicate signature".into());
        }
    }
    for contract in v["prior_contracts"].as_array().into_iter().flatten() {
        let mfa = serde_json::from_value(contract["mfa"].clone()).map_err(|e| e.to_string())?;
        let alternatives = contract["alternatives"]
            .as_array()
            .ok_or("contract alternatives missing")?
            .iter()
            .map(|alt| {
                let signature = Type::from_value(&alt["signature"])?;
                let constraints = alt["constraints"]
                    .as_array()
                    .ok_or("contract constraints missing")?
                    .iter()
                    .map(|pair| {
                        let xs = pair.as_array().ok_or("contract constraint pair missing")?;
                        if xs.len() != 2 {
                            return Err("constraint pair arity".into());
                        }
                        Ok((Type::from_value(&xs[0])?, Type::from_value(&xs[1])?))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                Ok((signature, constraints))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let summary =
            rust_dialyzer_poc::contract_runtime::ContractSummary::from_parts(alternatives)
                .map_err(|e| e.to_string())?;
        let expected = contract["arguments"]
            .as_array()
            .ok_or("contract arguments missing")?
            .iter()
            .map(Type::from_value)
            .collect::<Result<Vec<_>, _>>()?;
        if summary.arguments != expected {
            return Err("processed contract domains disagree".into());
        }
        let capture =
            rust_dialyzer_poc::term_order::CaptureEnvironment::parse(&contract["captured_record"])?;
        if library
            .contracts
            .insert(mfa, (std::sync::Arc::new(summary), capture))
            .is_some()
        {
            return Err("duplicate contract".into());
        }
    }
    let indexed = if v["indexed_callgraph"].as_bool() == Some(true) {
        let program = rust_dialyzer_poc::program::Program::build_with_contracts(
            &core,
            if v["construct_ignore_runtime"] == true {
                rust_dialyzer_poc::program::TargetContracts::Ignore
            } else {
                rust_dialyzer_poc::program::TargetContracts::Use
            },
        )?;
        let policy =
            rust_dialyzer_poc::closure_dependencies::DependencyPolicy::parse(include_str!(
                "/Users/simedw/Documents/ChatGPT/rust-dialyzer/corpus/dependency_policy.json"
            ))?;
        let dependencies = rust_dialyzer_poc::closure_dependencies::ClosureDependencies::analyze(
            &core, &program, &policy,
        )?;
        Some((program, dependencies))
    } else {
        None
    };
    let entries = core
        .modules
        .iter()
        .enumerate()
        .flat_map(|(module, m)| {
            if let CoreExpr::Module(module_payload) = &m.nodes[m.core.0].expr {
                let rust_dialyzer_poc::core::ModuleExpr { definitions, .. } =
                    module_payload.as_ref();

                definitions
                    .iter()
                    .filter(|(node, _)| {
                        let Some(names) = v["entry_names"].as_array() else {
                            return true;
                        };
                        let CoreExpr::Var(term) = m.nodes[node.0].expr else {
                            return false;
                        };
                        let rust_dialyzer_poc::core::ErlangTerm::Tuple(pair) = &m.terms[term.0]
                        else {
                            return false;
                        };
                        let rust_dialyzer_poc::core::ErlangTerm::Atom(name) = &m.terms[pair[0].0]
                        else {
                            return false;
                        };
                        names.iter().any(|x| x.as_str() == Some(name))
                    })
                    .map(|(v, f)| (ModuleId(module), *v, *f))
                    .collect::<Vec<_>>()
            } else {
                vec![]
            }
        })
        .collect::<Vec<_>>();
    let nodes = rust_dialyzer_poc::dataflow_nodes::DataflowNodes::build(&core, &labels)?;
    let capture_config = v.get("source_capture");
    let runtime = capture_config
        .map(|config| {
            if v["construct_ignore_runtime"] == true {
                GenerationRuntimeCaptures::for_analysis(
                    "source_phase".into(),
                    labels.next_label,
                    &indexed.as_ref().ok_or("indexed program required")?.0,
                )
            } else {
                header_boundary::parse(&config["headers"])
            }
        })
        .transpose()?;
    let mut record_tables = ProbeRecordTables(std::collections::BTreeMap::new());
    let mut initial_cache = Vec::new();
    let mut solvers = Vec::new();
    if let Some(config) = capture_config {
        for table in config["record_tables"]
            .as_array()
            .ok_or("original record table captures missing")?
        {
            let module =
                ModuleId(table["module"].as_u64().ok_or("table module ID missing")? as usize);
            record_tables.0.insert(module, {
                let source = rust_dialyzer_poc::core::ErlangTerms::parse(&table["source_terms"])?;
                rust_dialyzer_poc::term_order::CaptureEnvironment::literal_roots(
                    &source,
                    &[source.root],
                )?
            });
        }
        for module in config["initial_record_cache"]
            .as_array()
            .ok_or("initial cache order missing")?
        {
            initial_cache.push(ModuleId(
                module.as_u64().ok_or("cache module missing")? as usize
            ));
        }
        solvers = config["solvers"]
            .as_array()
            .ok_or("source solvers missing")?
            .iter()
            .map(|s| {
                s.as_str()
                    .map(str::to_owned)
                    .ok_or("solver atom missing".to_owned())
            })
            .collect::<Result<_, _>>()?;
    }
    let capture_inputs = runtime.as_ref().map(|runtime| GenerationCaptureInputs {
        core_captures: None,
        runtime,
        records: &record_tables,
        initial_record_cache: &initial_cache,
        solvers: &solvers,
    });
    if v["analyze_scc"].as_bool() == Some(true) {
        let (program, dependencies) = indexed.as_ref().ok_or("SCC graph input absent")?;
        let graph = rust_dialyzer_poc::source_callgraph::SourceCallGraph::build(&nodes, program, dependencies, &registry)
        .map_err(|error| format!("{error:?}"))?;
        let mfas = v["entry_mfas"]
            .as_array()
            .ok_or("SCC MFAs missing")?
            .iter()
            .map(|value| serde_json::from_value(value.clone()).map_err(|error| error.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        let requested = v["solvers"]
            .as_array()
            .ok_or("SCC solver selection missing")?
            .iter()
            .map(|name| {
                name.as_str()
                    .map(str::to_owned)
                    .ok_or("SCC solver is not an atom".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let solvers =
            rust_dialyzer_poc::constraints::analyze_scc::SolverSelection::parse(&requested)
                .map_err(|error| format!("{error:?}"))?;
        let result = rust_dialyzer_poc::constraints::analyze_scc::SccAnalyzer {
            nodes: &nodes,
            profile: &profile,
            provider: &registry,
            graph: &graph,
            phase: GenerationPhase {
                captures: capture_inputs.as_ref(),
                callgraph: GenerationCallGraph::Indexed {
                    program,
                    dependencies,
                },
                propagated: &propagated,
                library: &library,
                records: &records,
                self_recursive: None,
            },
        }
        .analyze(
            &mfas,
            CoreLabel(
                v["phase_next_label"]
                    .as_u64()
                    .ok_or("phase label missing")?,
            ),
            &solvers,
            GenerationOutput::WithSourceState,
        );
        let result = match result {
            Ok(result) => result,
            Err(rust_dialyzer_poc::constraints::analyze_scc::AnalyzeSccError::Native(
                rust_dialyzer_poc::native_error::NativeTransferError::FunctionClause,
            )) => {
                return Ok(
                    json!({"status":"native_error","class":"error","reason":"function_clause"}),
                );
            }
            Err(error) => return Err(format!("{error:?}")),
        };
        if let Some(expected) = v.get("native_source_state") {
            let expected = rust_dialyzer_poc::term_order::CaptureEnvironment::parse(expected)?;
            if !result
                .source_state
                .as_ref()
                .ok_or("SCC state missing")?
                .compare(&expected)?
                .is_eq()
            {
                return Err("native SCC source state mismatch".into());
            }
        }
        let bindings = result
            .bindings
            .iter()
            .map(|(key, typ)| match key {
                BindingKey::Label(label) => Ok(json!({"id":identifier(&label),"type":typ})),
                _ => Err("unexpected SCC ordinary binding key".to_owned()),
            })
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(
            json!({"status":"analyzed","bindings":bindings,"refold_allocations":result.refold_allocations,"source_state_matched":true}),
        );
    }
    let generated = Generator::new(
        &nodes,
        &profile,
        &registry,
        GenerationPhase {
            captures: capture_inputs.as_ref(),
            callgraph: match &indexed {
                Some((program, dependencies)) => GenerationCallGraph::Indexed {
                    program,
                    dependencies,
                },
                None => GenerationCallGraph::Absent,
            },
            propagated: &propagated,
            library: &library,
            records: &records,
            self_recursive: None,
        },
    )
    .map_err(|e| format!("{e:?}"))?
    .generate(&entries, GenerationOutput::WithSourceState)
    .map_err(|e| format!("{e:?}"))?;
    if let Some(expected) = v.get("native_source_state") {
        let expected = rust_dialyzer_poc::term_order::CaptureEnvironment::parse(expected)?;
        let actual = generated
            .source_state
            .as_ref()
            .ok_or("source state capture missing")?;
        if !actual.compare(&expected)?.is_eq() {
            return Err("native source state mismatch".into());
        }
    }
    let graphs=generated.functions.iter().map(|f|Ok(json!({"function":identifier(f.function.variable()),"graph":generated.arena.to_value(f.body).map_err(|e|format!("{e:?}"))?}))).collect::<Result<Vec<_>,String>>()?;
    let mut closure_checks = 0;
    if let Some(expected) = v["native"]["functions"].as_array() {
        for (actual, expected) in graphs.iter().zip(expected) {
            let a = actual["graph"]["nodes"]
                .as_array()
                .ok_or("actual graph nodes")?;
            let b = expected["graph"]["nodes"]
                .as_array()
                .ok_or("native graph nodes")?;
            for (a, b) in a.iter().zip(b) {
                if let Some(id) = a["right"]["derived"].as_u64() {
                    let key = generated
                        .arena
                        .derived_order_key(NativeDerivedId(id as usize))
                        .ok_or("generated closure key absent")?;
                    let expected = ClosureOrderKey::parse(&b["right"]["closure"])?;
                    if !key.compare(&expected)?.is_eq() {
                        return Err("generated source closure capture differs".into());
                    }
                    closure_checks += 1;
                }
            }
        }
    }
    let functions = generated
        .functions
        .iter()
        .map(|f| f.function.clone())
        .collect::<Vec<_>>();
    let parameters = generated.parameters.clone();
    let next_label = generated.next_label.0;
    let refold_allocations = generated.refold_allocations;
    let tree = generated.prepare().map_err(|e| format!("{e:?}"))?;
    let map = if functions.len() > 1 {
        tree.prepare_scc(functions.clone())?
            .solve(BindingMap::new(), &registry)
            .map_err(|e| format!("{e:?}"))?
            .bindings
    } else {
        let result = tree
            .solve(BindingMap::new(), &registry)
            .map_err(|e| format!("{e:?}"))?;
        let AtomicOutcome::Solved { mut bindings, .. } = result.outcome else {
            return Err("function reference unexpectedly failed".into());
        };
        if let Some(rec) = &parameters[&functions[0]].recursive_variable {
            let ty = bindings
                .get(&BindingKey::Label(functions[0].variable().clone()))
                .cloned()
                .unwrap_or_else(Type::any);
            bindings.insert(BindingKey::Label(rec.clone()), ty);
        }
        bindings
    };
    let relevant = functions
        .iter()
        .map(|f| f.variable().clone())
        .chain(
            parameters
                .values()
                .filter_map(|p| p.recursive_variable.clone()),
        )
        .collect::<std::collections::BTreeSet<_>>();
    let bindings = map
        .into_iter()
        .filter_map(|(k, t)| match k {
            BindingKey::Label(id) if relevant.contains(&id) => {
                Some(json!({"id":identifier(&id),"type":t.to_value()}))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut output = json!({"status":"generated","functions":graphs,"next_label":next_label,"bindings":bindings,"closure_checks":closure_checks});
    if v["native"].get("refold_allocations").is_some() {
        output["refold_allocations"] = json!(refold_allocations);
    }
    if v.get("native_source_state").is_some() {
        output["source_state_matched"] = json!(true);
    }
    Ok(output)
}
fn main() {
    for line in io::stdin().lock().lines() {
        let result = (|| -> Result<Value, String> {
            let v = serde_json::from_str(&line.map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            run(&v)
        })();
        println!(
            "{}",
            match result {
                Ok(v) => v,
                Err(e) => json!({"error":e}),
            }
        );
    }
}
