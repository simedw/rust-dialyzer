use rust_dialyzer_poc::{
    bifs::registry::Registry,
    binder::{BinderState, OpaqueMatch, WarningFunction},
    body_calls::{ActiveContract, CallWorld},
    body_flow::{BodyFlow, BodyWarning},
    closure_dependencies::{ClosureDependencies, DependencyPolicy},
    contract_runtime::ContractSummary,
    core::{CoreProgram, ErlangTerm, ErlangTerms, TermId},
    dataflow_map::DataflowMap,
    dataflow_nodes::DataflowNodes,
    emission::WarningState,
    function_tree::build_function_state,
    labels::{CoreLabel, Labels},
    lattice::Type,
    native_form_environment::RawFormTerm,
    patterns::Patterns,
    program::{FunctionName, ModuleId, NodeRef, Program},
    source_callgraph::SourceCallGraph,
    term_order::compare_map_keys,
    warning_payload::NativeWarning,
    worklist::FunctionKey,
};
use serde_json::{Value, json};
use std::{
    cmp::Ordering,
    collections::BTreeMap,
    io::{self, BufRead},
    sync::Arc,
};
#[derive(Clone, PartialEq)]
struct Warning(NativeWarning);
impl From<OpaqueMatch> for Warning {
    fn from(w: OpaqueMatch) -> Self {
        Self(w.native_warning())
    }
}
impl From<BodyWarning> for Warning {
    fn from(w: BodyWarning) -> Self {
        Self(w.native_warning())
    }
}
impl From<rust_dialyzer_poc::call_warning::CallWarning> for Warning {
    fn from(w: rust_dialyzer_poc::call_warning::CallWarning) -> Self {
        Self(w.native_warning())
    }
}
impl From<rust_dialyzer_poc::guard_warning::GuardWarning> for Warning {
    fn from(w: rust_dialyzer_poc::guard_warning::GuardWarning) -> Self {
        Self(w.native_warning())
    }
}
impl From<rust_dialyzer_poc::body_flow::CaseWarning> for Warning {
    fn from(w: rust_dialyzer_poc::body_flow::CaseWarning) -> Self {
        Self(w.native_warning())
    }
}
fn run(r: &Value) -> Result<Value, String> {
    let core = CoreProgram::parse(&r["program"].to_string())?;
    let program = Program::build(&core)?;
    let labels = Labels::build(&core, CoreLabel(0))?;
    let nodes = DataflowNodes::build(&core, &labels)?;
    let registry = Registry::standard()?;
    let deps = ClosureDependencies::analyze(
        &core,
        &program,
        &DependencyPolicy::parse(include_str!("../../corpus/dependency_policy.json"))?,
    )?;
    let graph = SourceCallGraph::build(&nodes, &program, &deps, &registry)
        .map_err(|e| e.to_string())?;
    let inputs = graph.function_inputs(ModuleId(0), &program, &labels, BTreeMap::new())?;
    let mut functions = build_function_state(&nodes, ModuleId(0), &inputs)?;
    for op in r["operations"].as_array().ok_or("operations")? {
        let label = CoreLabel(op["label"].as_u64().ok_or("label")?);
        let key = FunctionKey::Label(label);
        let args = op["arguments"]
            .as_array()
            .ok_or("args")?
            .iter()
            .map(Type::from_value)
            .collect::<Result<Vec<_>, _>>()?;
        functions
            .forward_arguments(key, &args)
            .map_err(|e| format!("{e:?}"))?;
        functions
            .update_return(key, &args, &Type::from_value(&op["result"])?)
            .map_err(|e| format!("{e:?}"))?;
        if op["reachable"] == true {
            functions
                .capture(label, DataflowMap::default())
                .map_err(|e| format!("{e:?}"))?;
        }
    }
    if r["warning_mode"] == true {
        functions.set_warning_mode();
    }
    let mut world = CallWorld::default();
    for c in r["contracts"].as_array().ok_or("contracts")? {
        let parts = c["alternatives"]
            .as_array()
            .ok_or("parts")?
            .iter()
            .map(|a| {
                Ok((
                    Type::from_value(&a["function"])?,
                    a["constraints"]
                        .as_array()
                        .ok_or("constraints")?
                        .iter()
                        .map(|pair| Ok((Type::from_value(&pair[0])?, Type::from_value(&pair[1])?)))
                        .collect::<Result<Vec<_>, String>>()?,
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let arena = Arc::new(ErlangTerms::parse(&c["capture"])?);
        world.active_contracts.insert(
            serde_json::from_value(c["mfa"].clone()).map_err(|e| e.to_string())?,
            ActiveContract {
                summary: Arc::new(ContractSummary::from_parts(parts).map_err(|e| e.to_string())?),
                capture: RawFormTerm::new(arena.clone(), arena.root)?,
            },
        );
    }
    let translated_files = core.modules[0]
        .terms
        .iter()
        .enumerate()
        .filter_map(|(index, t)| {
            let ErlangTerm::List { items, .. } = t else {
                return None;
            };
            let chars = items
                .iter()
                .map(|id| {
                    let ErlangTerm::Integer(s) = &core.modules[0].terms[id.0] else {
                        return None;
                    };
                    char::from_u32(s.parse().ok()?)
                })
                .collect::<Option<String>>()?;
            if chars == "fixture.erl" || chars == "other.erl" {
                Some(((ModuleId(0), TermId(index)), format!("/source/{chars}")))
            } else {
                None
            }
        })
        .collect::<BTreeMap<_, _>>();
    let state = BinderState::<Warning> {
        module: core.modules[0].name.as_str().into(),
        current_fun: WarningFunction::Named(FunctionName {
            module: core.modules[0].name.clone(),
            name: "entry".into(),
            arity: 1,
        }),
        translated_files: translated_files.into(),
        letrec: BTreeMap::new().into(),
        records: BTreeMap::new().into(),
        warnings: WarningState {
            warning_mode: r["warning_mode"] == true,
            warnings: vec![],
        },
    };
    let mut patterns = Patterns::new(&nodes);
    let letrec = BTreeMap::new();
    let letrec: rust_dialyzer_poc::letrec_types::LetrecLinks = letrec.into();
    let mut flow = BodyFlow {
        patterns: &mut patterns,
        functions: &mut functions,
        bifs: &registry,
        letrec: &letrec,
        guards: None,
        calls: None,
    };
    let state = if let Some(label) = r["baseline_node"].as_u64() {
        let node = (0..core.modules[0].nodes.len())
            .map(|node| NodeRef {
                module: ModuleId(0),
                node,
            })
            .find(|node| labels.label(*node) == Some(CoreLabel(label)))
            .ok_or("baseline node")?;
        flow.evaluate(node, &DataflowMap::default(), &state)
            .map_err(|e| format!("{e:?}"))?
            .state
    } else {
        state
    };
    let state = flow
        .collect_function_warnings(&graph, &world, &state)
        .map_err(|e| format!("{e:?}"))?;
    let ws = NativeWarning::list(
        &state
            .warnings
            .warnings
            .iter()
            .map(|w| w.0.clone())
            .collect::<Vec<_>>(),
    );
    let expected = ErlangTerms::parse(&r["warnings"])?;
    let matches =
        compare_map_keys(ws.arena(), ws.id(), &expected, expected.root)? == Ordering::Equal;
    Ok(
        json!({"id":r["id"],"native":{"warning_count":state.warnings.warnings.len(),"warnings_match":matches}}),
    )
}
fn main() {
    for line in io::stdin().lock().lines() {
        let value = line
            .map_err(|e| e.to_string())
            .and_then(|s| serde_json::from_str::<Value>(&s).map_err(|e| e.to_string()))
            .and_then(|r| run(&r));
        println!("{}", value.unwrap_or_else(|e| json!({"error":e})));
    }
}
