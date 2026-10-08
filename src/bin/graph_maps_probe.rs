use rust_dialyzer_poc::{native_map_order::ordered_keys, native_runtime_hash::NativeAtomLayout};
use serde_json::{Value, json};
use std::io::{self, BufRead};
#[path = "support/graph_keys.rs"]
mod graph_keys;
fn main() -> Result<(), String> {
    for line in io::stdin().lock().lines() {
        let row: Value =
            serde_json::from_str(&line.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let layout = NativeAtomLayout::parse(&row["profile"]).map_err(|e| e.to_string())?;
        let keys = row["insert"]
            .as_array()
            .ok_or("insert array")?
            .iter()
            .map(graph_keys::key)
            .collect::<Result<Vec<_>, _>>()?;
        let actual = json!(
            ordered_keys(&layout, keys)
                .map_err(|e| e.to_string())?
                .iter()
                .map(graph_keys::wire)
                .collect::<Vec<_>>()
        );
        println!(
            "{}",
            json!({"id":row["id"],"matches":actual==row["keys"],"actual":if actual==row["keys"]{Value::Null}else{actual},"expected":row["keys"]})
        );
    }
    Ok(())
}
