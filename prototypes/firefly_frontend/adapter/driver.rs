#[path = "../../../src/phase_timings.rs"]
mod phase_timings;
use firefly_beam::AbstractCode;
fn main() -> anyhow::Result<()> {
    let mut timings = phase_timings::PhaseTimings::new("beam_adapter");
    let args: Vec<String> = std::env::args().skip(1).collect();
    anyhow::ensure!(
        args.len() == 3,
        "usage: adapt-beam BEAM SEED.json OUTPUT.json"
    );
    let seed: serde_json::Value = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let forms = match seed
        .get("native_forms_etf")
        .and_then(serde_json::Value::as_str)
    {
        Some(path) => AbstractCode::from_abstract_forms(&std::fs::read(path)?)?,
        None => AbstractCode::from_beam_file(&args[0])?,
    };
    timings.mark("seed_json_and_forms_parse");
    let output = firefly_core_adapter::frontend::lower(
        forms,
        native_beam_seed::PreparedCoreSeed::from_wire_value(&seed).map_err(anyhow::Error::msg)?,
    )?;
    std::fs::write(&args[2], serde_json::to_vec(&output.to_wire_value())?)?;
    timings.mark("serialize_and_write_core");
    Ok(())
}
