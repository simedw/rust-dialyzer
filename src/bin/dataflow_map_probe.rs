use rust_dialyzer_poc::{dataflow_map::DataflowMap, labels::CoreLabel, lattice::Type};
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn key(value: &Value) -> Result<CoreLabel, Box<dyn std::error::Error>> {
    Ok(CoreLabel(value.as_u64().ok_or("node label expected")?))
}
fn operations(
    mut map: DataflowMap,
    steps: &[Value],
) -> Result<DataflowMap, Box<dyn std::error::Error>> {
    for step in steps {
        match step["operation"].as_str().ok_or("operation expected")? {
            "enter" => map.enter_type(
                key(&step["key"])?,
                serde_json::from_value::<Type>(step["type"].clone())?,
            ),
            "literal" => map.enter_literal_substitution(
                key(&step["key"])?,
                serde_json::from_value::<Type>(step["type"].clone())?,
            ),
            "alias" => map.enter_variable_substitution(key(&step["key"])?, key(&step["value"])?),
            "join" => {
                let scope = map.begin_join();
                let branches = step["branches"]
                    .as_array()
                    .ok_or("branches expected")?
                    .iter()
                    .map(|steps| {
                        operations(
                            scope.branch(),
                            steps.as_array().ok_or("branch steps expected")?,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                map = scope.finish(&branches)?;
            }
            other => return Err(format!("unsupported operation {other}").into()),
        }
    }
    Ok(map)
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    for line in io::stdin().lock().lines() {
        let row: Value = serde_json::from_str(&line?)?;
        let map = operations(
            DataflowMap::default(),
            row["steps"].as_array().ok_or("steps expected")?,
        )?;
        let types: serde_json::Map<String, Value> = map
            .types()
            .iter()
            .map(|(key, value)| (key.native_integer(), value.to_value()))
            .collect();
        let substitutions: serde_json::Map<String, Value> = map
            .substitutions()
            .iter()
            .map(|(key, value)| {
                (
                    key.native_integer(),
                    json!(
                        value
                            .native_integer()
                            .parse::<serde_json::Number>()
                            .expect("validated integer identity")
                    ),
                )
            })
            .collect();
        let looked_up: Vec<_> = (0..16)
            .map(|node| map.lookup(CoreLabel(node)).to_value())
            .collect();
        println!(
            "{}",
            json!({"id":row["id"],"types":types,"substitutions":substitutions,"modified":map.modified().iter().map(|key|key.native_integer().parse::<serde_json::Number>().expect("validated integer identity")).collect::<Vec<_>>(),"lookup":looked_up})
        );
    }
    Ok(())
}
