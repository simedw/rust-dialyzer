use rust_dialyzer_poc::{
    bifs::registry::Registry,
    call_transfer::Signature,
    closure_dependencies::{ClosureDependencies, DependencyPolicy},
    compiled_definitions::PreparedDefinitions,
    constraints::{
        analyze_scc::SolverSelection, closure_order::ClosureCodeProfile,
        generation_capture_headers::GenerationRuntimeCaptures,
    },
    contract_book::ContractBook,
    core::{CoreProgram, ErlangTerms},
    core_analysis::{AnalysisPhases, AnalysisState, ModuleRequest},
    core_phases::{CorePhaseInputs, CorePhases},
    dataflow_nodes::DataflowNodes,
    folded_core::FoldedCore,
    labels::{CoreLabel, Labels},
    lattice::Type,
    program::{ModuleId, NodeRef, Program},
    source_callgraph::SourceCallGraph,
    success_phase::FunctionTypes,
    term_order::compare_map_keys,
    warning_payload::NativeWarning,
};
use serde_json::{Value, json};
use std::{
    cmp::Ordering,
    collections::BTreeMap,
    io::{self, BufRead},
};
fn view(arena: &ErlangTerms, id: rust_dialyzer_poc::core::TermId) -> Value {
    use rust_dialyzer_poc::core::ErlangTerm::*;
    match &arena.terms[id.0] {
        ExternalFun(payload) => json!({"external_fun":{"module":payload.module,"function":payload.function,"arity":payload.arity}}),
        Atom(s) => json!({"atom":s}),
        Integer(s) => json!({"integer":s}),
        Float(s) => json!({"float":s}),
        Binary(s) => json!({"binary":s}),
        Bitstring { bytes, bits } => json!({"bytes":bytes,"bits":bits}),
        Nil => json!([]),
        Tuple(xs) => json!({"tuple":xs.iter().map(|x|view(arena,*x)).collect::<Vec<_>>()}),
        List { items, tail } => {
            json!({"list":items.iter().map(|x|view(arena,*x)).collect::<Vec<_>>(),"tail":view(arena,*tail)})
        }
        Map(xs) => {
            json!({"map":xs.iter().map(|(k,v)|(view(arena,*k),view(arena,*v))).collect::<Vec<_>>()})
        }
    }
}
fn sigmap(types: &FunctionTypes) -> Value {
    json!(types.iter().map(|(label,sig)|(label.0.to_string(),json!({"arguments":sig.arguments.iter().map(Type::to_value).collect::<Vec<_>>(),"result":sig.result.to_value()}))).collect::<serde_json::Map<_,_>>())
}
fn run(r: &Value, library_raw: &str, profile: &ClosureCodeProfile) -> Result<Vec<Value>, String> {
    let members = r["members"].as_array().ok_or("group members")?;
    let r = members.first().ok_or("empty group")?;
    let raw = if let Some(path) = r["core"].as_str() {
        std::fs::read_to_string(path).map_err(|e| e.to_string())?
    } else {
        r["program"].to_string()
    };
    let core = CoreProgram::parse(&raw)?;
    let labels = Labels::build(&core, CoreLabel(0))?;
    let nodes = DataflowNodes::build(&core, &labels)?;
    let program = Program::build(&core)?;
    let bifs = Registry::load(include_str!("../../corpus/bifs.json"))?;
    let deps = ClosureDependencies::analyze(
        &core,
        &program,
        &DependencyPolicy::parse(include_str!("../../corpus/dependency_policy.json"))?,
    )?;
    let graph = SourceCallGraph::build(&nodes, &program, &deps, &bifs)
        .map_err(|e| e.to_string())?;
    let library = rust_dialyzer_poc::dependencies::PriorPlt::parse(
        library_raw,
        &core,
        &rust_dialyzer_poc::dependencies::ImportPlan::cold_targets(&core),
    )
    .and_then(|prior| prior.into_cold_store())?;
    let definitions =
        PreparedDefinitions::prepare(&core, &program, &library).map_err(|e| e.to_string())?;
    let folded = FoldedCore::build(&core);
    let mut phases = CorePhases::new(
        CorePhaseInputs {
            nodes: &nodes,
            program: &program,
            dependencies: &deps,
            graph: &graph,
            definitions: &definitions,
            profile,
            folded: &folded,
            bifs: &bifs,
        },
        &library,
        SolverSelection::Default,
    )
    .map_err(|e| format!("{e:?}"))?;
    let mut output = Vec::new();
    for r in members {
        let mut state = AnalysisState {
            signatures: FunctionTypes::new(),
            contracts: ContractBook::new(BTreeMap::new()),
        };
        for p in r["prior"].as_array().ok_or("prior")? {
            state.signatures.insert(
                CoreLabel(p["label"].as_u64().ok_or("prior label")?),
                Signature {
                    arguments: p["arguments"]
                        .as_array()
                        .ok_or("arguments")?
                        .iter()
                        .map(Type::from_value)
                        .collect::<Result<_, _>>()?,
                    result: Type::from_value(&p["result"])?,
                },
            );
        }
        let module = ModuleId(r["module"].as_u64().unwrap_or(0) as usize);
        let mut previous = FunctionTypes::new();
        for function in program.functions.iter().filter(|f| f.module == module) {
            let label = nodes.label(NodeRef {
                module: function.module,
                node: function.node.0,
            })?;
            previous.insert(
                label,
                state
                    .signatures
                    .get(&label)
                    .cloned()
                    .unwrap_or_else(|| Signature {
                        arguments: vec![Type::any(); function.arity],
                        result: Type::any(),
                    }),
            );
        }
        phases.publish(state.signatures.iter().map(|(label, signature)| (*label, signature)), state.contracts.active_names());
        let runtime = GenerationRuntimeCaptures::default();
        let types = phases
            .refine_module(ModuleRequest {
                runtime: &runtime,
                module,
                previous: &previous,
                state: &state,
            })
            .map_err(|e| format!("{e:?}"))?;
        let diagnostics = phases
            .module_bodies()
            .module_diagnostics(module, &previous, None)
            .map_err(|e| format!("{e:?}"))?;
        let warnings = NativeWarning::list(&diagnostics.warnings);
        let expected = ErlangTerms::parse(&r["warnings"])?;
        let matches = compare_map_keys(warnings.arena(), warnings.id(), &expected, expected.root)?
            == Ordering::Equal;
        if r["debug"].as_bool().unwrap_or(false) {
            eprintln!(
                "{}",
                json!({"expected":view(&expected,expected.root),"actual":view(warnings.arena(),warnings.id())})
            );
        }
        output.push(
        json!({"id":r["id"],"native":{"status":"ok","types":sigmap(&types),"warning_types":sigmap(&diagnostics.signatures),"warnings_match":matches,"warning_count":diagnostics.warnings.len()}}),
    )
    }
    Ok(output)
}
fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let library = std::fs::read_to_string(&args[0]).expect("library artifact");
    let profile = ClosureCodeProfile::parse(
        &serde_json::from_str::<Value>(&std::fs::read_to_string(&args[1]).expect("profile"))
            .expect("profile JSON"),
    )
    .expect("profile parse");
    for line in io::stdin().lock().lines() {
        let out = line
            .map_err(|e| e.to_string())
            .and_then(|s| serde_json::from_str::<Value>(&s).map_err(|e| e.to_string()))
            .and_then(|r| run(&r, &library, &profile));
        match out {
            Ok(rows) => {
                for row in rows {
                    println!("{row}");
                }
            }
            Err(e) => println!("{}", json!({"error":e})),
        }
    }
}
