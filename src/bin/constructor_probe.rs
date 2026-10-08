use num_bigint::BigInt;
use rust_dialyzer_poc::lattice::{Endpoint, LatticeError, Type};
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn endpoint(s: &str) -> Endpoint {
    match s {
        "neg_inf" => Endpoint::NegativeInfinity,
        "pos_inf" => Endpoint::PositiveInfinity,
        _ => Endpoint::Finite(s.parse().unwrap()),
    }
}
fn main() {
    for line in io::stdin().lock().lines() {
        let c: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let a = c["a"].as_str().unwrap();
        let b = c["b"].as_str().unwrap();
        let r = if c["op"] == "range" {
            match Type::range(endpoint(a), endpoint(b)) {
                Ok(t) => json!({"value":t.to_native_value().to_value()}),
                Err(LatticeError::FunctionClause) => json!({"error":"function_clause"}),
                Err(e) => panic!("unexpected error {e}"),
            }
        } else {
            let u: BigInt = a.parse().unwrap();
            let base: BigInt = b.parse().unwrap();
            json!({"value":Type::native_bitstring(u.clone(),base.clone()).to_native_value().to_value(),"canonical":Type::bitstring(u,base).is_ok()})
        };
        println!("{r}");
    }
}
