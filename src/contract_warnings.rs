use crate::{
    bifs::registry::{Mfa, Registry},
    call_transfer::Signature,
    contract_presentation,
    contract_runtime::ContractSummary,
    core::{ErlangTerm, TermId},
    form_context::{FormContext, FormOperation},
    form_error::{FormError, ReasonBuilder},
    lattice::{NativeValue, PresentationContext, Type, TypeView},
    native_form_environment::{NativeFormSite, NativeSiteKind, RawFormTerm},
    resolver::FormCache,
    validation::{self, RangeWarning, Validation},
    warning_payload::{NativeWarning, string},
};
use std::collections::BTreeMap;

pub struct ContractWarningEntry<'a> {
    pub name: &'a crate::declaration_key::DeclarationKey,
    pub summary: &'a ContractSummary,
    pub capture: &'a RawFormTerm,
    pub file: &'a RawFormTerm,
    pub location: &'a RawFormTerm,
}
pub struct ContractWarningModule<'a> {
    pub ordered_contracts: &'a [ContractWarningEntry<'a>],
    pub presentation: &'a PresentationContext,
    pub forms: &'a FormContext,
}
fn atom(v: &str) -> NativeValue {
    NativeValue::Atom(v.into())
}
fn int(v: usize) -> NativeValue {
    NativeValue::Integer(v.into())
}
fn origin(entry: &ContractWarningEntry<'_>) -> RawFormTerm {
    let mut b = ReasonBuilder::new();
    let file = b.copy(entry.file);
    let loc = b.copy(entry.location);
    let name = b.copy(entry.name.raw());
    let root = b.tuple(vec![file, loc, name]);
    b.finish(root)
}
fn warn(
    entry: &ContractWarningEntry<'_>,
    category: &str,
    tag: &str,
    tail: Vec<NativeValue>,
) -> NativeWarning {
    let mut b = ReasonBuilder::new();
    let ErlangTerm::Tuple(fields) = entry.name.raw().term() else {
        unreachable!()
    };
    let mut args = fields
        .iter()
        .map(|id| {
            b.copy(
                &RawFormTerm::new(entry.name.raw().arena().clone(), *id)
                    .expect("declaration key field"),
            )
        })
        .collect::<Vec<_>>();
    args.extend(tail.iter().map(|v| b.native(v)));
    let args = b.list(args);
    let tag = b.atom(tag);
    let msg = b.tuple(vec![tag, args]);
    NativeWarning::from_raw_origin(category, &origin(entry), &b.finish(msg))
}
fn signature(value: &Type, ctx: &PresentationContext) -> Result<String, FormError> {
    let s = ctx.type_to_string(value)?;
    s.strip_prefix("fun(")
        .and_then(|s| s.strip_suffix(')'))
        .map(str::to_owned)
        .ok_or_else(|| "invalid function presentation".into())
}
fn parts(value: &Type) -> Result<(Vec<Type>, Type), FormError> {
    if let TypeView::Families(s) = value.view()
        && let Some(f) = s.functions
        && let Some(args) = f.domain
    {
        Ok((args.to_vec(), f.range.clone()))
    } else {
        Err("invalid contract function".into())
    }
}
fn subtype(a: &Type, b: &Type) -> Result<bool, FormError> {
    Ok(*a == a.meet(b)?)
}
fn proper(raw: &RawFormTerm, id: TermId) -> Result<Vec<TermId>, FormError> {
    let mut ids = Vec::new();
    let mut id = id;
    loop {
        match &raw.arena().terms[id.0] {
            ErlangTerm::Nil => return Ok(ids),
            ErlangTerm::List { items, tail } => {
                ids.extend(items);
                id = *tail
            }
            _ => return Err("contract forms must be a proper list".into()),
        }
    }
}
fn no_remote(
    entry: &ContractWarningEntry<'_>,
    ctx: &FormContext,
) -> Result<Option<Type>, FormError> {
    let ErlangTerm::Tuple(fields) = entry.capture.term() else {
        return Err("contract record".into());
    };
    let forms = proper(entry.capture, fields[3])?;
    if forms.len() != 1 {
        return Ok(None);
    }
    let ErlangTerm::Tuple(pair) = &entry.capture.arena().terms[forms[0].0] else {
        return Err("contract form pair".into());
    };
    if !proper(entry.capture, pair[1])?.is_empty() {
        return Ok(None);
    }
    let form = RawFormTerm::new(entry.capture.arena().clone(), pair[0])?;
    let site = NativeFormSite {
        kind: NativeSiteKind::Spec,
        scope: crate::native_form_environment::NativeFormScope::Contract(entry.name.clone()),
        file: entry.file.clone(),
    };
    let result = ctx
        .execute(
            &site,
            &form,
            &FormCache::default(),
            FormOperation::WithoutRemote,
        )
        .map_err(|e| e.error)?;
    Ok(Some(
        result
            .value
            .expect("WithoutRemote returns a type")
            .substitute(&BTreeMap::new())?,
    ))
}
fn invalid(
    entry: &ContractWarningEntry<'_>,
    sig: &Type,
    details: NativeValue,
    ctx: &PresentationContext,
) -> Result<NativeWarning, FormError> {
    Ok(warn(
        entry,
        "warn_contract_types",
        "invalid_contract",
        vec![
            details,
            string(&contract_presentation::contract_to_string(entry.capture)?),
            string(&signature(sig, ctx)?),
        ],
    ))
}
fn picky(
    entry: &ContractWarningEntry<'_>,
    contract: &Type,
    sig: &Type,
    module: &ContractWarningModule<'_>,
) -> Result<Option<NativeWarning>, FormError> {
    if contract.abstract_records(module.presentation)?
        == sig.abstract_records(module.presentation)?
    {
        return Ok(None);
    }
    let (_, sr) = parts(sig)?;
    let (_, cr) = parts(contract)?;
    if sr.is_none() && cr.is_unit() {
        return Ok(None);
    }
    let tag = if subtype(contract, sig)? {
        "contract_subtype"
    } else if subtype(sig, contract)? {
        if let Some(nr) = no_remote(entry, module.forms)? {
            let (ca, cr) = parts(contract)?;
            let (na, nr) = parts(&nr)?;
            let (sa, sr) = parts(sig)?;
            let mut blame = true;
            for ((c, n), s) in std::iter::once(cr)
                .chain(ca)
                .zip(std::iter::once(nr).chain(na))
                .zip(std::iter::once(sr).chain(sa))
            {
                if c == n {
                    if c != s {
                        blame = false;
                        break;
                    }
                } else if subtype(&s, &n)? && !subtype(&n, &s)? {
                    blame = false;
                    break;
                }
            }
            if blame {
                return Ok(None);
            }
        }
        "contract_supertype"
    } else {
        "contract_diff"
    };
    let category = match tag {
        "contract_subtype" => "warn_contract_subtype",
        "contract_supertype" => "warn_contract_supertype",
        _ => "warn_contract_not_equal",
    };
    Ok(Some(warn(
        entry,
        category,
        tag,
        vec![
            string(&contract_presentation::contract_to_string(entry.capture)?),
            string(&signature(sig, module.presentation)?),
        ],
    )))
}
fn collect_entry(
    module: &ContractWarningModule<'_>,
    entry: &ContractWarningEntry<'_>,
    signatures: &BTreeMap<Mfa, Signature>,
    bifs: &Registry,
    acc: &mut Vec<NativeWarning>,
) -> Result<(), FormError> {
        let ErlangTerm::Tuple(fields) = entry.name.raw().term() else {
            unreachable!()
        };
        if !matches!(
            &entry.name.raw().arena().terms[fields[0].0],
            ErlangTerm::Atom(_)
        ) || !matches!(
            &entry.name.raw().arena().terms[fields[1].0],
            ErlangTerm::Atom(_)
        ) {
            return Err(FormError::Native(
                crate::native_error::NativeTransferError::FunctionClause,
            ));
        }
        let Some(name) = entry.name.callable_mfa() else {
            return Ok(());
        };
        let Some(success) = signatures.get(&name) else {
            return Ok(());
        };
        let sig = Type::function(Some(success.arguments.clone()), success.result.clone());
        match validation::validate(entry.summary, &sig, &name.module)? {
            Validation::Overlapping => acc.push(warn(
                entry,
                "warn_overlapping_contract",
                "overlapping_contract",
                vec![],
            )),
            Validation::Invalid {
                arguments,
                return_invalid,
            } => {
                let details = if let Some(args) = arguments {
                    NativeValue::Tuple(vec![
                        NativeValue::List(args.into_iter().map(int).collect()),
                        atom(if return_invalid == Some(true) {
                            "true"
                        } else {
                            "false"
                        }),
                    ])
                } else {
                    atom("none")
                };
                acc.push(invalid(entry, &sig, details, module.presentation)?);
            }
            Validation::OpaqueMismatch { range } => acc.push(warn(
                entry,
                "warn_contract_opaque",
                "contract_with_opaque",
                vec![
                    string(&PresentationContext::default().type_to_string(&range)?),
                    string(&signature(&sig, module.presentation)?),
                ],
            )),
            Validation::RangeWarnings { warnings } => {
                for finding in warnings {
                    match finding {
                        RangeWarning::Extra { extra, success } => {
                            if let Some(nr) = no_remote(entry, module.forms)?
                                && !subtype(&extra, &parts(&nr)?.1)?
                            {
                                continue;
                            }
                            acc.push(warn(
                                entry,
                                "warn_contract_extra_return",
                                "extra_range",
                                vec![
                                    string(
                                        &PresentationContext::default()
                                            .type_to_string(&extra)?,
                                    ),
                                    string(
                                        &PresentationContext::default()
                                            .type_to_string(&success)?,
                                    ),
                                ],
                            ));
                        }
                        RangeWarning::Missing { missing, contract } => acc.push(warn(
                            entry,
                            "warn_contract_missing_return",
                            "missing_range",
                            vec![
                                string(
                                    &PresentationContext::default().type_to_string(&missing)?,
                                ),
                                string(
                                    &PresentationContext::default()
                                        .type_to_string(&contract)?,
                                ),
                            ],
                        )),
                    }
                }
            }
            Validation::Valid => {
                let contract = Type::function(
                    Some(entry.summary.arguments.clone()),
                    entry
                        .summary
                        .return_for(&entry.summary.arguments)
                        .map_err(|e| FormError::Transport(e.to_string()))?,
                )
                .substitute(&BTreeMap::new())?;
                let mut check = sig;
                if !(name.module == "erlang" && name.function == "raise" && name.arity == 3)
                    && bifs.is_known(&name)
                {
                    let args = bifs
                        .argument_domains(&name)
                        .map_err(|e| FormError::Transport(e.to_string()))?;
                    let ret = bifs
                        .transfer(&name, &vec![Type::any(); name.arity])
                        .map_err(|e| FormError::Transport(e.to_string()))?
                        .result;
                    check = Type::function(Some(args), ret);
                    if !matches!(
                        validation::validate(entry.summary, &check, &name.module)?,
                        Validation::Valid | Validation::RangeWarnings { .. }
                    ) {
                        acc.push(invalid(entry, &check, atom("none"), module.presentation)?);
                        return Ok(());
                    }
                }
                if let Some(w) = picky(entry, &contract, &check, module)? {
                    acc.push(w)
                }
            }
        }
    Ok(())
}

pub fn collect(
    modules: &[ContractWarningModule<'_>],
    signatures: &BTreeMap<Mfa, Signature>,
    bifs: &Registry,
) -> Result<Vec<NativeWarning>, FormError> {
    let mut acc = Vec::new();
    for module in modules {
        for entry in module.ordered_contracts {
            collect_entry(module, entry, signatures, bifs, &mut acc).map_err(|error| {
                FormError::Transport(format!(
                    "contract {:?}: {error}",
                    entry.name.callable_mfa()
                ))
            })?;
        }
    }
    acc.reverse();
    Ok(acc)
}

pub fn without_functions(
    entries: &[ContractWarningEntry<'_>],
    implemented: &[Mfa],
) -> Vec<NativeWarning> {
    let mut missing = entries.iter().collect::<Vec<_>>();
    for name in implemented {
        if let Some(index) = missing
            .iter()
            .position(|entry| entry.name.callable_mfa().as_ref() == Some(name))
        {
            missing.remove(index);
        }
    }
    missing
        .into_iter()
        .map(|entry| {
            warn(
                entry,
                "warn_contract_syntax",
                "spec_missing_fun",
                Vec::new(),
            )
        })
        .collect()
}
