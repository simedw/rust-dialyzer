use rust_dialyzer_poc::{
    bifs::registry::Registry,
    binder::{BinderState, OpaqueMatch, WarningFunction},
    body_flow::{BodyError, BodyFlow, BodyWarning},
    core::{CoreExpr, CoreProgram, ErlangTerms},
    dataflow_map::DataflowMap,
    dataflow_nodes::DataflowNodes,
    emission::WarningState,
    function_state::FunctionState,
    function_tree::{GraphInputs, build_function_state},
    labels::{CoreLabel, Labels},
    lattice::{LatticeError, Type},
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
    if row["fixture_escaping"] != "all" {
        return Err("explicit fixture escaping profile required".into());
    }
    let mut graph_inputs = GraphInputs::empty();
    for function in &program.functions {
        graph_inputs.escaping.insert(
            labels
                .label(NodeRef {
                    module: function.module,
                    node: function.node.0,
                })
                .ok_or("function label")?,
        );
    }
    let mut functions = build_function_state(&nodes, ModuleId(0), &graph_inputs)?;
    if row["warning_mode"] == true && row["module_loop"] != true {
        functions.set_warning_mode();
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
    let mut state = BinderState::<Warning> {
        module: m.name.as_str().into(),
        current_fun: WarningFunction::Named(FunctionName {
            module: m.name.clone(),
            name: "entry".into(),
            arity: 3,
        }),
        translated_files: BTreeMap::new().into(),
        letrec: BTreeMap::new().into(),
        records: BTreeMap::new().into(),
        warnings: WarningState {
            warning_mode: row["warning_mode"] == true,
            warnings: vec![],
        },
    };
    for record in row["records"].as_array().into_iter().flatten() {
        let tag = record["tag"].as_str().ok_or("record tag")?.to_owned();
        let values = record["fields"]
            .as_array()
            .ok_or("record fields")?
            .iter()
            .map(Type::from_value)
            .collect::<Result<Vec<_>, _>>()?;
        let fields = values
            .iter()
            .enumerate()
            .map(
                |(i, typ)| rust_dialyzer_poc::lattice::PresentationRecordField {
                    name: format!("field{i}"),
                    declared_type: typ.clone(),
                },
            )
            .collect();
        let mut elements = vec![Type::atom(tag.clone())];
        elements.extend(values);
        std::sync::Arc::make_mut(&mut state.records).insert(
            (tag, elements.len() - 1),
            rust_dialyzer_poc::binder::BinderRecord {
                prototype: Type::tuple(elements).map_err(|e| format!("{e:?}"))?,
                fields,
            },
        );
    }
    let folded = rust_dialyzer_poc::folded_core::FoldedCore::build(&core);
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
    let mut patterns = Patterns::new(&nodes);
    let world = rust_dialyzer_poc::body_calls::CallWorld::default();
    let letrec: rust_dialyzer_poc::letrec_types::LetrecLinks = letrec.into();
    let mut flow = BodyFlow {
        calls: Some((&producer, &world)),
        guards: Some(&guards),
        patterns: &mut patterns,
        functions: &mut functions,
        bifs: &bifs,
        letrec: &letrec,
    };
    let result = if row["module_loop"] == true {
        flow.analyze_module(&state, row["warning_mode"] == true)
            .map(|state| rust_dialyzer_poc::body_flow::BodyBound {
                state,
                map: map.clone(),
                value: Type::any(),
            })
    } else {
        flow.evaluate(
            NodeRef {
                module: ModuleId(0),
                node: body.0,
            },
            &map,
            &state,
        )
    };
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
        Err(BodyError::NativeWarningTag(tag)) => {
            json!({"status":"error","kind":"case_clause","payload":tag})
        }
        Err(BodyError::Native(LatticeError::FunctionClause)) => {
            json!({"status":"error","kind":"function_clause"})
        }
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

impl From<rust_dialyzer_poc::call_warning::CallWarning> for Warning {
    fn from(w: rust_dialyzer_poc::call_warning::CallWarning) -> Self {
        Self(w.native_warning())
    }
}
