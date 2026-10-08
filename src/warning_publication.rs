use crate::{
    compiled_definitions::PreparedDefinitions,
    core::{ErlangTerm, ErlangTerms, TermId},
    diagnostic_inputs::prepared_origin,
    form_error::ReasonBuilder,
    native_error::NativeTransferError,
    native_form_environment::RawFormTerm,
    program::FunctionName,
    warning_collection::{self, ContractOrigin, ContractOrigins, PostprocessError},
    warning_payload::NativeWarning,
    warning_policy::Policy,
    warning_render::{self, LocationOption, RenderError},
    warnings::Tag,
};
use std::sync::Arc;

#[derive(Debug)]
pub enum PublicationError {
    Native(NativeTransferError),
    Postprocess(PostprocessError),
    Order(RenderError),
    Boundary(String),
}

pub struct BackendWarnings {
    pub collected: Vec<NativeWarning>,
}

pub struct PublishedWarnings {
    pub raw: Vec<RawFormTerm>,
    pub backend: BackendWarnings,
}

pub struct UnknownWarnings {
    pub by_module: Vec<(RawFormTerm, NativeWarning)>,
}

fn sort_unknown(
    warnings: Vec<(RawFormTerm, NativeWarning)>,
) -> Result<Vec<(RawFormTerm, NativeWarning)>, PublicationError> {
    let mut sorted: Vec<(RawFormTerm, (RawFormTerm, NativeWarning))> = Vec::new();
    for (module, warning) in warnings {
        let mut builder = ReasonBuilder::new();
        let m = builder.copy(&module);
        let w = builder.copy(&warning.raw);
        let root = builder.tuple(vec![m, w]);
        let pair = builder.finish(root);
        let mut at = sorted.len();
        while at > 0
            && crate::term_order::compare_terms(
                pair.arena(),
                pair.id(),
                sorted[at - 1].0.arena(),
                sorted[at - 1].0.id(),
            )
            .map_err(PublicationError::Boundary)?
            .is_lt()
        {
            at -= 1;
        }
        sorted.insert(at, (pair, (module, warning)));
    }
    Ok(sorted.into_iter().map(|(_, warning)| warning).collect())
}

pub fn collect_unknown(
    calls: Vec<(String, NativeWarning)>,
    types: &[crate::native_form_environment::ExternalTypeEvent],
    legal: &crate::warnings::Profile,
) -> Result<UnknownWarnings, PublicationError> {
    if !legal.contains(Tag::WarnUnknown) {
        return Ok(UnknownWarnings {
            by_module: Vec::new(),
        });
    }
    let calls = calls
        .into_iter()
        .map(|(module, warning)| {
            let mut builder = ReasonBuilder::new();
            let root = builder.atom(&module);
            (builder.finish(root), warning)
        })
        .collect();
    let mut by_module = sort_unknown(calls)?;
    let mut type_warnings = Vec::new();
    for event in types {
        let raw = event.to_arena();
        let [target, info] = fields::<2>(raw.arena(), raw.id())?;
        let [_, _, origin] = fields::<3>(raw.arena(), info)?;
        let [module, _, _] = fields::<3>(raw.arena(), origin)?;
        let mut builder = ReasonBuilder::new();
        let tag = builder.atom("unknown_type");
        let target = builder.copy(
            &RawFormTerm::new(raw.arena().clone(), target).map_err(PublicationError::Boundary)?,
        );
        let root = builder.tuple(vec![tag, target]);
        let warning = NativeWarning::from_raw_origin(
            "warn_unknown",
            &RawFormTerm::new(raw.arena().clone(), info).map_err(PublicationError::Boundary)?,
            &builder.finish(root),
        );
        type_warnings.push((
            RawFormTerm::new(raw.arena().clone(), module).map_err(PublicationError::Boundary)?,
            warning,
        ));
    }
    by_module.extend(sort_unknown(type_warnings)?);
    Ok(UnknownWarnings { by_module })
}

fn fields<const N: usize>(
    arena: &ErlangTerms,
    id: TermId,
) -> Result<[TermId; N], PublicationError> {
    let Some(ErlangTerm::Tuple(fields)) = arena.terms.get(id.0) else {
        return Err(PublicationError::Native(
            NativeTransferError::FunctionClause,
        ));
    };
    fields
        .as_slice()
        .try_into()
        .map_err(|_| PublicationError::Native(NativeTransferError::FunctionClause))
}
fn allowed(arena: &ErlangTerms, root: TermId, policy: &Policy) -> Result<bool, PublicationError> {
    let [tag, info, _] = fields::<3>(arena, root)?;
    let [_, _, origin] = fields::<3>(arena, info)?;
    let mut key = origin;
    let (module_name, function) = loop {
        match &arena.terms[key.0] {
            ErlangTerm::Atom(module) => break (module.as_str(), None),
            ErlangTerm::Tuple(_) => {
                let [module, function, arity] = fields::<3>(arena, key)?;
                let ErlangTerm::Atom(module_name) = &arena.terms[module.0] else {
                    key = module;
                    continue;
                };
                let function = match (&arena.terms[function.0], &arena.terms[arity.0]) {
                    (ErlangTerm::Atom(function), ErlangTerm::Integer(arity)) => {
                        arity.parse().ok().map(|arity| FunctionName {
                            module: module_name.clone(),
                            name: function.clone(),
                            arity,
                        })
                    }
                    _ => None,
                };
                break (module_name.as_str(), function);
            }
            _ => {
                return Err(PublicationError::Native(
                    NativeTransferError::FunctionClause,
                ));
            }
        }
    };
    let Some(module) = policy.modules.iter().find(|m| m.module == module_name) else {
        return Ok(true);
    };
    if function.as_ref().is_some_and(|function| {
        module
            .functions
            .iter()
            .any(|f| &f.function == function && f.suppressed)
    }) {
        return Ok(false);
    }
    let ErlangTerm::Atom(tag) = &arena.terms[tag.0] else {
        return Ok(false);
    };
    let Ok(tag) = serde_json::from_value::<Tag>(serde_json::Value::String(tag.clone())) else {
        return Ok(false);
    };
    Ok(function.map_or_else(
        || module.profile.contains(tag),
        |function| policy.allows(&function, tag),
    ))
}

pub fn publish(
    analysis: &[NativeWarning],
    preceding_filtered: &[NativeWarning],
    unknown: &[NativeWarning],
    definitions: &PreparedDefinitions,
    policy: &Policy,
    location: LocationOption,
) -> Result<PublishedWarnings, PublicationError> {
    let mut builder = ReasonBuilder::new();
    let warnings = analysis
        .iter()
        .map(|w| builder.copy(&w.raw))
        .collect::<Vec<_>>();
    let unknown = unknown
        .iter()
        .map(|w| builder.copy(&w.raw))
        .collect::<Vec<_>>();
    let preceding_filtered = preceding_filtered
        .iter()
        .map(|warning| builder.copy(&warning.raw))
        .collect::<Vec<_>>();
    let mut origins = ContractOrigins::new();
    for (mfa, contract) in definitions.contracts.callables() {
        let origin = prepared_origin(contract);
        origins.insert(
            FunctionName {
                module: mfa.module.clone(),
                name: mfa.function.clone(),
                arity: mfa.arity,
            },
            ContractOrigin {
                file: builder.copy(&origin.file),
                location: builder.copy(&origin.location),
            },
        );
    }
    let root = builder.list(Vec::new());
    let mut arena = Arc::unwrap_or_clone(builder.finish(root).arena);
    let warnings = warning_collection::postprocess(&mut arena, &warnings, &origins)
        .map_err(PublicationError::Postprocess)?;
    let mut retained = Vec::with_capacity(warnings.len() + unknown.len());
    for warning in warnings {
        if allowed(&arena, warning, policy)? {
            retained.push(warning);
        }
    }
    let mut backend = preceding_filtered.clone();
    backend.extend(retained.iter().copied());
    retained.extend(unknown);
    retained.extend(preceding_filtered);
    let retained =
        warning_render::process_warnings(&arena, &retained).map_err(PublicationError::Order)?;
    let mut output = Vec::with_capacity(retained.len());
    for warning in retained {
        let [tag, info, message] = fields::<3>(&arena, warning)?;
        let [file, position, _] = fields::<3>(&arena, info)?;
        let position = if location == LocationOption::Line {
            match &arena.terms[position.0] {
                ErlangTerm::Tuple(xs) if xs.len() == 2 => xs[0],
                _ => position,
            }
        } else {
            position
        };
        let info = TermId(arena.terms.len());
        arena.terms.push(ErlangTerm::Tuple(vec![file, position]));
        let warning = TermId(arena.terms.len());
        arena
            .terms
            .push(ErlangTerm::Tuple(vec![tag, info, message]));
        output.push(warning);
    }
    let arena = Arc::new(arena);
    Ok(PublishedWarnings {
        backend: BackendWarnings {
            collected: backend
                .into_iter()
                .map(|id| {
                    RawFormTerm::new(arena.clone(), id)
                        .map(|raw| NativeWarning { raw })
                        .map_err(PublicationError::Boundary)
                })
                .collect::<Result<_, _>>()?,
        },
        raw: output
            .into_iter()
            .map(|id| RawFormTerm::new(arena.clone(), id).map_err(PublicationError::Boundary))
            .collect::<Result<_, _>>()?,
    })
}
