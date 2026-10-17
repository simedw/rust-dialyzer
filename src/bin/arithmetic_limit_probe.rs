use num_bigint::BigInt;
use num_traits::{Signed, Zero};
use rust_dialyzer_poc::{
    bifs::registry::{Mfa, Registry},
    fingerprint::md5,
    lattice::{Endpoint, EndpointView, IntegerView, NumberView, Type, TypeView},
};
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn number(v: &Value) -> BigInt {
    match v["kind"].as_str().unwrap() {
        "small" => v["value"].as_str().unwrap().parse().unwrap(),
        "power" => {
            let x = BigInt::from(1u8) << v["bits"].as_u64().unwrap() as usize;
            if v["negative"].as_bool().unwrap_or(false) {
                -x
            } else {
                x
            }
        }
        "mask" => {
            let x: BigInt = (BigInt::from(1u8) << v["bits"].as_u64().unwrap() as usize) - 1;
            if v["negative"].as_bool().unwrap_or(false) {
                -x
            } else {
                x
            }
        }
        k => panic!("unknown descriptor {k}"),
    }
}
fn input(v: &Value) -> Type {
    if let Some(r) = v.get("range") {
        Type::range(
            Endpoint::Finite(number(&r[0])),
            Endpoint::Finite(number(&r[1])),
        )
        .unwrap()
    } else {
        Type::integers(v.as_array().unwrap().iter().map(number))
    }
}
fn integer(v: &BigInt) -> Value {
    let bytes = if v.is_zero() {
        Vec::new()
    } else {
        v.to_bytes_be().1
    };
    json!({"sign":if v.is_zero(){0}else if v.is_negative(){-1}else{1},"bits":v.bits(),"md5":md5(&bytes).iter().map(|b|format!("{b:02x}")).collect::<String>()})
}
fn endpoint(v: EndpointView<'_>) -> Value {
    match v {
        EndpointView::Finite(x) => integer(x),
        EndpointView::NegativeInfinity => json!("neg_inf"),
        EndpointView::PositiveInfinity => json!("pos_inf"),
    }
}
fn domain(t: &Type) -> Value {
    match t.view() {
        TypeView::Families(s) => match s.number {
            Some(NumberView::Number) => json!({"kind":"number"}),
            Some(NumberView::Integer(IntegerView::All)) => json!({"kind":"integer_all"}),
            Some(NumberView::Integer(IntegerView::Set(s))) => {
                json!({"kind":"set","values":s.iter().map(integer).collect::<Vec<_>>()})
            }
            Some(NumberView::Integer(IntegerView::Range { from, to })) => {
                json!({"kind":"range","from":endpoint(from),"to":endpoint(to)})
            }
            _ => panic!("noninteger domain"),
        },
        _ => panic!("noninteger type"),
    }
}
fn main() {
    let registry = Registry::standard().unwrap();
    for l in io::stdin().lock().lines() {
        let r: Value = serde_json::from_str(&l.unwrap()).unwrap();
        let a = input(&r["left"]);
        let b = input(&r["right"]);
        let t = registry.transfer(
            &Mfa {
                module: "erlang".into(),
                function: r["op"].as_str().unwrap().into(),
                arity: 2,
            },
            &[a, b],
        );
        let actual = match t {
            Ok(t) => domain(&t.result),
            Err(rust_dialyzer_poc::bifs::registry::TransferError::NativeException(error)) => {
                error.report()
            }
            Err(error) => panic!("{error}"),
        };
        println!(
            "{}",
            json!({"id":r["id"],"matched":actual==r["expected"],"actual":actual})
        );
    }
}
