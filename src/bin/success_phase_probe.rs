use rust_dialyzer_poc::{
    call_transfer::Signature,
    labels::CoreLabel,
    lattice::Type,
    success_phase::{self, FunctionTypes},
};
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn signature(v: &Value) -> Result<Signature, String> {
    Ok(Signature {
        arguments: v["arguments"]
            .as_array()
            .ok_or("arguments required")?
            .iter()
            .map(Type::from_value)
            .collect::<Result<_, _>>()?,
        result: Type::from_value(&v["result"])?,
    })
}
fn table(v: &Value) -> Result<FunctionTypes, String> {
    let mut out = FunctionTypes::new();
    for row in v.as_array().ok_or("function table required")? {
        let key = CoreLabel(row["label"].as_u64().ok_or("label required")?);
        if out.insert(key, signature(row)?).is_some() {
            return Err("duplicate label".into());
        }
    }
    Ok(out)
}
fn run(v: &Value) -> Result<Value, String> {
    let old = table(&v["previous"])?;
    let new = table(&v["next"])?;
    let strict = success_phase::reached_fixpoint_strict(&old, &new);
    let updated = success_phase::updated_types(&old, &new);
    let persisted=new.iter().map(|(key,s)|success_phase::persisted_signature(s).map(|s|json!({"label":key.0,"arguments":s.arguments.iter().map(Type::to_value).collect::<Vec<_>>(),"result":s.result.to_value()}))).collect::<Result<Vec<_>,_>>();
    let outcome = |result: Result<Value, rust_dialyzer_poc::lattice::LatticeError>| match result {
        Ok(value) => json!({"status":"ok","value":value}),
        Err(error) => json!({"status":"error","reason":error.to_string()}),
    };
    Ok(
        json!({"id":v["id"],"strict":outcome(strict.map(|b|json!(b))),"updated":outcome(updated.map(|rows|json!(rows.iter().map(|(key,s)|json!({"label":key.0,"arguments":s.arguments.iter().map(Type::to_value).collect::<Vec<_>>(),"result":s.result.to_value()})).collect::<Vec<_>>()))),"persisted":outcome(persisted.map(|rows|json!(rows)))}),
    )
}
fn main() {
    for line in io::stdin().lock().lines() {
        let output = line
            .map_err(|e| e.to_string())
            .and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))
            .and_then(|v| run(&v))
            .unwrap_or_else(|e| json!({"boundary_error":e}));
        println!("{output}");
    }
}
