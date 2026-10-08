#[path = "../../../src/phase_timings.rs"]
mod phase_timings;
use firefly_beam::{AbstractCode, ast::Form};
use firefly_diagnostics::CodeMap;
use firefly_intern::Symbol;
use firefly_pass::Pass;
use firefly_syntax_base::ApplicationMetadata;
use firefly_syntax_erl::passes::{
    AbstractErlangToAst, AstToCore, CanonicalizeSyntax, SemanticAnalysis,
};
use firefly_util::diagnostics::{CaptureEmitter, DiagnosticsConfig, DiagnosticsHandler};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};
fn maximum(debug: &str, field: &str) -> usize {
    debug
        .split(field)
        .skip(1)
        .filter_map(|p| p.split(|c: char| !c.is_ascii_digit()).next()?.parse().ok())
        .max()
        .unwrap_or(1)
}
pub fn lower(
    forms: AbstractCode,
    mut seed: native_beam_seed::PreparedCoreSeed,
) -> anyhow::Result<rust_dialyzer_poc::core::CoreModule> {
    let mut timings = phase_timings::PhaseTimings::new("beam_adapter");
    let debug = format!("{forms:?}");
    let lines = maximum(&debug, "line: ") + 2;
    let columns = maximum(&debug, "column: ") + 2;
    anyhow::ensure!(
        lines.checked_mul(columns + 1).unwrap_or(usize::MAX) <= 16 * 1024 * 1024,
        "synthetic source exceeds limit"
    );
    let synthetic = format!("{}\n", " ".repeat(columns)).repeat(lines);
    let codemap = Arc::new(CodeMap::new());
    for form in &forms.forms {
        if let Form::File(file) = form {
            codemap.add(
                PathBuf::from(file.original_file.as_str().get()),
                synthetic.clone(),
            );
        }
    }
    let emitter = Arc::new(CaptureEmitter::new());
    let diagnostics = Arc::new(DiagnosticsHandler::new(
        DiagnosticsConfig::default(),
        codemap.clone(),
        emitter.clone(),
    ));
    let app = ApplicationMetadata {
        name: Symbol::intern("prototype"),
        modules: BTreeMap::new(),
    };
    timings.mark("synthetic_codemap");
    let ast = AbstractErlangToAst::new(&diagnostics, &codemap).run(forms.into())?;
    timings.mark("abstract_forms_to_ast");
    let ast = SemanticAnalysis::new(&diagnostics, &app)
        .for_dialyzer()
        .run(ast)
        .map_err(|error| anyhow::anyhow!("{error}\n{}", emitter.captured()))?;
    timings.mark("semantic_analysis");
    let ast = CanonicalizeSyntax::new(codemap.clone()).run(ast)?;
    timings.mark("canonicalize_syntax");
    let mut core = AstToCore::new(diagnostics.clone())
        .for_dialyzer()
        .run(ast)?;
    crate::clean::clean(&mut core).map_err(anyhow::Error::msg)?;
    anyhow::ensure!(
        !diagnostics.has_errors(),
        "compiler diagnostics: {}",
        emitter.captured()
    );
    timings.mark("lower_to_core");
    seed.source_column_offset = 1;
    let output = crate::adapt(&core, &codemap, seed).map_err(anyhow::Error::msg)?;
    timings.mark("core_adapter_conversion");
    Ok(output)
}
