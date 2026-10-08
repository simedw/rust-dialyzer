use rust_dialyzer_poc::{
    binder::{BinderState, OpaqueMatch, WarningFunction},
    core::{CoreExpr, CoreProgram, ErlangTerms},
    dataflow_map::DataflowMap,
    dataflow_nodes::DataflowNodes,
    labels::{CoreLabel, Labels},
    lattice::Type,
    program::{ModuleId, NodeRef},
};
use rust_dialyzer_poc::{
    folded_core::FoldedCore,
    guard_control::{GuardControl, GuardEnvironment, GuardError},
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
use rust_dialyzer_poc::{
    compiled_definitions::PreparedDefinitions,
    program::{FunctionIdentity, Program},
    warning_payload::NativeWarning,
};
fn run(
    request: &Value,
    registry: &rust_dialyzer_poc::bifs::registry::Registry,
) -> Result<Value, String> {
    let core = CoreProgram::parse(
        &std::fs::read_to_string(request["core"].as_str().ok_or("core")?)
            .map_err(|e| e.to_string())?,
    )?;
    let program = Program::build(&core)?;
    let library = rust_dialyzer_poc::dependencies::PriorPlt::parse(
        &std::fs::read_to_string(request["environment"].as_str().ok_or("environment")?)
            .map_err(|e| e.to_string())?,
        &core,
        &rust_dialyzer_poc::dependencies::ImportPlan::cold_targets(&core),
    )
    .and_then(|prior| prior.into_cold_store())?;
    let definitions = PreparedDefinitions::prepare(&core, &program, &library)
        .map_err(|e| format!("preparation: {e}"))?;
    let labels = Labels::build(&core, CoreLabel(0))?;
    let nodes = DataflowNodes::build(&core, &labels)?;
    let folded = FoldedCore::build(&core);
    let folded_nodes = DataflowNodes::build(&folded.program, &labels)?;
    let patterns = rust_dialyzer_poc::patterns::Patterns::new(&folded_nodes);
    let producer = GuardProducer {
        original: &nodes,
        folded: &folded,
        patterns: &patterns,
    };
    let literals = GuardLiterals::new(&folded)?;
    let control = GuardControl {
        nodes: &nodes,
        producer: &producer,
        literals: &literals,
        registry,
    };
    let mut roots = Vec::new();
    let mut initial_maps = Vec::new();
    let mut base_states = Vec::new();
    for (mi, module) in core.modules.iter().enumerate() {
        let mid = ModuleId(mi);
        let mut map = DataflowMap::default();
        for node in &module.nodes {
            if matches!(node.expr, CoreExpr::Var(_))
                && let Some(label) = labels.label(NodeRef {
                    module: mid,
                    node: node.id.0,
                })
            {
                map.enter_type(label, Type::any());
            }
        }
        initial_maps.push(map);
        let mut state = BinderState::<Combined>::for_module(&core, &definitions, mid, false)
            .map_err(|e| e.to_string())?;
        if let Some(files) = request.get("files") {
            let actual = state
                .translated_files
                .values()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            if json!(actual) != files["modules"][mi]["files"] {
                return Err(format!(
                    "Source codeserver filename translation differs module{mi}"
                ));
            }
        }
        // The guard fixture uses an empty Native codeserver.
        std::sync::Arc::make_mut(&mut state.translated_files).clear();
        base_states.push(state);
        let functions = program
            .functions
            .iter()
            .filter(|f| f.module == mid)
            .map(|f| (f.node.0, f))
            .collect::<BTreeMap<_, _>>();
        let mut todo = vec![(module.core, WarningFunction::Undefined)];
        let mut local = Vec::new();
        while let Some((id, current)) = todo.pop() {
            let node = NodeRef {
                module: mid,
                node: id.0,
            };
            let current = if let Some(fun) = functions.get(&id.0) {
                match &fun.identity {
                    FunctionIdentity::Named(name) => WarningFunction::Named(name.clone()),
                    _ => WarningFunction::Label(labels.label(node).ok_or("function label")?),
                }
            } else {
                current
            };
            if let CoreExpr::Clause { guard, .. } = module.nodes[id.0].expr {
                local.push((
                    NodeRef {
                        module: mid,
                        node: guard.0,
                    },
                    current.clone(),
                ));
            }
            todo.extend(
                module.nodes[id.0]
                    .expr
                    .native_children()
                    .into_iter()
                    .rev()
                    .map(|id| (id, current.clone())),
            );
        }
        roots.push(local);
    }
    for (mid, map) in initial_maps.iter().enumerate() {
        let expected = request["initial_keys"][mid]
            .as_array()
            .ok_or("initial keys")?
            .iter()
            .map(|key| {
                key.as_u64()
                    .ok_or("initial key integer")
                    .map(|key| key.to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let actual = map
            .types()
            .keys()
            .map(|key| key.native_integer())
            .collect::<Vec<_>>();
        if actual != expected {
            return Err(format!("initial source keys differ module{mid}"));
        }
    }
    let mut matches = 0;
    let mut unsupported = 0;
    let mut mismatches = Vec::new();
    for row in request["results"].as_array().ok_or("results")? {
        let mid = row["module"].as_u64().ok_or("module index")? as usize;
        let index = row["guard_index"].as_u64().ok_or("guard index")? as usize;
        let (node, current) = roots
            .get(mid)
            .and_then(|r| r.get(index))
            .ok_or("guard index outside source traversal")?;
        if labels.label(*node).map(|label| label.0) != row["label"].as_u64() {
            return Err(format!(
                "source root labels differ module{mid} guard{index}"
            ));
        }
        let mut state = base_states[mid].clone();
        state.current_fun = current.clone();
        state.warnings.warning_mode = row["warnings"].as_bool().ok_or("warning mode")?;
        let current_wire = match current {
            WarningFunction::Named(f) => json!([f.module, f.name, f.arity]),
            WarningFunction::Label(l) => json!(l.0),
            WarningFunction::Undefined => json!("undefined"),
            WarningFunction::Top => json!("top"),
        };
        if current_wire != row["current_fun"] {
            return Err(format!(
                "current function differs module{mid} guard{index}: {current_wire} / {}",
                row["current_fun"]
            ));
        }
        let eval = match row["eval"].as_str() {
            Some("pos") => GuardEval::Positive,
            Some("neg") => GuardEval::Negative,
            _ => GuardEval::Unknown,
        };
        let mut warnings = Vec::new();
        let mut raw_reason = None;
        let output = match control.evaluate(
            *node,
            &initial_maps[mid],
            &GuardEnvironment::new(),
            eval,
            &state,
        ) {
            Ok(bound) => {
                warnings = bound.state.warnings.warnings;
                json!({"ok":{"value":bound.value.to_value(),"map":{"types_changed":bound.map.types().iter().filter(|(k,t)|!t.is_any() || !initial_maps[mid].types().contains_key(k)).map(|(k,t)|(k.native_integer(),t.to_value())).collect::<serde_json::Map<_,_>>(),"types_removed":initial_maps[mid].types().keys().filter(|k|!bound.map.types().contains_key(k)).map(|k|k.native_integer()).collect::<Vec<_>>(),"substitutions":bound.map.substitutions().iter().map(|(k,v)|(k.native_integer(),json!(v.native_integer()))).collect::<serde_json::Map<_,_>>(),"modified":bound.map.modified().iter().map(|k|k.native_integer()).collect::<Vec<_>>()}}})
            }
            Err(GuardError::Failure { fatal, reason: r }) => {
                if let Some(r) = r {
                    raw_reason = Some(r.native_message());
                }
                json!({"failure":{"fatal":fatal}})
            }
            Err(GuardError::Native(error)) => json!({"error":error.report()}),
            Err(GuardError::Producer(rust_dialyzer_poc::guard_producer::ProducerError::Form(
                rust_dialyzer_poc::form_error::FormError::Native(error),
            ))) => json!({"error":error.report()}),
            Err(error) => {
                unsupported += 1;
                mismatches.push(json!({"module":mid,"guard":index,"status":"unsupported","error":format!("{error:?}")}));
                continue;
            }
        };
        let mut expected = serde_json::Map::new();
        for key in ["ok", "failure", "error", "raw_exception"] {
            if let Some(v) = row.get(key) {
                expected.insert(key.into(), v.clone());
            }
        }
        let expected_warnings = expected
            .get_mut("ok")
            .and_then(Value::as_object_mut)
            .and_then(|v| v.remove("warning_terms"));
        if let Some(map) = expected
            .get_mut("ok")
            .and_then(|v| v.get_mut("map"))
            .and_then(Value::as_object_mut)
        {
            if let Some(modified) = map.get_mut("modified") {
                *modified = json!(
                    modified
                        .as_array()
                        .ok_or("modified array")?
                        .iter()
                        .map(|v| v.as_i64().ok_or("modified integer").map(|v| v.to_string()))
                        .collect::<Result<Vec<_>, _>>()?
                );
            }
            if let Some(removed) = map.get_mut("types_removed") {
                *removed = json!(
                    removed
                        .as_array()
                        .ok_or("removed array")?
                        .iter()
                        .map(|key| key
                            .as_i64()
                            .ok_or("removed integer")
                            .map(|key| key.to_string()))
                        .collect::<Result<Vec<_>, _>>()?
                );
            }
            if let Some(subs) = map.get_mut("substitutions") {
                *subs = json!(
                    subs.as_object()
                        .ok_or("substitutions object")?
                        .iter()
                        .map(|(k, v)| Ok((
                            k.clone(),
                            json!(v.as_i64().ok_or("substitution integer")?.to_string())
                        )))
                        .collect::<Result<serde_json::Map<_, _>, String>>()?
                );
            }
        }
        let expected_reason = expected
            .get_mut("failure")
            .and_then(Value::as_object_mut)
            .and_then(|v| v.remove("raw_reason"));
        let warning_match = if let Some(raw) = expected_warnings {
            let expected = ErlangTerms::parse(&raw)?;
            let actual = NativeWarning::list(
                &warnings
                    .iter()
                    .map(Combined::native_warning)
                    .collect::<Vec<_>>(),
            );
            rust_dialyzer_poc::term_order::compare_map_keys(
                actual.arena(),
                actual.id(),
                &expected,
                expected.root,
            )? == std::cmp::Ordering::Equal
        } else {
            warnings.is_empty()
        };
        let reason_match = match (raw_reason, expected_reason) {
            (None, None) => true,
            (Some(actual), Some(expected)) => {
                let expected = ErlangTerms::parse(&expected)?;
                rust_dialyzer_poc::term_order::compare_map_keys(
                    actual.arena(),
                    actual.id(),
                    &expected,
                    expected.root,
                )? == std::cmp::Ordering::Equal
            }
            _ => false,
        };
        if output == Value::Object(expected.clone()) && warning_match && reason_match {
            matches += 1;
        } else {
            mismatches.push(json!({"module":mid,"guard":index,"eval":row["eval"],"warnings":row["warnings"],"output":output,"expected":expected,"warning_match":warning_match,"reason_match":reason_match}));
        }
    }
    Ok(
        json!({"id":request["id"],"cases":request["results"].as_array().unwrap().len(),"matches":matches,"unsupported":unsupported,"roots":roots.iter().map(Vec::len).sum::<usize>(),"mismatches":mismatches}),
    )
}
fn main() -> Result<(), String> {
    let registry = rust_dialyzer_poc::bifs::registry::Registry::load(
        &std::fs::read_to_string(
            std::env::args()
                .nth(1)
                .unwrap_or_else(|| "data/catalogue.json".into()),
        )
        .map_err(|e| e.to_string())?,
    )?;
    for line in io::stdin().lock().lines() {
        let request =
            serde_json::from_str(&line.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        println!("{}", run(&request, &registry)?);
    }
    Ok(())
}
