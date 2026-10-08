use rust_dialyzer_poc::{core::CoreProgram, warnings::Registry};
use rust_dialyzer_poc::{returns, warning_policy as policy};
use std::io::{self, BufRead};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let registry = Registry::load(include_str!("../../corpus/warnings.json"))?;
    let bifs =
        rust_dialyzer_poc::bifs::registry::Registry::load(include_str!("../../corpus/bifs.json"))?;
    let classify = std::env::args().any(|a| a == "--returns");
    for line in io::stdin().lock().lines() {
        let path = line?;
        let core = CoreProgram::parse(&std::fs::read_to_string(path)?)?;
        if classify {
            let functions = core.modules.iter().map(|m| {
                let fs = m.nodes.iter().filter(|n| matches!(n.expr, rust_dialyzer_poc::core::CoreExpr::Fun { .. })).map(|n| {
                    Ok(serde_json::json!({"node":n.id.0,"kind":returns::classify(m,n.id,&bifs)?}))
                }).collect::<Result<Vec<_>,String>>()?;
                Ok(serde_json::json!({"module":m.name,"functions":fs}))
            }).collect::<Result<Vec<_>,String>>()?;
            println!("{}", serde_json::to_string(&functions)?);
        } else {
            println!(
                "{}",
                serde_json::to_string(&policy::Policy::build(
                    &core,
                    &registry,
                    &registry.default_profile()
                ))?
            );
        }
    }
    Ok(())
}
