use rust_dialyzer_poc::{
    core::{CoreProgram, ErlangTerms},
    core_capture::CoreCaptures,
    labels::{CoreLabel, Labels},
    program::ModuleId,
    term_order::CaptureEnvironment,
};
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn run(value: &Value) -> Result<Value, String> {
    let serialized = if let Some(path) = value["core_json"].as_str() {
        std::fs::read_to_string(path).map_err(|e| e.to_string())?
    } else {
        value["program"].to_string()
    };
    let core = CoreProgram::parse(&serialized)?;
    let labels = Labels::build(&core, CoreLabel(value["initial"].as_u64().unwrap_or(0)))?;
    let cached: Value = if let Some(path) = value["native_json"].as_str() {
        serde_json::from_str(&std::fs::read_to_string(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?
    } else {
        json!({"captures": value["native"], "next_label": value["next_label"]})
    };
    let expected = cached["captures"]
        .as_array()
        .ok_or("native module captures missing")?;
    if expected.len() != core.modules.len() {
        return Err("native module capture count differs".into());
    }
    let mut terms = 0;
    let mut nodes = 0;
    let mut changed = 0;
    for (index, module) in core.modules.iter().enumerate() {
        let captures = CoreCaptures::build(&core, &labels, ModuleId(index))?;
        let actual = captures.environment(&[module.core])?;
        let native_terms = ErlangTerms::parse(&expected[index])?;
        let actual_terms = captures.source_terms(&[module.core])?;
        let limits = rust_dialyzer_poc::external_term::Limits::default();
        let actual_bytes = rust_dialyzer_poc::external_term::encode(&actual_terms, limits, false)
            .map_err(|e| e.to_string())?;
        let native_bytes = rust_dialyzer_poc::external_term::encode(&native_terms, limits, false)
            .map_err(|e| e.to_string())?;
        if actual_bytes != native_bytes {
            return Err(format!(
                "exact labeled Core term encoding differs in module {}",
                module.name
            ));
        }
        let native = CaptureEnvironment::literals(&native_terms)?;
        if actual.compare(&native)? != std::cmp::Ordering::Equal {
            return Err(format!(
                "labeled Core capture differs in module {}",
                module.name
            ));
        }
        terms += module.terms.len();
        nodes += module.nodes.len();
        changed += (0..module.nodes.len())
            .filter(|node| {
                labels.is_reannotated(rust_dialyzer_poc::program::NodeRef {
                    module: ModuleId(index),
                    node: *node,
                })
            })
            .count();
    }
    Ok(
        json!({"id":value["id"],"modules":core.modules.len(),"nodes":nodes,"source_terms":terms,"reannotated":changed,"next_label":labels.next_label.0,"matched":true}),
    )
}
fn main() {
    for line in io::stdin().lock().lines() {
        let result = (|| {
            let value: Value = serde_json::from_str(&line.map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            run(&value)
        })();
        println!(
            "{}",
            match result {
                Ok(v) => v,
                Err(e) => json!({"error":e}),
            }
        );
    }
}
