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
                    ErlangTerm::ExternalFun(external_fun_payload) => {
                        let crate::core::ExternalFun {
                            module, function, ..
                        } = external_fun_payload.as_ref();

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
                    #[cfg(otp_domain = "29")]
                    ErlangTerm::Record(record) => {
                        names.insert(record.module.as_str());
                        names.insert(record.name.as_str());
                        for (name, value) in &record.fields { names.insert(name.as_str()); pending.push(*value); }
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
    assemble_direct(compiled, source, false)
}
fn assemble_direct(
    compiled: Vec<Compilation>,
    source: ContextSource<'_>,
    direct: bool,
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
        let adapt = if direct {
            adapter::module_direct
        } else {
            adapter::module
        };
        adapt(
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
    complete(runtime, modules, context_seconds, adapter_seconds)
}
fn complete(
    runtime: RuntimeContext,
    modules: Vec<crate::core::CoreModule>,
    context_seconds: f64,
    adapter_seconds: f64,
) -> Result<LoadedNative, String> {
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

/// Experimental emission paths; the reference path remains the default.
#[derive(Clone, Copy, Debug, Default)]
pub enum Emission {
    #[default]
    Reference,
    Direct,
    Fused,
}
impl Emission {
    pub fn name(self) -> &'static str {
        match self {
            Self::Reference => "reference",
            Self::Direct => "direct",
            Self::Fused => "fused",
        }
    }
}
pub fn load_emission(
    paths: &[String],
    source: ContextSource<'_>,
    emission: Emission,
) -> Result<LoadedNative, String> {
    if paths.is_empty() {
        return Err("native frontend requires at least one BEAM file".into());
    }
    let started = Instant::now();
    let mut loaded = match emission {
        Emission::Reference => load(paths, source)?,
        Emission::Direct => {
            let compiled = setup_map(paths, |path| {
                std::fs::read(path)
                    .map_err(|e| format!("{path}: {e}"))
                    .and_then(|bytes| compile_beam(&bytes).map_err(|e| format!("{path}: {e}")))
            })
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;
            let elapsed = started.elapsed().as_secs_f64();
            let mut loaded = assemble_direct(compiled, source, true)?;
            loaded.timings["compile_seconds"] = elapsed.into();
            loaded
        }
        Emission::Fused => load_fused(paths, source)?,
    };
    loaded.timings["core_emission"] = emission.name().into();
    Ok(loaded)
}
fn fused_atom_names(
    module: &str,
    forms: &crate::core::ErlangTerms,
    view: &super::clean_view::CleanView,
) -> Result<FastHashSet<String>, String> {
    let mut names = view.atoms()?;
    names.insert(module.to_owned());
    let mut visited = vec![false; forms.terms.len()];
    let mut pending = vec![forms.root];
    while let Some(id) = pending.pop() {
        if std::mem::replace(&mut visited[id.0], true) {
            continue;
        }
        match &forms.terms[id.0] {
            ErlangTerm::Atom(name) => {
                if !names.contains(name) {
                    names.insert(name.clone());
                }
            }
            ErlangTerm::ExternalFun(external_fun_payload) => {
                let crate::core::ExternalFun {
                    module, function, ..
                } = external_fun_payload.as_ref();

                names.insert(module.clone());
                names.insert(function.clone());
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
    Ok(names)
}
struct TraceOnDrop<'a>(&'static str, &'a str, u64);
impl Drop for TraceOnDrop<'_> {
    fn drop(&mut self) {
        if crate::job_trace::enabled() {
            let t1 = crate::job_trace::now();
            crate::job_trace::record(|| serde_json::json!({"ev":self.0,"name":self.1,"t0":self.2,"t1":t1}));
        }
    }
}
fn load_fused(paths: &[String], source: ContextSource<'_>) -> Result<LoadedNative, String> {
    let started = Instant::now();
    let discover_atoms = matches!(&source, ContextSource::Native { .. });
    // BEAM size predicts compile and adapter cost; start large modules first.
    let sizes: Vec<u64> = paths
        .iter()
        .map(|path| std::fs::metadata(path).map_or(0, |m| m.len()))
        .collect();
    let mut compiled = crate::ordered_parallel::setup_map_owned_by_cost(paths.iter().collect(), &sizes, |path| {
        let t0 = crate::job_trace::now();
        let _trace = TraceOnDrop("fe_compile", path, t0);
        let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
        let raw = super::compile_beam_raw(&bytes).map_err(|e| format!("{path}: {e}"))?;
        let raw = super::RawCompilation {
            module: raw.module,
            source_forms: super::term::compact(raw.source_forms)?,
            raw_core: super::term::compact(raw.raw_core)?,
        };
        let view = super::clean_view::CleanView::new(raw.raw_core)?;
        let atoms = if discover_atoms {
            Some(
                fused_atom_names(&raw.module, &raw.source_forms, &view)
                    .map_err(|e| format!("{path}: {e}"))?,
            )
        } else {
            None
        };
        Ok((raw.module, raw.source_forms, view, atoms))
    })
    .into_iter()
    .collect::<Result<Vec<_>, String>>()?;
    let compile_seconds = started.elapsed().as_secs_f64();
    let started = Instant::now();
    let runtime = match source {
        ContextSource::Imported {
            context,
            closure_profile,
        } => RuntimeContext::import(context.clone(), closure_profile)?,
        ContextSource::Native {
            typesig_beam,
            contracts_beam,
        } => {
            let mut names = FastHashSet::default();
            for (_, _, _, atoms) in &mut compiled {
                names.extend(atoms.take().expect("native compilation collected atoms"));
            }
            RuntimeContext::native(names, typesig_beam, contracts_beam, Limits::default())?
        }
    };
    let layout =
        NativeAtomLayout::parse(&runtime.context["atom_layout"]).map_err(|e| e.to_string())?;
    let context_seconds = started.elapsed().as_secs_f64();
    let started = Instant::now();
    // Each worker drops its module's compiler arenas instead of one thread
    // dropping all of them afterwards.
    let modules = crate::ordered_parallel::setup_map_owned_by_cost(compiled, &sizes, |(module, forms, view, _)| {
        let _trace = TraceOnDrop("fe_adapt", &module, crate::job_trace::now());
        adapter::module_view(module.clone(), forms, &view, &layout)
            .map_err(|e| format!("{module}: {e}"))
    })
    .into_iter()
    .collect::<Result<Vec<_>, _>>()?;
    let adapter_seconds = started.elapsed().as_secs_f64();
    let mut loaded = complete(runtime, modules, context_seconds, adapter_seconds)?;
    loaded.timings["compile_seconds"] = compile_seconds.into();
    Ok(loaded)
}
