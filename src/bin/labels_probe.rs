use rust_dialyzer_poc::{
    core::CoreProgram,
    labels::{CoreLabel, Labels},
};
use serde::Deserialize;
use serde_json::json;
use std::io::{self, BufRead};
#[derive(Deserialize)]
struct Request {
    id: String,
    core_json: String,
    initial: u64,
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    for line in io::stdin().lock().lines() {
        let r: Request = serde_json::from_str(&line?)?;
        let core = CoreProgram::parse(&std::fs::read_to_string(r.core_json)?)?;
        let labels = Labels::build(&core, CoreLabel(r.initial))?;
        let modules:Vec<_>=core.modules.iter().zip(labels.nodes).map(|(module,nodes)|json!({"name":module.name,"labels":nodes.into_iter().map(|label|label.map(|label|label.0)).collect::<Vec<_>>()})).collect();
        println!(
            "{}",
            json!({"id":r.id,"next_label":labels.next_label.0,"modules":modules})
        );
    }
    Ok(())
}
