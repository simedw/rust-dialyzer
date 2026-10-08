use rust_dialyzer_poc::{native_ets_set::NativeEtsSet, native_runtime_hash::NativeAtomLayout};
use serde_json::{Value, json};
use std::io::{self, BufRead};
#[path = "support/graph_keys.rs"]
mod graph_keys;
fn main() -> Result<(), String> {
    for line in io::stdin().lock().lines() {
        let row: Value =
            serde_json::from_str(&line.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let layout = NativeAtomLayout::parse(&row["profile"]).map_err(|e| e.to_string())?;
        let mut table = NativeEtsSet::default();
        for value in row["insert"].as_array().ok_or("insert array")? {
            table
                .insert(graph_keys::key(value)?, 1, &layout)
                .map_err(|e| e.to_string())?;
        }
        for value in row["remove"].as_array().ok_or("remove array")? {
            table
                .remove(&graph_keys::key(value)?, &layout)
                .map_err(|e| e.to_string())?;
        }
        let keys = json!(
            table
                .select_keys()
                .iter()
                .map(graph_keys::wire)
                .collect::<Vec<_>>()
        );
        println!(
            "{}",
            json!({"id":row["id"],"matches":keys==row["keys"],"actual":if keys==row["keys"]{Value::Null}else{keys},"expected":if table.select_keys().iter().map(graph_keys::wire).collect::<Vec<_>>()==row["keys"].as_array().ok_or("keys array")?.clone(){Value::Null}else{row["keys"].clone()}})
        );
    }
    Ok(())
}
