use rust_dialyzer_poc::{
    compiled_definitions::PreparedDefinitions, core::CoreProgram, program::Program,
    type_exports::arena_to_value,
};
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn run(request: Value) -> Result<Value, String> {
    let core = CoreProgram::parse(
        &std::fs::read_to_string(request["core"].as_str().ok_or("core")?)
            .map_err(|e| e.to_string())?,
    )?;
    let program = Program::build(&core)?;
    let library = rust_dialyzer_poc::dependencies::PriorPlt::parse(
        &std::fs::read_to_string(request["environment"].as_str().ok_or("environment")?)
            .map_err(|e| e.to_string())?,
        &core,
        &rust_dialyzer_poc::dependencies::ImportPlan::cold_targets(&core),
    )
    .and_then(|prior| prior.into_cold_store())?;
    let outcome = match PreparedDefinitions::prepare(&core, &program, &library) {
        Ok(p) => {
            let records=p.records.into_values().map(|r|json!({"module":p.module_names[r.module.0],"tag":r.tag,"field_names":r.field_names,"fields":r.field_types.iter().map(|t|t.to_native_value().to_value()).collect::<Vec<_>>(),"prototype":r.prototype.to_native_value().to_value()})).collect::<Vec<_>>();
            let contracts=p.contracts.callables().chain(p.callbacks.callables()).map(|(mfa,c)|json!({"mfa":mfa,"callback":c.callback,"optional":c.optional,"capture":arena_to_value(&c.capture.arena().terms,c.capture.id())})).collect::<Vec<_>>();
            json!({"status":"ok","records":records,"contracts":contracts,"tables":p.tables.into_iter().filter(|(name,_)|core.modules.iter().any(|m| &m.name==name)).map(|(name,t)|(name,arena_to_value(&t.arena().terms,t.root()))).collect::<std::collections::BTreeMap<_,_>>(),"events":p.external_types.into_iter().map(|e|{let r=e.to_arena();arena_to_value(&r.arena().terms,r.id())}).collect::<Vec<_>>()})
        }
        Err(e) => {
            json!({"status":"error","stage":format!("{:?}",e.stage),"error":e.error.report(),"events":e.external_types.into_iter().map(|e|{let r=e.to_arena();arena_to_value(&r.arena().terms,r.id())}).collect::<Vec<_>>()})
        }
    };
    Ok(outcome)
}
fn main() -> Result<(), String> {
    for line in io::stdin().lock().lines() {
        let request =
            serde_json::from_str(&line.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let output = run(request)?;
        println!("{output}");
    }
    Ok(())
}
