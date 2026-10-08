use rust_dialyzer_poc::{
    core::ErlangTerms,
    term_order::{compare_map_keys, compare_terms},
};
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn main() {
    for line in io::stdin().lock().lines() {
        let result = (|| -> Result<Value, String> {
            let v: Value = serde_json::from_str(&line.map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            let a = ErlangTerms::parse(&v["a"])?;
            let b = ErlangTerms::parse(&v["b"])?;
            let c = if v["exact"] == true {
                compare_map_keys(&a, a.root, &b, b.root)
            } else {
                compare_terms(&a, a.root, &b, b.root)
            }?;
            Ok(json!(match c {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            }))
        })();
        println!(
            "{}",
            match result {
                Ok(v) => json!({"ok":v}),
                Err(e) => json!({"error":e}),
            }
        );
    }
}
