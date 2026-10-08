use rust_dialyzer_poc::external_term::{decode, encode};
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1)
    }
}
fn run() -> Result<(), String> {
    let args = std::env::args().collect::<Vec<_>>();
    if args.len() != 3 {
        return Err("usage: translate-elixir metadata.etf forms.etf".into());
    }
    let bytes = std::fs::read(&args[1]).map_err(|e| e.to_string())?;
    let a = decode(&bytes, Default::default()).map_err(|e| e.to_string())?;
    let forms = elixir_frontend::translate(&a, a.root)?;
    let out = encode(&forms, Default::default(), false).map_err(|e| e.to_string())?;
    std::fs::write(&args[2], out).map_err(|e| e.to_string())
}
