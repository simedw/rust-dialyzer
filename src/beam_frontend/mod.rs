//! Native debug-information frontend, pinned to Elixir 1.19.4 and OTP 28.3.
//! Unsupported compiler constructs are errors; this module never invokes a VM.
pub mod clean;
pub mod debug;
pub mod elixir;
mod erl_bifs;
pub mod erl_prepare;
pub mod lower;
pub mod stats;
pub mod term;

use crate::core::ErlangTerms;
use debug::{Backend, DebugModule};

pub fn forms(module: &DebugModule) -> Result<ErlangTerms, String> {
    match module.backend {
        Backend::Elixir => elixir::lift(&module.terms),
        Backend::Erlang => debug::erlang_forms(module),
    }
}

/// Native compiler output before analyzer transport/provenance integration.
/// Both roots own exact terms; intermediate serialization is unnecessary.
pub struct Compilation {
    pub module: String,
    pub source_forms: ErlangTerms,
    pub cleaned_core: ErlangTerms,
}

pub fn compile_beam(bytes: &[u8]) -> Result<Compilation, String> {
    let debug = debug::read_beam(bytes, crate::external_term::Limits::default())?;
    let source_forms = forms(&debug).map_err(|e| format!("forms {}: {e}", debug.module))?;
    let core =
        raw_core(&debug, &source_forms).map_err(|e| format!("core {}: {e}", debug.module))?;
    let cleaned_core = clean::clean(&core).map_err(|e| format!("clean {}: {e}", debug.module))?;
    Ok(Compilation {
        module: debug.module,
        source_forms,
        cleaned_core,
    })
}

/// Erlang debug forms require call qualification before v3_core. Expanded
/// Elixir forms enter v3_core directly, as in elixir_erl_compiler.
pub fn raw_core(module: &DebugModule, forms: &ErlangTerms) -> Result<ErlangTerms, String> {
    match module.backend {
        Backend::Elixir => lower::lower(forms, &module.module),
        Backend::Erlang => lower::lower(&erl_prepare::prepare(forms)?, &module.module),
    }
}
