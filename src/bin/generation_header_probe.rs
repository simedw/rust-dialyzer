use rust_dialyzer_poc::term_order;
use serde_json::{Value, json};
use std::io::{self, BufRead};
#[path = "support/generation_headers.rs"]
mod header_boundary;
fn run(v: &Value) -> Result<Value, String> {
    let header = header_boundary::parse(&v["headers"])?;
    let mut terms = vec![];
    let roots = header.append(&mut terms);
    let nil = term_order::CaptureId(terms.len());
    terms.push(term_order::CaptureTerm::Nil);
    let root = term_order::CaptureId(terms.len());
    terms.push(term_order::CaptureTerm::List {
        items: roots.to_vec(),
        tail: nil,
    });
    let actual = term_order::CaptureEnvironment::new(
        std::sync::Arc::new(term_order::TermOrderArena::from_captures(&terms)?),
        root,
    )?;
    let expected = term_order::CaptureEnvironment::parse(&v["native"])?;
    if !actual.compare(&expected)?.is_eq() {
        return Err("native header layout/identity mismatch".into());
    }
    Ok(json!({"matched":true}))
}
fn main() {
    for line in io::stdin().lock().lines() {
        let result = (|| {
            let v: Value = serde_json::from_str(&line.map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            run(&v)
        })();
        println!("{}", result.unwrap_or_else(|e| json!({"error":e})));
    }
}
