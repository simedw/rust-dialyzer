use rust_dialyzer_poc::{core::CoreProgram, program::Program};
fn main() {
    let result = (|| -> Result<(), String> {
        let path = std::env::args()
            .nth(1)
            .ok_or("usage: program_inspect CORE.json")?;
        let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let core = CoreProgram::parse(&raw)?;
        let program = Program::build(&core)?;
        let policy = rust_dialyzer_poc::closure_dependencies::DependencyPolicy::parse(
            include_str!("../../corpus/dependency_policy.json"),
        )?;
        let closures = rust_dialyzer_poc::closure_dependencies::ClosureDependencies::analyze(
            &core, &program, &policy,
        )?;
        let plan = rust_dialyzer_poc::schedule::Plan::build(&program, closures)?;
        println!(
            "{}",
            serde_json::to_string_pretty(
                &serde_json::json!({"index":program.inspect(),"schedule":plan.inspect()})
            )
            .map_err(|e| e.to_string())?
        );
        Ok(())
    })();
    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
