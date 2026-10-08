use crate::{
    behaviours::{self, CallbackEvidence, CallbackFinding},
    bifs::registry::Mfa,
    call_transfer::Signature,
    contract_runtime::ContractSummary,
    form_error::{FormError, ReasonBuilder},
    lattice::{NativeValue, PresentationContext},
    native_form_environment::RawFormTerm,
    warning_payload::{NativeWarning, string},
};
use std::collections::{BTreeMap, BTreeSet};
pub struct CallbackDeclaration<'a> {
    pub name: crate::declaration_key::DeclarationKey,
    pub summary: &'a ContractSummary,
    pub optional: bool,
}
pub struct BehaviourOccurrence<'a> {
    pub name: &'a crate::raw_literal::RawLiteral,
    pub location: &'a RawFormTerm,
    pub callbacks: Option<&'a [CallbackDeclaration<'a>]>,
}
pub struct DeclaredCallback<'a> {
    pub summary: &'a ContractSummary,
    pub file: &'a RawFormTerm,
    pub location: &'a RawFormTerm,
}
pub struct CallbackWarningModule<'a> {
    pub module: &'a str,
    pub filename: &'a RawFormTerm,
    pub behaviours: &'a [BehaviourOccurrence<'a>],
    pub signatures: &'a BTreeMap<Mfa, Signature>,
    pub exported: &'a BTreeSet<Mfa>,
    pub declared: &'a BTreeMap<Mfa, DeclaredCallback<'a>>,
    pub function_origins: &'a BTreeMap<Mfa, RawFormTerm>,
    pub presentation: &'a PresentationContext,
}
fn atom(s: &str) -> NativeValue {
    NativeValue::Atom(s.into())
}
fn int(n: usize) -> NativeValue {
    NativeValue::Integer(n.into())
}
fn module_origin(
    module: &CallbackWarningModule<'_>,
    file: &RawFormTerm,
    location: &RawFormTerm,
) -> RawFormTerm {
    let mut b = ReasonBuilder::new();
    let file = b.copy(file);
    let loc = b.copy(location);
    let m = b.atom(module.module);
    let root = b.tuple(vec![file, loc, m]);
    b.finish(root)
}
fn warning(
    origin: &RawFormTerm,
    category: &str,
    tag: &str,
    behaviour: &crate::raw_literal::RawLiteral,
    args: Vec<NativeValue>,
) -> NativeWarning {
    let mut b = ReasonBuilder::new();
    let mut values = vec![b.copy(behaviour.raw())];
    values.extend(args.iter().map(|value| b.native(value)));
    let values = b.list(values);
    let tag = b.atom(tag);
    let msg = b.tuple(vec![tag, values]);
    NativeWarning::from_raw_origin(category, origin, &b.finish(msg))
}
pub fn collect(module: &CallbackWarningModule<'_>) -> Result<Vec<NativeWarning>, FormError> {
    let mut acc = Vec::new();
    for occurrence in module.behaviours {
        // Original PLT callback lookup accepts atom module identities only.
        if occurrence.name.as_atom().is_none() {
            return Err(FormError::Native(
                crate::native_error::NativeTransferError::FunctionClause,
            ));
        }
        // Source keyfind selects the first declaration location for repeated behaviours.
        let location = module
            .behaviours
            .iter()
            .find(|b| b.name.normal_cmp(occurrence.name).is_eq())
            .expect("current behaviour exists")
            .location;
        let declaration_origin = module_origin(module, module.filename, location);
        let Some(callbacks) = occurrence.callbacks else {
            acc.insert(
                0,
                warning(
                    &declaration_origin,
                    "warn_undefined_callbacks",
                    "callback_info_missing",
                    occurrence.name,
                    vec![],
                ),
            );
            continue;
        };
        for callback in callbacks {
            let crate::core::ErlangTerm::Tuple(fields) = callback.name.raw().term() else {
                unreachable!()
            };
            if !matches!(
                &callback.name.raw().arena().terms[fields[1].0],
                crate::core::ErlangTerm::Atom(_)
            ) {
                return Err(FormError::Native(
                    crate::native_error::NativeTransferError::FunctionClause,
                ));
            }
            let mut target = ReasonBuilder::new();
            let function = target.copy(&RawFormTerm::new(
                callback.name.raw().arena().clone(),
                fields[1],
            )?);
            let arity = target.copy(&RawFormTerm::new(
                callback.name.raw().arena().clone(),
                fields[2],
            )?);
            let root = target.tuple(vec![function, arity]);
            let target =
                crate::declaration_key::DeclarationKey::parse(module.module, &target.finish(root))?;
            let Some(name) = target.callable_mfa() else {
                if !callback.optional {
                    let mut b = ReasonBuilder::new();
                    let behaviour = b.copy(occurrence.name.raw());
                    let function = b.copy(&RawFormTerm::new(
                        callback.name.raw().arena().clone(),
                        fields[1],
                    )?);
                    let arity = b.copy(&RawFormTerm::new(
                        callback.name.raw().arena().clone(),
                        fields[2],
                    )?);
                    let args = b.list(vec![behaviour, function, arity]);
                    let tag = b.atom("callback_missing");
                    let msg = b.tuple(vec![tag, args]);
                    acc.insert(
                        0,
                        NativeWarning::from_raw_origin(
                            "warn_behaviour",
                            &declaration_origin,
                            &b.finish(msg),
                        ),
                    );
                }
                continue;
            };
            let declared = module.declared.get(&name);
            let findings = behaviours::check_callback(
                callback.summary,
                CallbackEvidence {
                    optional: callback.optional,
                    exported: module.exported.contains(&name),
                    inferred: module.signatures.get(&name),
                    declared: declared.map(|d| d.summary),
                },
            )
            .map_err(|e| match e {
                crate::contract_runtime::ContractError::Lattice(e) => FormError::from(e),
                other => FormError::Transport(other.to_string()),
            })?;
            let mut warnings = Vec::new();
            for finding in findings {
                let mut args = vec![atom(&name.function), int(name.arity)];
                let (tag, origin) = match finding {
                    CallbackFinding::Missing => ("callback_missing", &declaration_origin),
                    CallbackFinding::NotExported => (
                        "callback_not_exported",
                        module
                            .function_origins
                            .get(&name)
                            .ok_or("missing function warning origin")?,
                    ),
                    CallbackFinding::Return { actual, expected } => {
                        args.extend([
                            string(&module.presentation.type_to_string(&actual)?),
                            string(&module.presentation.type_to_string(&expected)?),
                        ]);
                        (
                            "callback_type_mismatch",
                            module
                                .function_origins
                                .get(&name)
                                .ok_or("missing function warning origin")?,
                        )
                    }
                    CallbackFinding::Argument {
                        position,
                        actual,
                        expected,
                    } => {
                        args.extend([
                            int(position),
                            string(&module.presentation.type_to_string(&actual)?),
                            string(&module.presentation.type_to_string(&expected)?),
                        ]);
                        (
                            "callback_arg_type_mismatch",
                            module
                                .function_origins
                                .get(&name)
                                .ok_or("missing function warning origin")?,
                        )
                    }
                    CallbackFinding::SpecReturn { extra, expected } => {
                        args.extend([
                            string(&module.presentation.type_to_string(&extra)?),
                            string(&module.presentation.type_to_string(&expected)?),
                        ]);
                        let d = declared.expect("spec finding has declaration");
                        let origin = module_origin(module, d.file, d.location);
                        warnings.push(warning(
                            &origin,
                            "warn_behaviour",
                            "callback_spec_type_mismatch",
                            occurrence.name,
                            args,
                        ));
                        continue;
                    }
                    CallbackFinding::SpecArgument {
                        position,
                        actual,
                        expected,
                    } => {
                        args.extend([
                            int(position),
                            string(&module.presentation.type_to_string(&actual)?),
                            string(&module.presentation.type_to_string(&expected)?),
                        ]);
                        let d = declared.expect("spec finding has declaration");
                        let origin = module_origin(module, d.file, d.location);
                        warnings.push(warning(
                            &origin,
                            "warn_behaviour",
                            "callback_spec_arg_type_mismatch",
                            occurrence.name,
                            args,
                        ));
                        continue;
                    }
                };
                warnings.push(warning(
                    origin,
                    "warn_behaviour",
                    tag,
                    occurrence.name,
                    args,
                ));
            }
            acc.splice(0..0, warnings);
        }
    }
    Ok(acc)
}
