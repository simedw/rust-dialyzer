use rust_dialyzer_poc::{
    binder::{BinderState, OpaqueMatch, WarningFunction},
    core::{CoreExpr, CoreProgram, ErlangTerm},
    dataflow_map::DataflowMap,
    dataflow_nodes::DataflowNodes,
    emission::WarningState,
    labels::{CoreLabel, Labels},
    lattice::Type,
    program::{ModuleId, NodeRef},
};
use rust_dialyzer_poc::{
    folded_core::FoldedCore,
    guard_control::{
        GuardControl, GuardEnvironment, GuardError, GuardFailureKind, GuardFailureMessage,
        GuardReason,
    },
    guard_literals::GuardLiterals,
    guard_ops::GuardEval,
    guard_producer::GuardProducer,
    guard_warning::GuardWarning,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{self, BufRead},
};
#[derive(Clone, Debug)]
enum Combined {
    Match(Box<OpaqueMatch>),
    Guard(Box<GuardWarning>),
}
impl Combined {
    fn native_warning(&self) -> rust_dialyzer_poc::warning_payload::NativeWarning {
        match self {
            Self::Match(w) => w.native_warning(),
            Self::Guard(w) => w.native_warning(),
        }
    }
}
impl PartialEq for Combined {
    fn eq(&self, other: &Self) -> bool {
        self.native_warning() == other.native_warning()
    }
}
impl From<OpaqueMatch> for Combined {
    fn from(v: OpaqueMatch) -> Self {
        Self::Match(Box::new(v))
    }
}
impl From<GuardWarning> for Combined {
    fn from(v: GuardWarning) -> Self {
        Self::Guard(Box::new(v))
    }
}
fn run(v: &Value, registry: &rust_dialyzer_poc::bifs::registry::Registry) -> Result<Value, String> {
    let core = CoreProgram::parse(
        &std::fs::read_to_string(v["core_json"].as_str().ok_or("core_json")?)
            .map_err(|e| e.to_string())?,
    )?;
    let labels = Labels::build(&core, CoreLabel(0))?;
    let nodes = DataflowNodes::build(&core, &labels)?;
    let module = &core.modules[0];
    let CoreExpr::Module(module_payload) = &module.nodes[module.core.0].expr else {
        return Err("module".into());
    };
    let rust_dialyzer_poc::core::ModuleExpr { definitions, .. } = module_payload.as_ref();
    let CoreExpr::Fun { body, .. } = module.nodes[definitions[0].1.0].expr else {
        return Err("fun".into());
    };
    let mut map = DataflowMap::default();
    for node in &module.nodes {
        if let CoreExpr::Var(id) = node.expr
            && let ErlangTerm::Atom(name) = &module.terms[id.0]
            && matches!(name.as_str(), "X" | "Y")
        {
            let typ = Type::from_value(if name == "X" { &v["x"] } else { &v["y"] })?;
            map.enter_type(
                nodes.label(NodeRef {
                    module: ModuleId(0),
                    node: node.id.0,
                })?,
                typ,
            )
        }
    }
    let mut state: BinderState<Combined> = BinderState {
        module: "fixture".into(),
        current_fun: WarningFunction::Named(rust_dialyzer_poc::program::FunctionName {
            module: "fixture".into(),
            name: "fixture".into(),
            arity: 0,
        }),
        translated_files: BTreeMap::new().into(),
        letrec: BTreeMap::new().into(),
        records: BTreeMap::new().into(),
        warnings: WarningState {
            warning_mode: v["warnings"].as_bool().unwrap_or(false),
            warnings: vec![],
        },
    };
    for record in v["records"].as_array().into_iter().flatten() {
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
    let eval = match v["eval"].as_str() {
        Some("pos") => GuardEval::Positive,
        Some("neg") => GuardEval::Negative,
        _ => GuardEval::Unknown,
    };
    let folded = FoldedCore::build(&core);
    let folded_nodes = DataflowNodes::build(&folded.program, &labels)?;
    let patterns = rust_dialyzer_poc::patterns::Patterns::new(&folded_nodes);
    let producer = GuardProducer {
        original: &nodes,
        folded: &folded,
        patterns: &patterns,
    };
    let literals = GuardLiterals::new(&folded)?;
    let mut returned_warnings = Vec::new();
    let output = match (GuardControl {
        nodes: &nodes,
        producer: &producer,
        literals: &literals,
        registry,
    })
    .evaluate(
        NodeRef {
            module: ModuleId(0),
            node: body.0,
        },
        &map,
        &GuardEnvironment::new(),
        eval,
        &state,
    ) {
        Ok(bound) => {
            returned_warnings = bound.state.warnings.warnings;
            json!({"ok":{"value":bound.value.to_value(),"map":{"types":bound.map.types().iter().map(|(k,t)|(k.native_integer(),t.to_value())).collect::<serde_json::Map<_,_>>(),"substitutions":bound.map.substitutions().iter().map(|(k,v)|(k.native_integer(),json!(v.native_integer()))).collect::<serde_json::Map<_,_>>(),"modified":bound.map.modified().iter().map(|k|k.native_integer()).collect::<Vec<_>>()}}})
        }
        Err(GuardError::UnderapproxCaseClauseAtom(atom)) => {
            json!({"error":{"kind":"error","class":"case_clause","payload":{"kind":"atom","value":atom}}})
        }
        Err(GuardError::Native(error)) => json!({"error":error.report()}),
        Err(GuardError::Failure {
            fatal,
            reason: None,
        }) => json!({"failure":{"fatal":fatal}}),
        Err(GuardError::Failure {
            fatal,
            reason: Some(GuardReason::GuardFailure { kind, message, .. }),
        }) => {
            let kind = match kind {
                GuardFailureKind::GuardFail => "guard_fail",
                GuardFailureKind::NegGuardFail => "neg_guard_fail",
            };
            let message = match message {
                GuardFailureMessage::Arguments { function, args } => {
                    json!({"function":function,"args":args})
                }
                GuardFailureMessage::Infix {
                    left,
                    operator,
                    right,
                } => json!({"left":left,"operator":operator,"right":right}),
            };
            json!({"failure":{"fatal":fatal,"reason":{"kind":kind,"message":message}}})
        }
        Err(GuardError::Failure {
            fatal,
            reason: Some(GuardReason::RecordMatching { actual, tag, .. }),
        }) => {
            json!({"failure":{"fatal":fatal,"reason":{"kind":"record_matching","message":{"actual":actual,"tag":tag}}}})
        }
        Err(GuardError::Producer(rust_dialyzer_poc::guard_producer::ProducerError::Form(
            rust_dialyzer_poc::form_error::FormError::Native(error),
        ))) => json!({"error":error.report()}),
        Err(error) => return Err(format!("uncovered guard control {error:?}")),
    };
    let mut output = output;
    if v["warnings"].as_bool().unwrap_or(false) && output.get("ok").is_some() {
        output["ok"]["warnings"]=json!(returned_warnings.iter().map(|w|match w {
            Combined::Guard(w)=>match w.as_ref() {
                GuardWarning::OpaqueGuard(w)=>{
                    let message=match &w.message {
                        rust_dialyzer_poc::guard_warning::OpaqueGuardMessage::Arguments {function,args}=>json!({"function":function,"args":args}),
                        rust_dialyzer_poc::guard_warning::OpaqueGuardMessage::Infix {left,operator,right,positions}=>json!({"left":left,"operator":operator,"right":right,"positions":positions.positions()}),
                    };
                    let location=match w.info.location {rust_dialyzer_poc::binder::WarningLocation::Line(n)=>json!(n),rust_dialyzer_poc::binder::WarningLocation::Position{line,column}=>json!([line,column])};
                    json!({"info":{"file":w.info.filename,"location":location,"function":["fixture","fixture",0]},"message":message})
                }
            },
            Combined::Match(w)=>{
                let location=match w.info.location {rust_dialyzer_poc::binder::WarningLocation::Line(n)=>json!(n),rust_dialyzer_poc::binder::WarningLocation::Position{line,column}=>json!([line,column])};
                json!({"info":{"file":w.info.filename,"location":location,"function":["fixture","fixture",0]},"message":{"match":w.message}})
            },
        }).collect::<Vec<_>>());
    }
    let mut literal_exact = true;
    if let Some(inputs) = v["literal_operands"].as_array() {
        let CoreExpr::Call { args, .. } = &module.nodes[body.0].expr else {
            return Err("literal AST proof not call".into());
        };
        for input in inputs {
            let index = input["arg"].as_u64().ok_or("literal argument index")? as usize;
            let raw = literals
                .raw_operand(NodeRef {
                    module: ModuleId(0),
                    node: args[index].0,
                })?
                .to_raw();
            let native = rust_dialyzer_poc::core::ErlangTerms::parse(&input["term"])?;
            literal_exact &= rust_dialyzer_poc::term_order::compare_map_keys(
                raw.arena(),
                raw.id(),
                &native,
                native.root,
            )? == std::cmp::Ordering::Equal;
        }
    }
    let mut warning_exact = true;
    if let Some(raw) = v["ok"].get("warning_terms") {
        let native = rust_dialyzer_poc::core::ErlangTerms::parse(raw)?;
        let actual = rust_dialyzer_poc::warning_payload::NativeWarning::list(
            &returned_warnings
                .iter()
                .map(Combined::native_warning)
                .collect::<Vec<_>>(),
        );
        warning_exact = rust_dialyzer_poc::term_order::compare_map_keys(
            actual.arena(),
            actual.id(),
            &native,
            native.root,
        )? == std::cmp::Ordering::Equal;
    }
    let mut expected = json!({});
    for key in ["ok", "error", "failure"] {
        if let Some(val) = v.get(key) {
            expected[key] = val.clone()
        }
    }
    if let Some(ok) = expected.get_mut("ok").and_then(Value::as_object_mut) {
        ok.remove("warning_terms");
    }
    if let Some(m) = expected["ok"]["map"]["modified"].as_array() {
        expected["ok"]["map"]["modified"] = json!(
            m.iter()
                .map(|i| i.as_i64().expect("native label integer").to_string())
                .collect::<Vec<_>>()
        )
    }
    if let Some(subs) = expected["ok"]["map"]["substitutions"].as_object() {
        expected["ok"]["map"]["substitutions"] = json!(
            subs.iter()
                .map(|(k, i)| (
                    k.clone(),
                    json!(i.as_i64().expect("native label integer").to_string())
                ))
                .collect::<serde_json::Map<_, _>>()
        )
    }
    Ok(
        json!({"id":v["id"],"matches":output==expected && warning_exact && literal_exact,"warning_exact":warning_exact,"literal_exact":literal_exact,"output":output,"expected":if output==expected{Value::Null}else{expected}}),
    )
}
fn main() {
    let registry = rust_dialyzer_poc::bifs::registry::Registry::load(
        &std::fs::read_to_string(
            std::env::args()
                .nth(1)
                .unwrap_or_else(|| "corpus/bifs.json".into()),
        )
        .expect("catalogue file"),
    )
    .expect("typed registry");
    for line in io::stdin().lock().lines() {
        let v: Value = serde_json::from_str(&line.expect("line")).expect("request");
        println!("{}", run(&v, &registry).expect("control proof"));
    }
}
