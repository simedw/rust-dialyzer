use rust_dialyzer_poc::{core::ErlangTerms, literal_types::LiteralTypes};
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn main() -> Result<(), String> {
    for line in io::stdin().lock().lines() {
        let row: Value =
            serde_json::from_str(&line.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let arena = ErlangTerms::parse(&row["arena"])?;
        let types = LiteralTypes::build(&arena)?;
        println!(
            "{}",
            json!({"id":row["id"],"type":types.get(arena.root).ok_or("missing literal root")?.to_value()})
        );
    }
    Ok(())
}
