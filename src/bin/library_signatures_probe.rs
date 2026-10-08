use rust_dialyzer_poc::{
    body_calls::CallWorld,
    core::CoreProgram,
    dependencies::{ImportPlan, PriorPlt},
};
fn main() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 2 {
        return Err("usage: library_signatures_probe CORE DEPENDENCIES".into());
    }
    let read = |path: &str| std::fs::read_to_string(path).map_err(|error| error.to_string());
    let core = CoreProgram::parse(&read(&args[0])?)?;
    let library = PriorPlt::parse(&read(&args[1])?, &core, &ImportPlan::cold_targets(&core))?
        .into_cold_store()?;
    let world = CallWorld::from_library(&library);
    for name in library.summary_names() {
        let signature = world
            .named_signature(name)?
            .expect("listed library summary");
        println!("{}", serde_json::json!({"mfa":name,"signature":signature}));
    }
    Ok(())
}
