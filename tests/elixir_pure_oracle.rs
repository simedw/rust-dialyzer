//! Replays Elixir's own results for its compile-time inline-pure calls,
//! recorded by scripts/elixir_pure_oracle.exs.

use rust_dialyzer_poc::beam_frontend::elixir_pure::{Term, evaluate};
use serde_json::{Value, json};

fn decode(v: &Value) -> Term {
    let (kind, inner) = v.as_object().unwrap().iter().next().unwrap();
    let items = || inner.as_array().unwrap().iter().map(decode).collect();
    match kind.as_str() {
        "a" => Term::Atom(inner.as_str().unwrap().into()),
        "i" => Term::Int(inner.as_str().unwrap().parse().unwrap()),
        "b" => Term::Bin(
            (0..inner.as_str().unwrap().len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&inner.as_str().unwrap()[i..i + 2], 16).unwrap())
                .collect(),
        ),
        "l" => Term::List(items()),
        "t" => Term::Tuple(items()),
        "m" => Term::Map(
            inner
                .as_array()
                .unwrap()
                .iter()
                .map(|pair| (decode(&pair[0]), decode(&pair[1])))
                .collect(),
        ),
        other => panic!("unknown term kind {other}"),
    }
}

fn encode(t: &Term) -> Value {
    match t {
        Term::Atom(a) => json!({"a": a}),
        Term::Int(i) => json!({"i": i.to_string()}),
        Term::Float(f) => json!({"f": f}),
        Term::Bin(b) => json!({"b": b.iter().map(|x| format!("{x:02x}")).collect::<String>()}),
        Term::List(items) => json!({"l": items.iter().map(encode).collect::<Vec<_>>()}),
        Term::Tuple(items) => json!({"t": items.iter().map(encode).collect::<Vec<_>>()}),
        Term::Map(entries) => {
            let mut pairs: Vec<Value> = entries.iter().map(|(k, v)| json!([encode(k), encode(v)])).collect();
            pairs.sort_by_key(|p| p[0].to_string());
            json!({"m": pairs})
        }
    }
}

#[test]
fn inline_pure_calls_match_elixir() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/elixir_pure_cases.json")).unwrap();
    let mut failures = vec![];
    for case in &cases {
        let args: Vec<Term> = case["args"].as_array().unwrap().iter().map(decode).collect();
        let module = case["module"].as_str().unwrap();
        let function = case["function"].as_str().unwrap();
        let actual = match evaluate(&module, function, &args) {
            Ok(Some(value)) => encode(&value),
            Ok(None) => json!("raises"),
            Err(unsupported) => json!({"unsupported": unsupported}),
        };
        // Re-encode so map pairs use this side's ordering.
        let expected = match &case["result"] {
            Value::String(s) => json!(s),
            value => encode(&decode(value)),
        };
        if actual != expected {
            failures.push(format!(
                "{module}.{function}({}):\n  expected {expected}\n  actual   {actual}",
                case["args"]
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} cases differ:\n{}",
        failures.len(),
        cases.len(),
        failures.iter().take(15).cloned().collect::<Vec<_>>().join("\n")
    );
}
