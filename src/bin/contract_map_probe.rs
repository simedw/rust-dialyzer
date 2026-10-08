use rust_dialyzer_poc::core::{CoreProgram, ErlangTerm};
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn run(row: &Value) -> Result<Value, String> {
    let text = std::fs::read_to_string(row["core_json"].as_str().ok_or("Core path")?)
        .map_err(|e| e.to_string())?;
    let core = CoreProgram::parse(&text)?;
    let mut maps = Vec::new();
    for module in &core.modules {
        for form in module.contract_variable_maps.keys() {
            let map = module.contract_variable_map(*form)?;
            let iterator = module
                .map_iterator_pairs(map)?
                .map(|(key, _)| match &module.terms[key.0] {
                    ErlangTerm::Atom(a) => Ok(json!(["atom", a])),
                    ErlangTerm::Integer(i) => Ok(json!(["integer", i])),
                    _ => Err("variable key expected".into()),
                })
                .collect::<Result<Vec<Value>, String>>()?;
            let fold = iterator.iter().rev().cloned().collect::<Vec<_>>();
            maps.push(json!({"iterator":iterator,"fold":fold}));
        }
    }
    Ok(json!({"id":row["id"],"maps":maps}))
}
fn main() {
    for line in io::stdin().lock().lines() {
        let out = line
            .map_err(|e| e.to_string())
            .and_then(|s| serde_json::from_str::<Value>(&s).map_err(|e| e.to_string()))
            .and_then(|r| run(&r));
        println!("{}", out.unwrap_or_else(|e| json!({"error":e})));
    }
}
