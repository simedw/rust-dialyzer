use rust_dialyzer_poc::{
    core::CoreProgram,
    core_print::CorePrinter,
    dataflow_nodes::DataflowNodes,
    labels::{CoreLabel, Labels},
    patterns::Patterns,
    program::{ModuleId, NodeRef},
};
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn main() -> Result<(), String> {
    for line in io::stdin().lock().lines() {
        let row: Value =
            serde_json::from_str(&line.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let core = CoreProgram::parse(&row["core"].to_string())?;
        let labels = Labels::build(&core, CoreLabel(0))?;
        let nodes = DataflowNodes::build(&core, &labels)?;
        let patterns = Patterns::new(&nodes);
        let printer = CorePrinter {
            patterns: &patterns,
        };
        let result = printer.format_cerl(NodeRef {
            module: ModuleId(0),
            node: row["node"].as_u64().ok_or("node expected")? as usize,
        });
        println!(
            "{}",
            json!({"id":row["id"],"native":row["native"],"rust":result.map_err(|e|format!("{e:?}"))})
        )
    }
    Ok(())
}
