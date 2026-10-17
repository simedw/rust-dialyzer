use rust_dialyzer_poc::{
    binder::{BinderState, WarningFunction},
    core::{CoreProgram, ErlangTerm, ErlangTerms, TermId},
    dataflow_nodes::DataflowNodes,
    emission::WarningState,
    form_error::FormError,
    labels::{CoreLabel, Labels},
    lattice::Type,
    native_form_environment::RawFormTerm,
    native_type_operand::NativeTypeOperand,
    patterns::Patterns,
    program::{ModuleId, NodeRef},
};
use rust_dialyzer_poc::{
    folded_core::FoldedCore,
    guard_producer::{ArgumentRef, GuardProducer, ProducerError},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{self, BufRead},
    sync::Arc,
};
fn run(v: &Value) -> Result<Value, String> {
    let core = CoreProgram::parse(&v["core"].to_string())?;
    let labels = Labels::build(&core, CoreLabel(0))?;
    let nodes = DataflowNodes::build(&core, &labels)?;
    let fold = FoldedCore::build(&core);
    let fold_labels = Labels::build(&fold.program, CoreLabel(0))?;
    let fold_nodes = DataflowNodes::build(&fold.program, &fold_labels)?;
    let patterns = Patterns::new(&fold_nodes);
    let producer = GuardProducer {
        original: &nodes,
        folded: &fold,
        patterns: &patterns,
    };
    let mut state: BinderState = BinderState {
        module: "fixture".into(),
        current_fun: WarningFunction::Top,
        translated_files: BTreeMap::new().into(),
        letrec: BTreeMap::new().into(),
        records: BTreeMap::new().into(),
        #[cfg(otp_domain = "29")]
        native_records: BTreeMap::new().into(),
        #[cfg(otp_domain = "29")]
        native_record_imports: BTreeMap::new().into(),
        warnings: WarningState {
            warning_mode: false,
            warnings: vec![],
        },
    };
    for record in v["records"].as_array().unwrap_or(&Vec::new()) {
        let tag = record["tag"].as_str().ok_or("record tag")?;
        let fields = record["fields"]
            .as_array()
            .ok_or("record fields")?
            .iter()
            .enumerate()
            .map(|(i, t)| {
                Ok(rust_dialyzer_poc::lattice::PresentationRecordField {
                    name: format!("field{i}"),
                    declared_type: Type::from_value(t)?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let mut elements = vec![Type::atom(tag)];
        elements.extend(fields.iter().map(|f| f.declared_type.clone()));
        let prototype = Type::tuple(elements)?;
        std::sync::Arc::make_mut(&mut state.records).insert(
            (tag.into(), fields.len()),
            rust_dialyzer_poc::binder::BinderRecord { prototype, fields },
        );
    }
    let mut types = v["types"]
        .as_array()
        .ok_or("types")?
        .iter()
        .map(|t| Type::from_value(t).map(NativeTypeOperand::Canonical))
        .collect::<Result<Vec<_>, _>>()?;
    let raw = Arc::new(ErlangTerms {
        root: TermId(3),
        terms: vec![
            ErlangTerm::Nil,
            ErlangTerm::Atom("c_literal".into()),
            ErlangTerm::Atom("true".into()),
            ErlangTerm::Tuple(vec![TermId(1), TermId(0), TermId(2)]),
        ].into(),
    });
    types.push(NativeTypeOperand::Raw(RawFormTerm::new(
        raw.clone(),
        raw.root,
    )?));
    let mut count = 0;
    let mut errors = Vec::new();
    for (m, module) in v["modules"].as_array().ok_or("modules")?.iter().enumerate() {
        for row in module["outcomes"].as_array().ok_or("outcomes")? {
            let args = if let Some(args) = row["args"].as_array() {
                args.iter()
                    .map(|a| {
                        let node = NodeRef {
                            module: ModuleId(m),
                            node: a["node"].as_u64().ok_or("node")? as usize,
                        };
                        Ok(if a["folded"] == true {
                            ArgumentRef::Folded(node)
                        } else {
                            ArgumentRef::Original(node)
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?
            } else {
                let node = NodeRef {
                    module: ModuleId(m),
                    node: row["node"].as_u64().ok_or("node")? as usize,
                };
                vec![if row["folded"] == true {
                    ArgumentRef::Folded(node)
                } else {
                    ArgumentRef::Original(node)
                }]
            };
            let actual_types = if let Some(indices) = row["types"].as_array() {
                indices
                    .iter()
                    .map(|i| {
                        types
                            .get(i.as_u64().ok_or("type")? as usize)
                            .cloned()
                            .ok_or("type index".into())
                    })
                    .collect::<Result<Vec<_>, String>>()?
            } else {
                vec![
                    types
                        .get(row["type"].as_u64().ok_or("type")? as usize)
                        .ok_or("type index")?
                        .clone(),
                ]
            };
            let result = if row["entry"] == "format_args" {
                producer.format_args(&args, &actual_types, &state)
            } else {
                producer.format_args_1(&args, &actual_types, &state)
            };
            let output = match result {
                Ok(text) => json!({"ok":text}),
                Err(ProducerError::Form(FormError::Native(error))) => {
                    json!({"error":error.report()})
                }
                Err(error) => return Err(format!("unported producer {error:?}")),
            };
            if output != row["result"] {
                errors.push(json!({"module":module["module"],"row":row,"output":output}));
            }
            count += 1;
        }
    }
    Ok(json!({"id":v["id"],"cases":count,"errors":errors}))
}
fn main() {
    for line in io::stdin().lock().lines() {
        let v: Value = serde_json::from_str(&line.expect("line")).expect("request");
        println!("{}", run(&v).expect("producer proof"));
    }
}
