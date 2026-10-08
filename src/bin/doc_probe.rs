use rust_dialyzer_poc::core::ErlangTerms;
use rust_dialyzer_poc::erlang_syntax::pretty::Doc;
use std::io::{self, BufRead};
fn main() {
    for line in io::stdin().lock().lines() {
        let row: serde_json::Value = serde_json::from_str(&line.unwrap()).unwrap();
        let result = (|| {
            let arena = ErlangTerms::parse(&row["doc"])?;
            Doc::parse(&arena, arena.root)?.format(0)
        })();
        let out = match result {
            Ok(text) => {
                serde_json::json!({"id":row["id"],"matched":text==row["expected"],"actual":text})
            }
            Err(error) => serde_json::json!({"id":row["id"],"error":error}),
        };
        println!("{out}")
    }
}
