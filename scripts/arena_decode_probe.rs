use rust_dialyzer_poc::core::ErlangTerms;
use std::io::{self, BufRead};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    for line in io::stdin().lock().lines() {
        let value: serde_json::Value = serde_json::from_str(&line?)?;
        let result = match ErlangTerms::parse(&value) {
            Ok(arena) => serde_json::json!({"root":arena.root.0,"terms":format!("{:?}",arena.terms)}),
            Err(error) => serde_json::json!({"error":error}),
        };
        println!("{result}");
    }
    Ok(())
}
