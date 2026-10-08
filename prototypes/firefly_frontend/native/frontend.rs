use rust_dialyzer_poc::{
    external_term::{Limits, encode},
    native_runtime_hash::NativeAtomLayout,
    ordered_parallel::ordered_prefetch,
};
use serde_json::Value;

pub fn lower(
    paths: &[Value],
    layout: &NativeAtomLayout,
    compiler: elixir_frontend::CompilerVersion,
    workers: usize,
) -> Result<Vec<rust_dialyzer_poc::core::CoreModule>, String> {
    ordered_prefetch(
        paths.len(),
        workers,
        |index| {
            let path = paths[index].as_str().ok_or("BEAM path must be string")?;
            native_beam_seed::prepare(path, Some(layout), compiler)
                .map_err(|error| format!("{path}: BEAM preparation: {error}"))
        },
        |index, prepared| {
            let path = paths[index].as_str().expect("prepared path");
            let bytes = encode(&prepared.forms, Limits::default(), false)
                .map_err(|error| error.to_string())?;
            let forms = firefly_beam::AbstractCode::from_abstract_forms(&bytes)
                .map_err(|error| format!("{path}: abstract forms decoding: {error}"))?;
            firefly_core_adapter::frontend::lower(forms, prepared.seed)
                .map_err(|error| format!("{path}: Core lowering: {error}"))
        },
    )
}
