use rust_dialyzer_poc::{
    bifs::registry::Mfa,
    bifs::registry::Registry,
    binder::{BinderState, OpaqueMatch, WarningFunction},
    body_calls::{ActiveContract, ApplyTarget, CallControllerError, CallWorld},
    body_flow::{BodyFlow, BodyWarning},
    call_transfer::Signature,
    call_warning::CallWarning,
    contract_runtime::ContractSummary,
    core::{CoreExpr, CoreProgram, ErlangTerms},
    dataflow_map::DataflowMap,
    dataflow_nodes::DataflowNodes,
    emission::WarningState,
    folded_core::FoldedCore,
    function_state::FunctionState,
    function_tree::{GraphInputs, build_function_state},
    labels::{CoreLabel, Labels},
    lattice::Type,
    native_form_environment::RawFormTerm,
    patterns::Patterns,
    program::{FunctionName, ModuleId, NodeRef, Program},
    term_order::compare_map_keys,
    warning_payload::NativeWarning,
    worklist::FunctionKey,
};
use serde_json::{Value, json};
use std::{
    cmp::Ordering,
    collections::BTreeMap,
    io::{self, BufRead},
};
#[derive(Clone, Debug, PartialEq, Eq)]
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
impl From<CallWarning> for Warning {
    fn from(w: CallWarning) -> Self {
        Self(w.native_warning())
    }
}
fn typ(v: &Value) -> Result<Type, String> {
    Type::from_value(v)
}
fn signature(v: &Value) -> Result<Signature, String> {
    Ok(Signature {
        arguments: v["arguments"]
            .as_array()
            .ok_or("sig args")?
            .iter()
            .map(typ)
            .collect::<Result<_, _>>()?,
        result: typ(&v["result"])?,
    })
}
fn mfa(v: &Value) -> Result<Mfa, String> {
    serde_json::from_value(v.clone()).map_err(|e| e.to_string())
}
fn wire(k: FunctionKey) -> Value {
    match k {
        FunctionKey::Top => json!("top"),
        FunctionKey::Label(n) => json!(n.0),
    }
}
fn snap(s: &FunctionState) -> Value {
    json!({"entries":s.entries().iter().map(|(k,e)|{let sig=e.signature();(match k{FunctionKey::Top=>"top".into(),FunctionKey::Label(n)=>n.0.to_string()},json!({"called":e.called(),"args":sig.arguments.iter().map(Type::to_value).collect::<Vec<_>>(),"result":sig.result.to_value()}))}).collect::<serde_json::Map<String,Value>>(),"environments":s.environments().iter().map(|(key,map)|(match key{FunctionKey::Top=>"top".into(),FunctionKey::Label(n)=>n.0.to_string()},json!(map.types().iter().map(|(k,t)|(k.native_integer(),t.to_value())).collect::<serde_json::Map<String,Value>>()))).collect::<serde_json::Map<String,Value>>(),"queue":s.work().ordered().into_iter().map(wire).collect::<Vec<_>>(),"reachable":s.reachable().iter().map(|n|n.0).collect::<Vec<_>>(),"warning_mode":s.warning_mode()})
}
fn run(row: &Value) -> Result<Value, String> {
    let core = CoreProgram::parse(&row["program"].to_string())?;
    let labels = Labels::build(&core, CoreLabel(0))?;
    let nodes = DataflowNodes::build(&core, &labels)?;
    let program = Program::build(&core)?;
    let bifs = Registry::load(include_str!("../../corpus/bifs.json"))?;
    let mut graph_inputs = GraphInputs::empty();
    for l in row["escaping"].as_array().ok_or("escaping")? {
        graph_inputs
            .escaping
            .insert(CoreLabel(l.as_u64().ok_or("label")?));
    }
    for r in row["local_signatures"]
        .as_array()
        .ok_or("local signatures")?
    {
        graph_inputs.raw_signatures.insert(
            CoreLabel(r["label"].as_u64().ok_or("label")?),
            signature(&r["signature"])?,
        );
    }
    let mut functions = build_function_state(&nodes, ModuleId(0), &graph_inputs)?;
    for r in row["seeds"].as_array().ok_or("seeds")? {
        let key = FunctionKey::Label(CoreLabel(r["label"].as_u64().ok_or("label")?));
        let args = r["arguments"]
            .as_array()
            .ok_or("seedargs")?
            .iter()
            .map(typ)
            .collect::<Result<Vec<_>, _>>()?;
        functions
            .forward_arguments(key, &args)
            .map_err(|e| format!("{e:?}"))?;
        functions
            .update_return(key, &args, &typ(&r["result"])?)
            .map_err(|e| format!("{e:?}"))?;
    }
    while functions.pop_work().is_some() {}
    if row["warning_mode"] == true {
        functions.set_warning_mode();
    }
    let mut world = CallWorld::default();
    for r in row["remote_signatures"]
        .as_array()
        .ok_or("remote signatures")?
    {
        world
            .named_signatures
            .insert(mfa(&r["mfa"])?, signature(&r["signature"])?);
    }
    for r in row["contracts"].as_array().ok_or("contracts")? {
        let parts = r["alternatives"]
            .as_array()
            .ok_or("alternatives")?
            .iter()
            .map(|a| {
                Ok((
                    typ(&a["function"])?,
                    a["constraints"]
                        .as_array()
                        .ok_or("constraints")?
                        .iter()
                        .map(|c| Ok((typ(&c[0])?, typ(&c[1])?)))
                        .collect::<Result<Vec<_>, String>>()?,
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let arena = std::sync::Arc::new(ErlangTerms::parse(&r["capture"])?);
        world.active_contracts.insert(
            mfa(&r["mfa"])?,
            ActiveContract {
                summary: std::sync::Arc::new(
                    ContractSummary::from_parts(parts).map_err(|e| e.to_string())?,
                ),
                capture: RawFormTerm::new(arena.clone(), arena.root)?,
            },
        );
    }
    let mut letrec = BTreeMap::new();
    for binding in &program.bindings {
        let Some(function) = binding.declared_function else {
            continue;
        };
        let function = &program.functions[function.0];
        letrec.insert(
            labels
                .label(NodeRef {
                    module: binding.module,
                    node: binding.node.0,
                })
                .ok_or("binding label")?,
            labels
                .label(NodeRef {
                    module: function.module,
                    node: function.node.0,
                })
                .ok_or("function label")?,
        );
    }
    let m = &core.modules[0];
    let CoreExpr::Module { definitions, .. } = &m.nodes[m.core.0].expr else {
        return Err("module".into());
    };
    let CoreExpr::Fun { params, body } = &m.nodes[definitions[0].1.0].expr else {
        return Err("function".into());
    };
    let CoreExpr::Tuple(body_nodes) = &m.nodes[body.0].expr else {
        return Err("fixture wrapper".into());
    };
    let body = *body_nodes.first().ok_or("fixture expression")?;
    let mut map = DataflowMap::default();
    for (param, typ) in params
        .iter()
        .zip(row["arguments"].as_array().ok_or("arguments")?)
    {
        nodes.enter_type(
            NodeRef {
                module: ModuleId(0),
                node: param.0,
            },
            Type::from_value(typ)?,
            &mut map,
        )?;
    }
    let state = BinderState::<Warning> {
        module: m.name.clone(),
        current_fun: WarningFunction::Named(FunctionName {
            module: m.name.clone(),
            name: "entry".into(),
            arity: 5,
        }),
        translated_files: BTreeMap::new().into(),
        letrec: BTreeMap::new().into(),
        records: BTreeMap::new().into(),
        warnings: WarningState {
            warning_mode: row["warning_mode"] == true,
            warnings: vec![],
        },
    };
    let mut patterns = Patterns::new(&nodes);
    let tree = NodeRef {
        module: ModuleId(0),
        node: body.0,
    };
    if let Some(targets) = row["targets"].as_array() {
        world.call_sites.insert(
            tree,
            targets
                .iter()
                .map(|t| {
                    if t == "external" {
                        Ok(ApplyTarget::External)
                    } else {
                        Ok(ApplyTarget::Local(FunctionKey::Label(CoreLabel(
                            t.as_u64().ok_or("target label")?,
                        ))))
                    }
                })
                .collect::<Result<Vec<_>, String>>()?,
        );
    }
    let folded = FoldedCore::build(&core);
    let folded_nodes = DataflowNodes::build(&folded.program, &labels)?;
    let folded_patterns = Patterns::new(&folded_nodes);
    let producer = rust_dialyzer_poc::guard_producer::GuardProducer {
        original: &nodes,
        folded: &folded,
        patterns: &folded_patterns,
    };
    let literals = rust_dialyzer_poc::guard_literals::GuardLiterals::new(&folded)?;
    let guards = rust_dialyzer_poc::guard_control::GuardControl {
        nodes: &nodes,
        producer: &producer,
        literals: &literals,
        registry: &bifs,
    };
    let letrec: rust_dialyzer_poc::letrec_types::LetrecLinks = letrec.into();
    let result = BodyFlow {
        patterns: &mut patterns,
        functions: &mut functions,
        bifs: &bifs,
        letrec: &letrec,
        guards: Some(&guards),
        calls: Some((&producer, &world)),
    }
    .evaluate(tree, &map, &state)
    .map_err(|error| match error {
        rust_dialyzer_poc::body_flow::BodyError::Call(error) => error,
        rust_dialyzer_poc::body_flow::BodyError::Native(error) => {
            CallControllerError::Lattice(error)
        }
        error => CallControllerError::Boundary(format!("body traversal: {error:?}")),
    });
    let value = match result {
        Ok(bound) => {
            let warnings = NativeWarning::list(
                &bound
                    .state
                    .warnings
                    .warnings
                    .iter()
                    .map(|w| w.0.clone())
                    .collect::<Vec<_>>(),
            );
            let expected = ErlangTerms::parse(&row["warnings"])?;
            let matches =
                compare_map_keys(warnings.arena(), warnings.id(), &expected, expected.root)?
                    == Ordering::Equal;
            json!({"status":"ok","type":bound.value.to_value(),"map":{"types":bound.map.types().iter().map(|(k,t)|(k.native_integer(),t.to_value())).collect::<serde_json::Map<String,Value>>(),"substitutions":bound.map.substitutions().iter().map(|(k,v)|Ok((k.native_integer(),json!(v.native_integer().parse::<i64>().map_err(|e|e.to_string())?)))).collect::<Result<serde_json::Map<String,Value>,String>>()?,"modified":bound.map.modified().iter().map(|k|k.native_integer().parse::<i64>()).collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?},"functions":snap(&functions),"warnings_match":matches})
        }
        Err(CallControllerError::Lattice(e)) => {
            json!({"status":"error","error":rust_dialyzer_poc::form_error::FormError::from(e).report()})
        }
        Err(CallControllerError::Transfer(
            rust_dialyzer_poc::call_transfer::CallError::Lattice(e),
        )) => {
            json!({"status":"error","error":rust_dialyzer_poc::form_error::FormError::from(e).report()})
        }
        Err(CallControllerError::Transfer(
            rust_dialyzer_poc::call_transfer::CallError::NativeRange { error, .. },
        )) => {
            json!({"status":"error","error":rust_dialyzer_poc::form_error::FormError::Native(error).report()})
        }
        Err(CallControllerError::Form(e)) => json!({"status":"error","error":e.report()}),
        Err(error) => return Err(format!("{:?}: {error:?}", row["id"])),
    };
    Ok(value)
}
fn main() -> Result<(), String> {
    for line in io::stdin().lock().lines() {
        let row: Value =
            serde_json::from_str(&line.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        println!("{}", run(&row)?);
    }
    Ok(())
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
