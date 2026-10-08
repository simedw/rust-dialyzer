use rust_dialyzer_poc::emission::WarningState;
use rust_dialyzer_poc::{
    binder::{BindError, Binder, BinderState, Direction, WarningFunction},
    core::{CoreExpr, CoreProgram},
    core_print::CorePrinter,
    dataflow_map::DataflowMap,
    dataflow_nodes::DataflowNodes,
    labels::{CoreLabel, Labels},
    lattice::Type,
    patterns::{PatternRef, Patterns},
    program::{FunctionName, ModuleId, NodeRef},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{self, BufRead},
};
fn run(row: Value) -> Result<Value, String> {
    let core = CoreProgram::parse(
        &std::fs::read_to_string(row["core_json"].as_str().ok_or("Core path expected")?)
            .map_err(|e| e.to_string())?,
    )?;
    let labels = Labels::build(&core, CoreLabel(0))?;
    let nodes = DataflowNodes::build(&core, &labels)?;
    let mut patterns = Patterns::new(&nodes);
    let mut modules = Vec::new();
    for (index, module) in core.modules.iter().enumerate() {
        let mut initial = DataflowMap::default();
        for node in &module.nodes {
            if matches!(node.expr, CoreExpr::Var(_)) {
                initial.enter_type(
                    nodes.label(NodeRef {
                        module: ModuleId(index),
                        node: node.id.0,
                    })?,
                    Type::any(),
                )
            }
        }
        let state: BinderState = BinderState {
            module: module.name.clone(),
            current_fun: WarningFunction::Named(FunctionName {
                module: module.name.clone(),
                name: "fixture".into(),
                arity: 0,
            }),
            translated_files: BTreeMap::new().into(),
            letrec: BTreeMap::new().into(),
            records: BTreeMap::new().into(),
            warnings: WarningState {
                warning_mode: false,
                warnings: vec![],
            },
        };
        let mut results = Vec::new();
        for node in &module.nodes {
            let roots = match &node.expr {
                CoreExpr::Clause { patterns, .. } => patterns,
                CoreExpr::Fun { params, .. } => params,
                _ => continue,
            };
            if roots.is_empty() {
                continue;
            }
            let refs: Vec<_> = roots
                .iter()
                .map(|n| NodeRef {
                    module: ModuleId(index),
                    node: n.0,
                })
                .collect();
            let pats: Vec<_> = refs.iter().copied().map(PatternRef::Core).collect();
            let mut map = initial.clone();
            nodes.mark_fresh(&refs, &mut map)?;
            for reverse in [false, true] {
                let mut binder = Binder {
                    patterns: &mut patterns,
                };
                let result = binder.bind(
                    &pats,
                    &vec![Type::any(); pats.len()],
                    &map,
                    &state,
                    if reverse {
                        Direction::Reverse
                    } else {
                        Direction::Forward
                    },
                );
                let mut result = match result {
                    Ok(b) => {
                        json!({"status":"ok","types":b.types.iter().map(Type::to_value).collect::<Vec<_>>(),"changed":b.map.types().iter().filter(|(_,t)|!t.is_any()).map(|(k,t)|(k.native_integer(),t.to_value())).collect::<serde_json::Map<String,Value>>(),"substitutions":b.map.substitutions().iter().map(|(k,v)|(k.native_integer(),json!(v.native_integer()))).collect::<serde_json::Map<String,Value>>(),"warning_count":b.state.warnings.warnings.len()})
                    }
                    Err(BindError::Failure(f)) => {
                        json!({"status":"failure","kind":format!("{:?}",f.kind).to_lowercase(),"actual":f.actual.to_value(),"patterns":CorePrinter{patterns:&patterns}.format_patterns(&f.patterns).map_err(|e|format!("{e:?}"))?})
                    }
                    Err(e) => json!({"status":"exception","error":format!("{e:?}")}),
                };
                result["node"] = json!(node.id.0);
                result["reverse"] = json!(reverse);
                result["pattern_count"] = json!(roots.len());
                results.push(result)
            }
        }
        modules.push(json!({"module":module.name,"results":results}))
    }
    Ok(json!({"id":row["id"],"modules":modules}))
}
fn main() -> Result<(), String> {
    for line in io::stdin().lock().lines() {
        let row =
            serde_json::from_str(&line.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        println!("{}", run(row)?)
    }
    Ok(())
}
