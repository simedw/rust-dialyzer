use rust_dialyzer_poc::core_print::CorePrinter;
use rust_dialyzer_poc::emission::WarningState;
use rust_dialyzer_poc::{
    binder::{
        BindError, Binder, BinderRecord, BinderState, Direction, WarningFunction, WarningLocation,
    },
    core::{CoreExpr, CoreProgram},
    dataflow_map::DataflowMap,
    dataflow_nodes::DataflowNodes,
    labels::{CoreLabel, Labels},
    lattice::Type,
    patterns::{PatternRef, Patterns},
    program::{ModuleId, NodeRef},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{self, BufRead},
};
fn run(row: Value) -> Result<Value, String> {
    let input = if let Some(path) = row["core_json"].as_str() {
        std::fs::read_to_string(path).map_err(|e| e.to_string())?
    } else {
        row["core"].to_string()
    };
    let core = CoreProgram::parse(&input)?;
    let labels = Labels::build(&core, CoreLabel(0))?;
    let nodes = DataflowNodes::build(&core, &labels)?;
    let root = NodeRef {
        module: ModuleId(0),
        node: core.modules[0].core.0,
    };
    let CoreExpr::Module(module_payload) = nodes.expression(root)? else {
        return Err("expected module root".into());
    };
    let rust_dialyzer_poc::core::ModuleExpr { definitions, .. } = module_payload.as_ref();
    let CoreExpr::Fun { body, .. } = nodes.expression(NodeRef {
        module: root.module,
        node: definitions[0].1.0,
    })?
    else {
        return Err("expected fun".into());
    };
    let CoreExpr::Values(ids) = nodes.expression(NodeRef {
        module: root.module,
        node: body.0,
    })?
    else {
        return Err("expected values root".into());
    };
    let pats: Vec<_> = ids
        .iter()
        .map(|id| {
            PatternRef::Core(NodeRef {
                module: root.module,
                node: id.0,
            })
        })
        .collect();
    let mut map = DataflowMap::default();
    nodes.mark_fresh(
        &ids.iter()
            .map(|id| NodeRef {
                module: root.module,
                node: id.0,
            })
            .collect::<Vec<_>>(),
        &mut map,
    )?;
    if let Some(initial) = row["initial"].as_array() {
        for pair in initial {
            map.enter_literal_substitution(
                CoreLabel(pair[0].as_u64().ok_or("key expected")?),
                Type::from_value(&pair[1])?,
            )
        }
    }
    let mut state: BinderState = BinderState {
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
            warning_mode: row["warnings"].as_bool().unwrap_or(false),
            warnings: vec![],
        },
    };
    if let Some(records) = row["records"].as_array() {
        for rec in records {
            let tag = rec["tag"].as_str().ok_or("tag expected")?;
            let fs = rec["fields"].as_array().ok_or("fields expected")?;
            let mut fields = vec![Type::atom(tag)];
            fields.extend(
                fs.iter()
                    .map(Type::from_value)
                    .collect::<Result<Vec<_>, _>>()?,
            );
            let presentation = fs
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    Ok(rust_dialyzer_poc::lattice::PresentationRecordField {
                        name: format!("field{i}"),
                        declared_type: Type::from_value(t)?,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            std::sync::Arc::make_mut(&mut state.records).insert(
                (tag.into(), fs.len()),
                BinderRecord {
                    prototype: Type::tuple(fields)?,
                    fields: presentation,
                },
            );
        }
    }
    if let Some(letrecs) = row["letrec"].as_array() {
        for pair in letrecs {
            state.letrec.make_mut().insert(
                CoreLabel(pair[0].as_u64().ok_or("letrec key expected")?).into(),
                Type::from_value(&pair[1])?,
            );
        }
    }
    let types = row["types"]
        .as_array()
        .ok_or("types expected")?
        .iter()
        .map(Type::from_value)
        .collect::<Result<Vec<_>, _>>()?;
    let mut patterns = Patterns::new(&nodes);
    let mut binder = Binder {
        patterns: &mut patterns,
    };
    let dir = if row["reverse"].as_bool().unwrap_or(false) {
        Direction::Reverse
    } else {
        Direction::Forward
    };
    Ok(match binder.bind(&pats, &types, &map, &state, dir) {
        Ok(bound) => {
            json!({"id":row["id"],"status":"ok","map":{"types":bound.map.types().iter().map(|(k,t)|(k.native_integer(),t.to_value())).collect::<serde_json::Map<String,Value>>(),"substitutions":bound.map.substitutions().iter().map(|(k,v)|(k.native_integer(),json!(v.native_integer()))).collect::<serde_json::Map<String,Value>>(),"modified":bound.map.modified().iter().map(|k|k.native_integer()).collect::<Vec<_>>()},"types":bound.types.iter().map(Type::to_value).collect::<Vec<_>>(),"warning_count":bound.state.warnings.warnings.len(),"messages":bound.state.warnings.warnings.iter().map(|w|w.message.clone()).collect::<Vec<_>>(),"origins":bound.state.warnings.warnings.iter().map(|w|json!({"file":w.info.filename,"location":match w.info.location{WarningLocation::Line(n)=>json!(n),WarningLocation::Position{line,column}=>json!([line,column])},"function":match &w.info.current_fun{WarningFunction::Named(f)=>json!([f.module,f.name,f.arity]),WarningFunction::Label(l)=>json!(l.0),WarningFunction::Top=>json!("top"),WarningFunction::Undefined=>json!("undefined")}})).collect::<Vec<_>>(),"input_patterns":CorePrinter{patterns:&patterns}.format_patterns(&pats).map_err(|e|format!("{e:?}"))?})
        }
        Err(BindError::Failure(f)) => {
            json!({"id":row["id"],"status":"failure","kind":format!("{:?}",f.kind).to_lowercase(),"actual":f.actual.to_value(),"patterns":CorePrinter{patterns:&patterns}.format_patterns(&f.patterns).map_err(|e|format!("{e:?}"))?})
        }
        Err(e) => json!({"id":row["id"],"status":"exception","error":format!("{:?}",e)}),
    })
}
fn main() -> Result<(), String> {
    for line in io::stdin().lock().lines() {
        let row =
            serde_json::from_str(&line.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        println!("{}", run(row)?)
    }
    Ok(())
}
