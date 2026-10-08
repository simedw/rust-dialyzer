use rust_dialyzer_poc::guard_ops::{self, GuardEval, TypeTestResult};
use rust_dialyzer_poc::{
    lattice::{LatticeError, Type},
    native_error::NativeTransferError,
};
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn run(v: &Value) -> Result<Value, LatticeError> {
    let eval = match v["eval"].as_str() {
        Some("positive") => GuardEval::Positive,
        Some("negative") => GuardEval::Negative,
        _ => GuardEval::Unknown,
    };
    let ty =
        |key: &str| Type::from_value(&v[key]).expect("validated generated canonical Type input");
    Ok(match v["op"].as_str().expect("op") {
        "test_domain" => {
            json!({"kind":"type","value":guard_ops::type_test_domain(v["name"].as_str().expect("name"),v["arity"].as_u64().expect("arity") as usize)?.to_value()})
        }
        "eval_inf" => {
            json!({"kind":"type","value":guard_ops::eval_intersection(eval,&ty("actual"))?.to_value()})
        }
        "bind_test" => match guard_ops::bind_type_test(eval, &ty("expected"), &ty("actual"))? {
            TypeTestResult::Impossible => json!({"kind":"impossible"}),
            TypeTestResult::Refined { argument, result } => {
                json!({"kind":"refined","argument":argument.to_value(),"result":result.to_value()})
            }
        },
        _ => panic!("unknown generated operation"),
    })
}
fn main() {
    for line in io::stdin().lock().lines() {
        let v: Value = serde_json::from_str(&line.expect("line")).expect("request");
        let output = match run(&v) {
            Ok(result) => json!({"id":v["id"],"ok":result}),
            Err(error) => json!({"id":v["id"],"error":NativeTransferError::from(error).report()}),
        };
        println!("{output}");
    }
}
