use rust_dialyzer_poc::{
    core::CoreProgram,
    dataflow_map::DataflowMap,
    dataflow_nodes::DataflowNodes,
    labels::{CoreLabel, Labels},
    program::{ModuleId, NodeRef},
};
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn main() -> Result<(), String> {
    for line in io::stdin().lock().lines() {
        let row: Value =
            serde_json::from_str(&line.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let core = CoreProgram::parse(
            &std::fs::read_to_string(row["core_json"].as_str().ok_or("Core path expected")?)
                .map_err(|e| e.to_string())?,
        )?;
        let labels = Labels::build(&core, CoreLabel(0))?;
        let nodes = DataflowNodes::build(&core, &labels)?;
        let mut modules = Vec::new();
        for (index, module) in core.modules.iter().enumerate() {
            let scope = DataflowMap::default().begin_join();
            let mut map = scope.branch();
            nodes.mark_fresh(
                &[NodeRef {
                    module: ModuleId(index),
                    node: module.core.0,
                }],
                &mut map,
            )?;
            let types: serde_json::Map<String, Value> = map
                .types()
                .iter()
                .map(|(key, typ)| (key.native_integer().to_string(), typ.to_value()))
                .collect();
            modules.push(json!({"name":module.name,"types":types,"modified":map.modified().iter().map(|key|key.native_integer().parse::<serde_json::Number>().expect("validated integer identity")).collect::<Vec<_>>() }));
        }
        println!("{}", json!({"id":row["id"],"modules":modules}));
    }
    Ok(())
}
