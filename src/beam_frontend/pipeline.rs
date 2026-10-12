//! Native BEAM compilation and in-memory analyzer setup.
use super::{
    Compilation, adapter, compile_beam,
    runtime_context::{IdentityOrigin, RuntimeContext},
};
use crate::{
    constraints::closure_order::ClosureCodeProfile,
    core::{CoreProgram, ErlangTerm},
    external_term::Limits,
    fast_hash::FastHashSet,
    native_runtime_hash::NativeAtomLayout,
    ordered_parallel::setup_map,
};
use serde_json::{Value, json};
use std::time::Instant;

pub enum ContextSource<'a> {
    Imported {
        context: &'a Value,
        closure_profile: &'a Value,
    },
    Native {
        typesig_beam: &'a [u8],
        contracts_beam: &'a [u8],
    },
}
pub struct LoadedNative {
    pub core: CoreProgram,
    pub closure_profile: ClosureCodeProfile,
    pub timings: Value,
}
fn atom_names(compilations: &[Compilation]) -> Vec<String> {
    let mut names = FastHashSet::default();
    for compilation in compilations {
        names.insert(compilation.module.as_str());
        for arena in [&compilation.source_forms, &compilation.cleaned_core] {
            let mut visited = vec![false; arena.terms.len()];
            let mut pending = vec![arena.root];
            while let Some(id) = pending.pop() {
                if std::mem::replace(&mut visited[id.0], true) {
                    continue;
                }
                match &arena.terms[id.0] {
                    ErlangTerm::Atom(name) => {
                        names.insert(name.as_str());
                    }
                    ErlangTerm::ExternalFun {
                        module, function, ..
                    } => {
                        names.insert(module.as_str());
                        names.insert(function.as_str());
                    }
                    ErlangTerm::Tuple(items) => pending.extend(items.iter().copied()),
                    ErlangTerm::List { items, tail } => {
                        pending.extend(items.iter().copied());
                        pending.push(*tail);
                    }
                    ErlangTerm::Map(entries) => {
                        for &(key, value) in entries {
                            pending.extend([key, value]);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    names.into_iter().map(str::to_owned).collect()
}

pub fn load(paths: &[String], source: ContextSource<'_>) -> Result<LoadedNative, String> {
    if paths.is_empty() {
        return Err("native frontend requires at least one BEAM file".into());
    }
    let started = Instant::now();
    let compiled = setup_map(paths, |path| {
        std::fs::read(path)
            .map_err(|e| format!("{path}: {e}"))
            .and_then(|bytes| compile_beam(&bytes).map_err(|e| format!("{path}: {e}")))
    })
    .into_iter()
    .collect::<Result<Vec<_>, _>>()?;
    let compile_seconds = started.elapsed().as_secs_f64();
    let mut loaded = assemble(compiled, source)?;
    loaded.timings["compile_seconds"] = compile_seconds.into();
    Ok(loaded)
}
/// Assemble native compilation results in one atom scope before analysis.
/// Oracle tests can exercise this boundary independently of Core lowering.
pub fn assemble(
    compiled: Vec<Compilation>,
    source: ContextSource<'_>,
) -> Result<LoadedNative, String> {
    if compiled.is_empty() {
        return Err("native frontend requires at least one compilation".into());
    }
    let started = Instant::now();
    let runtime = match source {
        ContextSource::Imported {
            context,
            closure_profile,
        } => RuntimeContext::import(context.clone(), closure_profile)?,
        ContextSource::Native {
            typesig_beam,
            contracts_beam,
        } => RuntimeContext::native(
            atom_names(&compiled),
            typesig_beam,
            contracts_beam,
            Limits::default(),
        )?,
    };
    let layout =
        NativeAtomLayout::parse(&runtime.context["atom_layout"]).map_err(|e| e.to_string())?;
    let context_seconds = started.elapsed().as_secs_f64();
    let started = Instant::now();
    let modules = setup_map(&compiled, |compilation| {
        adapter::module(
            Compilation {
                module: compilation.module.clone(),
                source_forms: compilation.source_forms.clone(),
                cleaned_core: compilation.cleaned_core.clone(),
            },
            &layout,
        )
        .map_err(|e| format!("{}: {e}", compilation.module))
    })
    .into_iter()
    .collect::<Result<Vec<_>, _>>()?;
    drop(compiled);
    let adapter_seconds = started.elapsed().as_secs_f64();
    let started = Instant::now();
    let core = CoreProgram::from_modules(&runtime.context, modules)?;
    let validation_seconds = started.elapsed().as_secs_f64();
    let node_count: usize = core.modules.iter().map(|module| module.nodes.len()).sum();
    let term_count: usize = core.modules.iter().map(|module| module.terms.len()).sum();
    let atom_count = runtime.context["atom_layout"]["atom_hashes"]
        .as_object()
        .unwrap()
        .len();
    Ok(LoadedNative {
        core,
        closure_profile: runtime.closure_profile,
        timings: json!({"mode":"native_beam", "identity_origin":match runtime.identity_origin {
            IdentityOrigin::ImportedContext => "imported_context", IdentityOrigin::DeterministicNativeScope => "deterministic_native_scope" },
            "context_seconds":context_seconds, "atoms":atom_count, "nodes":node_count, "terms":term_count,
            "adapter_seconds":adapter_seconds,"validation_seconds":validation_seconds}),
    })
}
