use rust_dialyzer_poc::{core::CoreProgram, type_exports::arena_to_value};
fn main() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 2 {
        return Err("usage: library_contracts_probe CORE DEPENDENCIES".into());
    }
    let core = CoreProgram::parse(&std::fs::read_to_string(&args[0]).map_err(|e| e.to_string())?)?;
    let library = rust_dialyzer_poc::dependencies::PriorPlt::parse(
        &std::fs::read_to_string(&args[1]).map_err(|e| e.to_string())?,
        &core,
        &rust_dialyzer_poc::dependencies::ImportPlan::cold_targets(&core),
    )
    .and_then(|prior| prior.into_cold_store())?;
    for (name, contract) in &library.contracts {
        let raw = contract.captured_record();
        println!(
            "{}",
            serde_json::json!({"kind":"contract","mfa":name,"record":arena_to_value(&raw.arena().terms,raw.id())})
        );
    }
    for row in library
        .callbacks
        .original()
        .map_err(|e| format!("{e:?}"))?
        .rows()
    {
        println!(
            "{}",
            serde_json::json!({"kind":"callback","mfa":row.callable(),"record":arena_to_value(&row.capture.arena().terms,row.capture.id())})
        );
    }
    let world = rust_dialyzer_poc::body_calls::CallWorld::from_library(&library);
    let mut signatures = 0;
    for name in library.summary_names() {
        signatures += usize::from(world.named_signature(name)?.is_some());
    }
    let mut active_contracts = 0;
    for name in library.contracts.keys() {
        active_contracts += usize::from(world.active_contract(name)?.is_some());
    }
    eprintln!(
        "{}",
        serde_json::json!({"signatures":signatures,"active_contracts":active_contracts})
    );
    Ok(())
}
