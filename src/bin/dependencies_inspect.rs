use rust_dialyzer_poc::core::CoreProgram;
fn main() {
    let result = (|| -> Result<(), String> {
        let mut args = std::env::args().skip(1);
        let path = args
            .next()
            .ok_or("usage: dependencies_inspect CORE.json DEPENDENCIES.json")?;
        let dep = args.next().ok_or("missing dependency artifact")?;
        let core = CoreProgram::parse(&std::fs::read_to_string(path).map_err(|e| e.to_string())?)?;
        let store = rust_dialyzer_poc::dependencies::PriorPlt::parse(
            &std::fs::read_to_string(dep).map_err(|e| e.to_string())?,
            &core,
            &rust_dialyzer_poc::dependencies::ImportPlan::cold_targets(&core),
        )
        .and_then(|prior| prior.into_cold_store())?;
        println!(
            "{}",
            serde_json::to_string(&store.inspect()).map_err(|e| e.to_string())?
        );
        Ok(())
    })();
    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
