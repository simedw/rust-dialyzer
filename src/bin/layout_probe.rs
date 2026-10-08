use rust_dialyzer_poc::erlang_syntax::syntax::*;
use std::io::{self, BufRead};
fn main() {
    for line in io::stdin().lock().lines() {
        let row: serde_json::Value = serde_json::from_str(&line.unwrap()).unwrap();
        let source = row["source"].as_str().unwrap();
        let result = match row["helper"].as_str().unwrap() {
            "t" => format_type_or_literal(source),
            "sig" => format_signature(source),
            "field_diffs" => format_field_diffs(source),
            _ => panic!("unknown helper"),
        };
        println!(
            "{}",
            match result {
                Ok(actual) =>
                    serde_json::json!({"id":row["id"],"matched":actual==row["expected"],"actual":actual}),
                Err(SyntaxError::NativeBadMatchNil) =>
                    serde_json::json!({"id":row["id"],"matched":row["expected_error"]["class"]=="error"&&row["expected_error"]["reason"]=="{:badmatch, []}","error":"native badmatch []"}),
                Err(e) => serde_json::json!({"id":row["id"],"error":format!("{e:?}")}),
            }
        )
    }
}
