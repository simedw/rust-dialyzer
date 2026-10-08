use rust_dialyzer_poc::external_term::{Limits, decode, encode};
fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        return Err("usage: native-beam-seed BEAM OUTPUT.json [ORACLE_FORMS.etf] [--atom-layout ATOM_LAYOUT.json] [--compiler 1.19.x|1.20.x]; compiler defaults to 1.20.0".into());
    }
    let mut oracle = None;
    let mut layout = None;
    let mut compiler = elixir_frontend::CompilerVersion::V1_20;
    let mut pending = args[2..].iter();
    while let Some(arg) = pending.next() {
        match arg.as_str() {
            "--atom-layout" => {
                let path = pending.next().ok_or("--atom-layout requires a path")?;
                let value: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
                        .map_err(|e| e.to_string())?;
                layout = Some(
                    rust_dialyzer_poc::native_runtime_hash::NativeAtomLayout::parse(&value)
                        .map_err(|e| e.to_string())?,
                );
            }
            "--compiler" => {
                compiler = elixir_frontend::CompilerVersion::parse(
                    pending.next().ok_or("--compiler requires a version")?,
                )?
            }
            path if !path.starts_with('-') && oracle.is_none() => oracle = Some(path),
            _ => return Err(format!("unexpected argument {arg}")),
        }
    }
    let prepared = native_beam_seed::prepare(&args[0], layout.as_ref(), compiler)?;
    if let Some(path) = oracle {
        let expected = decode(
            &std::fs::read(path).map_err(|e| e.to_string())?,
            Limits::default(),
        )
        .map_err(|e| e.to_string())?;
        if !rust_dialyzer_poc::term_order::compare_map_keys(
            &prepared.forms,
            prepared.forms.root,
            &expected,
            expected.root,
        )?
        .is_eq()
        {
            return Err("native source forms differ from original OTP forms".into());
        }
        println!("Native source forms match original OTP forms.");
    }
    let mut seed = prepared.seed.to_wire_value();
    if prepared.translated {
        let output = std::path::Path::new(&args[1]).with_extension("forms.etf");
        std::fs::write(
            &output,
            encode(&prepared.forms, Limits::default(), false).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        seed["native_forms_etf"] = serde_json::json!(output);
    }
    std::fs::write(
        &args[1],
        serde_json::to_vec_pretty(&seed).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    println!("Native source forms and attributes extracted: {}", args[1]);
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
