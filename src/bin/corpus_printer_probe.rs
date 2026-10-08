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
        let core = CoreProgram::parse(
            &std::fs::read_to_string(row["core_json"].as_str().ok_or("Core path expected")?)
                .map_err(|e| e.to_string())?,
        )?;
        let labels = Labels::build(&core, CoreLabel(0))?;
        let nodes = DataflowNodes::build(&core, &labels)?;
        let patterns = Patterns::new(&nodes);
        let printer = CorePrinter {
            patterns: &patterns,
        };
        let modules=core.modules.iter().enumerate().map(|(i,m)|json!({"module":m.name,"text":printer.format_cerl(NodeRef{module:ModuleId(i),node:m.core.0}).map_err(|e|format!("{e:?}"))})).collect::<Vec<_>>();
        println!("{}", json!({"id":row["id"],"modules":modules}))
    }
    Ok(())
}
