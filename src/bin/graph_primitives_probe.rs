use rust_dialyzer_poc::native_runtime_hash::NativeAtomLayout;
use serde_json::{Value, json};
use std::io::{self, BufRead};
#[path = "support/graph_keys.rs"]
mod graph_keys;
use graph_keys::key;
fn main() -> Result<(), String> {
    for line in io::stdin().lock().lines() {
        let row: Value =
            serde_json::from_str(&line.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let layout = NativeAtomLayout::parse(&row["profile"]).map_err(|e| e.to_string())?;
        let key = key(&row["key"])?;
        let actual = layout.internal_hash(&key).map_err(|e| e.to_string())?;
        println!(
            "{}",
            json!({"id":row["id"],"key_roundtrip":graph_keys::wire(&key)==row["key"],"matches":actual.to_string()==row["hash"].as_str().ok_or("hash string")?,"actual":actual.to_string(),"expected":row["hash"]})
        );
    }
    Ok(())
}
