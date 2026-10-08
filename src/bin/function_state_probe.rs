use rust_dialyzer_poc::{
    call_transfer::Signature, dataflow_map::DataflowMap, lattice::Type, program::FunctionName,
};
use rust_dialyzer_poc::{
    function_state::{FunctionDefinition, FunctionState},
    worklist::FunctionKey,
};
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn key(v: &Value) -> FunctionKey {
    if v == "top" {
        FunctionKey::Top
    } else {
        FunctionKey::Label(rust_dialyzer_poc::labels::CoreLabel(v.as_u64().unwrap()))
    }
}
fn wire(k: FunctionKey) -> Value {
    match k {
        FunctionKey::Top => json!("top"),
        FunctionKey::Label(n) => json!(n.0),
    }
}
fn snap(s: &FunctionState) -> Value {
    json!({"entries":s.entries().iter().map(|(k,e)|{let sig=e.signature();(match k{FunctionKey::Top=>"top".into(),FunctionKey::Label(n)=>n.0.to_string()},json!({"called":e.called(),"args":sig.arguments.iter().map(Type::to_value).collect::<Vec<_>>(),"result":sig.result.to_value()}))}).collect::<serde_json::Map<String,Value>>(),"environments":s.environments().iter().map(|(key,map)|(match key{FunctionKey::Top=>"top".into(),FunctionKey::Label(n)=>n.0.to_string()},json!(map.types().iter().map(|(k,t)|(k.native_integer(),t.to_value())).collect::<serde_json::Map<String,Value>>()))).collect::<serde_json::Map<String,Value>>(),"queue":s.work().ordered().into_iter().map(wire).collect::<Vec<_>>(),"reachable":s.reachable().iter().map(|n|n.0).collect::<Vec<_>>(),"warning_mode":s.warning_mode()})
}
fn main() {
    for line in io::stdin().lock().lines() {
        let row: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let defs = row["definitions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| {
                let k = key(&d["key"]);
                let arity = d["arity"].as_u64().unwrap() as usize;
                FunctionDefinition {
                    key: k,
                    node: None,
                    arity,
                    escaping: d["escaping"].as_bool().unwrap(),
                    name: if d["named"].as_bool().unwrap() {
                        Some(FunctionName {
                            module: "fixture".into(),
                            name: format!("function_{}", d["key"]),
                            arity,
                        })
                    } else {
                        None
                    },
                    raw_signature: if d["raw"].is_null() {
                        None
                    } else {
                        Some(Signature {
                            arguments: vec![Type::any(); arity],
                            result: Type::from_value(&d["raw"]).unwrap(),
                        })
                    },
                    callers: row["caller_facts"][match k {
                        FunctionKey::Top => "top".into(),
                        FunctionKey::Label(n) => n.0.to_string(),
                    }]
                    .as_array()
                    .map(|xs| xs.iter().map(key).collect())
                    .unwrap_or_default(),
                }
            })
            .collect();
        let mut state = FunctionState::new(defs);
        let order = state.order().iter().copied().map(wire).collect::<Vec<_>>();
        let mut steps = Vec::new();
        for op in row["operations"].as_array().unwrap() {
            let k = key(&op["key"]);
            match op["kind"].as_str().unwrap() {
                "pop" => {
                    state.pop_work();
                }
                "warning" => state.set_warning_mode(),
                "capture" => {
                    let mut map = DataflowMap::default();
                    if let Some(values) = op["environment"].as_array() {
                        for pair in values {
                            map.enter_type(
                                rust_dialyzer_poc::labels::CoreLabel(pair[0].as_u64().unwrap()),
                                Type::from_value(&pair[1]).unwrap(),
                            );
                        }
                    }
                    let FunctionKey::Label(label) = k else {
                        panic!("capture requires a Core function label")
                    };
                    state.capture(label, map).unwrap();
                }
                "forward" => {
                    state
                        .forward_arguments(
                            k,
                            &op["args"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .map(|v| Type::from_value(v).unwrap())
                                .collect::<Vec<_>>(),
                        )
                        .unwrap();
                }
                "return" => {
                    state
                        .update_return(
                            k,
                            &op["args"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .map(|v| Type::from_value(v).unwrap())
                                .collect::<Vec<_>>(),
                            &Type::from_value(&op["result"]).unwrap(),
                        )
                        .unwrap();
                }
                _ => panic!(),
            }
            steps.push(snap(&state))
        }
        let functions = state
            .function_types()
            .iter()
            .map(|(key, s)| {
                (
                    key.0.to_string(),
                    Type::function(Some(s.arguments.clone()), s.result.clone()).to_value(),
                )
            })
            .collect::<serde_json::Map<String, Value>>();
        println!(
            "{}",
            json!({"id":row["id"],"order":order,"steps":steps,"functions":functions,"caller_facts":row["caller_facts"]})
        );
    }
}
