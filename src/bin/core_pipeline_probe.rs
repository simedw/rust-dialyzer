use rust_dialyzer_poc::{
    bifs::registry::Registry,
    closure_dependencies::{ClosureDependencies, DependencyPolicy},
    compiled_definitions::PreparedDefinitions,
    constraints::{analyze_scc::SolverSelection, closure_order::ClosureCodeProfile},
    core::{CoreProgram, ErlangTerm, ErlangTerms},
    core_phases::CorePhaseInputs,
    core_pipeline,
    dataflow_nodes::DataflowNodes,
    diagnostic_inputs::TargetCallbackState,
    folded_core::FoldedCore,
    labels::{CoreLabel, Labels},
    lattice::Type,
    native_form_environment::RawFormTerm,
    program::Program,
    source_callgraph::SourceCallGraph,
    success_phase::FunctionTypes,
    term_order::compare_map_keys,
    warning_payload::NativeWarning,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{self, BufRead},
};

fn module_warnings(raw: &RawFormTerm) -> Result<BTreeMap<String, Vec<NativeWarning>>, String> {
    let mut result = BTreeMap::<String, Vec<NativeWarning>>::new();
    let mut cursor = raw.id();
    loop {
        match &raw.arena().terms[cursor.0] {
            ErlangTerm::Nil => return Ok(result),
            ErlangTerm::List { items, tail } => {
                for id in items {
                    let warning = RawFormTerm::new(raw.arena().clone(), *id)?;
                    let ErlangTerm::Tuple(fields) = warning.term() else {
                        return Err("warning is not a tuple".into());
                    };
                    let Some(ErlangTerm::Tuple(info)) =
                        fields.get(1).map(|id| &raw.arena().terms[id.0])
                    else {
                        return Err("warning info is not a tuple".into());
                    };
                    let origin = info.get(2).ok_or("warning origin missing")?;
                    let module = match &raw.arena().terms[origin.0] {
                        ErlangTerm::Atom(module) => module,
                        ErlangTerm::Tuple(mfa) => {
                            match mfa.first().map(|id| &raw.arena().terms[id.0]) {
                                Some(ErlangTerm::Atom(module)) => module,
                                _ => return Err("warning module is not an atom".into()),
                            }
                        }
                        _ => return Err("warning origin is not a module or MFA".into()),
                    };
                    result
                        .entry(module.clone())
                        .or_default()
                        .push(NativeWarning { raw: warning });
                }
                cursor = *tail;
            }
            _ => return Err("warnings are not a proper list".into()),
        }
    }
}

fn signatures(types: &FunctionTypes, graph: &SourceCallGraph) -> Value {
    json!(
        types
            .iter()
            .filter_map(|(label, signature)| {
                graph.function_name(*label).map(|name| json!({
            "module":name.module,"function":name.function,"arity":name.arity,
            "arguments":signature.arguments.iter().map(Type::to_value).collect::<Vec<_>>(),
            "result":signature.result.to_value()
        }))
            })
            .collect::<Vec<_>>()
    )
}

fn run(row: &Value, library: &str, profile: &ClosureCodeProfile) -> Result<Value, String> {
    let core = CoreProgram::parse(&row["core"].to_string())?;
    let layout = core
        .atom_layout
        .as_ref()
        .ok_or("compiler atom layout missing")?;
    let labels = Labels::build(&core, CoreLabel(0))?;
    let nodes = DataflowNodes::build(&core, &labels)?;
    let program = Program::build(&core)?;
    let bifs = Registry::standard()?;
    let dependencies = ClosureDependencies::analyze(
        &core,
        &program,
        &DependencyPolicy::parse(include_str!("../../corpus/dependency_policy.json"))?,
    )?;
    let mut graph = SourceCallGraph::build(&nodes, &program, &dependencies, &bifs)
        .map_err(|error| error.to_string())?;
    graph
        .attach_layout(layout.clone())
        .map_err(|error| error.to_string())?;
    let plan = if let Some(lifecycle) = row.get("prior_plt") {
        let modules = lifecycle
            .get("purged_modules")
            .and_then(Value::as_array)
            .ok_or("prior_plt requires explicit purged_modules")?
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_owned)
                    .ok_or("purged module must be an atom name")
            })
            .collect::<Result<Vec<_>, _>>()?;
        rust_dialyzer_poc::dependencies::ImportPlan::before_cleanup(modules)
    } else {
        rust_dialyzer_poc::dependencies::ImportPlan::cold_targets(&core)
    };
    let prior = rust_dialyzer_poc::dependencies::PriorPlt::parse(library, &core, &plan)?;
    let external = graph.remove_external().map_err(|error| error.to_string())?;
    let library = if row.get("prior_plt").is_some() {
        prior.after_cleanup(&graph)?
    } else {
        prior.into_cold_store()?
    };
    let definitions = PreparedDefinitions::prepare(&core, &program, &library)
        .map_err(|error| error.to_string())?;
    let callbacks = TargetCallbackState::from_definitions(&definitions, layout)
        .map_err(|error| format!("{error:?}"))?;
    let folded = FoldedCore::build(&core);
    let solvers: Vec<String> = row
        .get("solvers")
        .map(|value| serde_json::from_value(value.clone()))
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or_default();
    let result = core_pipeline::analyze(
        CorePhaseInputs {
            nodes: &nodes,
            program: &program,
            dependencies: &dependencies,
            graph: &graph,
            definitions: &definitions,
            profile,
            folded: &folded,
            bifs: &bifs,
        },
        &library,
        &callbacks,
        layout,
        format!("rust-analysis/{}", layout.scope),
        SolverSelection::parse(&solvers).map_err(|error| format!("{error:?}"))?,
        if row["analysis_mode"] == "plt_build" {
            core_pipeline::AnalysisMode::PltBuild
        } else {
            core_pipeline::AnalysisMode::SuccessTypings
        },
        None,
    )
    .map_err(|error| format!("{error:?}"))?;
    let raw = NativeWarning::list(&result.raw_warnings);
    let matches = if let Some(expected) = row.get("warnings") {
        let expected = ErlangTerms::parse(expected)?;
        Some(compare_map_keys(raw.arena(), raw.id(), &expected, expected.root)?.is_eq())
    } else {
        None
    };
    let module_matches = row
        .get("warnings")
        .map(|expected| {
            let arena = std::sync::Arc::new(ErlangTerms::parse(expected)?);
            let expected = RawFormTerm::new(arena.clone(), arena.root)?;
            Ok::<_, String>(module_warnings(&raw)? == module_warnings(&expected)?)
        })
        .transpose()?;
    Ok(json!({
        "id":row["id"],
        "signatures":signatures(&result.state.signatures, &graph),
        "documentation":result.documentation.as_ref().map(|types|signatures(types, &graph)),
        "raw_warning_count":result.raw_warnings.len(),
        "raw_warnings_match":matches,
        "raw_module_warnings_match":module_matches,
        "external_edges":external.len()
    }))
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let library = std::fs::read_to_string(&args[0]).expect("library snapshot");
    let profile = ClosureCodeProfile::parse(
        &serde_json::from_str::<Value>(&std::fs::read_to_string(&args[1]).expect("profile"))
            .expect("profile JSON"),
    )
    .expect("closure profile");
    for line in io::stdin().lock().lines() {
        let result = line
            .map_err(|error| error.to_string())
            .and_then(|line| serde_json::from_str(&line).map_err(|error| error.to_string()))
            .and_then(|row| run(&row, &library, &profile));
        println!("{}", result.unwrap_or_else(|error| json!({"error":error})));
    }
}
