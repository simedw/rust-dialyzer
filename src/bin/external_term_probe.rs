use rust_dialyzer_poc::{
    core::ErlangTerms,
    external_term::{self, Limits},
    type_exports::arena_to_value,
};
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn run(row: &Value) -> Result<Value, String> {
    let bytes: Vec<u8> = serde_json::from_value(row["bytes"].clone()).map_err(|e| e.to_string())?;
    let arena = match external_term::decode(&bytes, Limits::default()) {
        Ok(arena) => arena,
        Err(error) => {
            return Ok(json!({"id":row["id"],"status":"error","detail":error.to_string()}));
        }
    };
    let encoded =
        external_term::encode(&arena, Limits::default(), true).map_err(|e| e.to_string())?;
    let decoded = external_term::decode(&encoded, Limits::default()).map_err(|e| e.to_string())?;
    let _: ErlangTerms = decoded;
    Ok(
        json!({"id":row["id"],"status":"ok","arena":arena_to_value(&arena.terms,arena.root),"encoded":encoded}),
    )
}
fn main() -> Result<(), String> {
    for line in io::stdin().lock().lines() {
        let row: Value =
            serde_json::from_str(&line.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        println!("{}", run(&row)?);
    }
    Ok(())
}
