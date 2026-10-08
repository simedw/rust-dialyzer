use rust_dialyzer_poc::{
    constraints::atomic::BindingMap,
    constraints::derived::{DerivedOp, EvalError, NoBifProvider},
    lattice::{LatticeError, Type},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{self, BufRead},
    sync::Arc,
};
fn run(value: &Value) -> Result<Value, String> {
    let field = Type::from_value(&value["field"])?;
    let records = Arc::new(BTreeMap::from([(
        ("entry_record".into(), 1),
        Type::tuple(vec![Type::atom("entry_record"), field]).map_err(|error| error.to_string())?,
    )]));
    let operation = DerivedOp::RecordTestArgument {
        destination: Type::from_value(&value["destination"])?,
        tag: Type::from_value(&value["tag"])?,
        arity: Type::from_value(&value["arity"])?,
        records,
    };
    Ok(
        match operation.evaluate(&BindingMap::new(), &NoBifProvider) {
            Ok(typ) => json!({"ok":typ}),
            Err(EvalError::Native(LatticeError::FunctionClause)) => {
                json!({"error":{"class":"error","reason":"function_clause"}})
            }
            Err(error) => return Err(format!("{error:?}")),
        },
    )
}
fn main() {
    for line in io::stdin().lock().lines() {
        let output = line
            .map_err(|error| error.to_string())
            .and_then(|line| serde_json::from_str(&line).map_err(|error| error.to_string()))
            .and_then(|value| run(&value))
            .unwrap_or_else(|error| json!({"rust_error":error}));
        println!("{output}");
    }
}
