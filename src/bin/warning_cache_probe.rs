use rust_dialyzer_poc::incremental::{
    ModuleWarning, WarningMap, merge_dependencies, merge_warnings, select_warnings,
};
use rust_dialyzer_poc::invalidation::ReverseDependencies;
use serde::Deserialize;
use std::io::{self, BufRead};
#[derive(Deserialize)]
struct Request {
    id: usize,
    collected: Option<Vec<ModuleWarning<String>>>,
    unknown: Vec<ModuleWarning<String>>,
    stored: Option<WarningMap<String>>,
    modules: Vec<String>,
    old_dependencies: ReverseDependencies,
    new_dependencies: ReverseDependencies,
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    for line in io::stdin().lock().lines() {
        let input: Request = serde_json::from_str(&line?)?;
        let merged = merge_warnings(
            input.collected.as_deref(),
            &input.unknown,
            input.stored.as_ref(),
        );
        let selected = merged
            .as_ref()
            .map(|map| select_warnings(map, &input.modules));
        println!(
            "{}",
            serde_json::json!({"id":input.id,"merged":merged,"selected":selected,"dependencies":merge_dependencies(&input.old_dependencies,&input.new_dependencies)})
        );
    }
    Ok(())
}
