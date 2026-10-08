use rust_dialyzer_poc::core::CoreProgram;

fn main() {
    let mut args = std::env::args().skip(1);
    let result = (|| -> Result<(), String> {
        if args.next().as_deref() != Some("--inspect-core") {
            return Err("usage: core_inspect --inspect-core FILE".into());
        }
        let path = args.next().ok_or("missing Core JSON path")?;
        if args.next().is_some() {
            return Err("unexpected arguments".into());
        }
        let input = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let parsed = CoreProgram::parse(&input)?;
        println!(
            "{}",
            serde_json::to_string_pretty(&parsed.inspect()).map_err(|e| e.to_string())?
        );
        Ok(())
    })();
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
