use rust_dialyzer_poc::{core::CoreProgram, forms::Forms, program::Program};
fn main() {
    let result = (|| -> Result<(), String> {
        let path = std::env::args()
            .nth(1)
            .ok_or("usage: forms_inspect CORE.json")?;
        let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let core = CoreProgram::parse(&raw)?;
        let program = Program::build(&core)?;
        let forms = Forms::build(&core, &program)?;
        println!(
            "{}",
            serde_json::to_string(&forms.inspect()).map_err(|e| e.to_string())?
        );
        Ok(())
    })();
    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
