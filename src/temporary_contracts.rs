use crate::{
    constraints::closure_order::{ClosureCodeProfile, ClosureOrderKey},
    core::{CoreProgram, ErlangTerm, ErlangTerms, TermId},
    declaration_key::DeclarationKey,
    form_error::{FormError, ReasonBuilder},
    forms::{Contract, Forms},
    native_error::NativeTransferError,
    native_form_environment::RawFormTerm,
    program::{MetadataKind, ModuleId, Program},
    term_order::{CaptureEnvironment, CaptureId, CaptureTerm, CapturedFunction, TermOrderArena},
};
use std::{collections::BTreeMap, sync::Arc};
type Result<T> = std::result::Result<T, FormError>;
struct TemporaryClosure {
    site: &'static str,
    environment: Vec<RawFormTerm>,
}
struct TemporaryContract {
    file_location: RawFormTerm,
    closures: Vec<TemporaryClosure>,
    forms: RawFormTerm,
    flags: RawFormTerm,
}
fn literal(raw: &RawFormTerm, captures: &mut Vec<CaptureTerm>) -> CaptureId {
    CaptureEnvironment::literal_roots(raw.arena(), &[raw.id()])
        .expect("validated literal")
        .append_values_to(captures)[0]
}
fn push(captures: &mut Vec<CaptureTerm>, term: CaptureTerm) -> CaptureId {
    let id = CaptureId(captures.len());
    captures.push(term);
    id
}
impl TemporaryContract {
    fn duplicate_reason(&self, profile: Option<&ClosureCodeProfile>) -> Result<FormError> {
        let mut captures = Vec::new();
        let mut closures = Vec::new();
        for closure in &self.closures {
            let profile = profile.ok_or(FormError::MissingClosureProfile("dialyzer_contracts"))?;
            let ClosureOrderKey::Local(local) = profile.instantiate(closure.site, {
                let mut b = ReasonBuilder::new();
                let roots = closure
                    .environment
                    .iter()
                    .map(|raw| b.copy(raw))
                    .collect::<Vec<_>>();
                let root = b.list(roots.clone());
                let raw = b.finish(root);
                CaptureEnvironment::literal_roots(raw.arena(), &roots)?
            })?
            else {
                unreachable!()
            };
            let environment = local.environment.append_values_to(&mut captures);
            closures.push(push(
                &mut captures,
                CaptureTerm::Function(CapturedFunction::Local {
                    module: local.module,
                    new_index: local.new_index,
                    old_uniq: local.old_uniq,
                    arity: local.arity,
                    name: local.name,
                    environment,
                }),
            ));
        }
        let nil = push(&mut captures, CaptureTerm::Nil);
        let closures = push(
            &mut captures,
            CaptureTerm::List {
                items: closures,
                tail: nil,
            },
        );
        let forms = literal(&self.forms, &mut captures);
        let tag = push(&mut captures, CaptureTerm::Atom("tmp_contract".into()));
        let tmp = push(
            &mut captures,
            CaptureTerm::Tuple(vec![tag, closures, forms]),
        );
        let file_location = literal(&self.file_location, &mut captures);
        let flags = literal(&self.flags, &mut captures);
        let previous = push(
            &mut captures,
            CaptureTerm::Tuple(vec![file_location, tmp, flags]),
        );
        let ok = push(&mut captures, CaptureTerm::Atom("ok".into()));
        let found = push(&mut captures, CaptureTerm::Tuple(vec![ok, previous]));
        let tag = push(&mut captures, CaptureTerm::Atom("try_clause".into()));
        let reason = push(&mut captures, CaptureTerm::Tuple(vec![tag, found]));
        let root = push(
            &mut captures,
            CaptureTerm::List {
                items: vec![reason],
                tail: nil,
            },
        );
        let arena = TermOrderArena::from_captures(&captures)?;
        Ok(FormError::CapturedError(CaptureEnvironment::new(
            Arc::new(arena),
            root,
        )?))
    }
}
fn pair_args(raw: &RawFormTerm, kind: &str) -> Option<Vec<TermId>> {
    let ErlangTerm::Tuple(xs) = raw.term() else {
        return None;
    };
    if xs.len() != 4
        || !matches!(&raw.arena().terms[xs[0].0],ErlangTerm::Atom(s) if s=="type")
        || !matches!(&raw.arena().terms[xs[2].0],ErlangTerm::Atom(s) if s==kind)
    {
        return None;
    }
    let args = RawFormTerm::new(raw.arena().clone(), xs[3]).expect("validated literal child");
    let args = crate::declaration_key::proper_list(&args)?;
    (args.len() == 2).then_some(args)
}
pub(crate) fn locations(
    core: &CoreProgram,
    program: &Program,
) -> Result<BTreeMap<usize, RawFormTerm>> {
    let mut by_module = vec![Vec::new(); core.modules.len()];
    for (index, meta) in program.metadata.iter().enumerate() {
        if let Some(entries) = by_module.get_mut(meta.module.0) {
            entries.push(index);
        }
    }
    let modules = core.modules.iter().zip(&by_module).collect::<Vec<_>>();
    let located = crate::ordered_parallel::setup_map(&modules, |(source, metadata)| {
        let mut output = Vec::with_capacity(metadata.len());
        let arena = Arc::new(ErlangTerms {
            root: TermId(0),
            terms: source.terms.clone(),
        });
        let raw = |id| RawFormTerm::new(arena.clone(), id);
        let mut b = ReasonBuilder::new();
        let file = b.string("nofile");
        let mut file = b.finish(file);
        for &index in metadata.iter() {
            let meta = &program.metadata[index];
            if matches!(&meta.kind,MetadataKind::Other(s) if s=="file") {
                let value = raw(meta.raw)?;
                if let Some(items) = crate::declaration_key::proper_list(&value)
                    && items.len() == 1
                    && let ErlangTerm::Tuple(pair) = &arena.terms[items[0].0]
                    && pair.len() == 2
                {
                    file = raw(pair[0])?;
                }
            }
            let annotations = raw(meta.origin.raw)?;
            let mut location = None;
            for id in crate::declaration_key::proper_list(&annotations)
                .ok_or("attribute annotation list expected")?
            {
                let value = raw(id)?;
                if matches!(value.term(), ErlangTerm::Integer(_))
                    || matches!(value.term(),ErlangTerm::Tuple(xs) if xs.len()==2 && matches!(arena.terms[xs[0].0],ErlangTerm::Integer(_)) && matches!(arena.terms[xs[1].0],ErlangTerm::Integer(_)))
                {
                    location = Some(value);
                    break;
                }
            }
            let mut b = ReasonBuilder::new();
            let file = b.copy(&file);
            let location = location
                .as_ref()
                .map(|v| b.copy(v))
                .unwrap_or_else(|| b.atom("undefined"));
            let root = b.tuple(vec![file, location]);
            output.push((index, b.finish(root)));
        }
        Ok::<_, FormError>(output)
    });
    let mut output = BTreeMap::new();
    for module in located {
        output.extend(module?);
    }
    Ok(output)
}
/// Lazily computed `locations`. Preparation now computes them once up front
/// and shares them with `Forms`; kept for its tests of `locations`.
#[cfg(test)]
pub(crate) struct LocationSnapshot<'a> {
    core: &'a CoreProgram,
    program: &'a Program,
    values: Option<BTreeMap<usize, RawFormTerm>>,
}
#[cfg(test)]
impl<'a> LocationSnapshot<'a> {
    pub(crate) fn new(core: &'a CoreProgram, program: &'a Program) -> Self {
        Self { core, program, values: None }
    }
    fn get(&mut self) -> Result<&BTreeMap<usize, RawFormTerm>> {
        if self.values.is_none() {
            self.values = Some(locations(self.core, self.program)?);
        }
        Ok(self.values.as_ref().expect("initialized locations"))
    }
}
/// A module's contract, read by `prepare_module`; `add` defers its
/// alternatives and records it in `Forms`.
pub(crate) struct PendingContract {
    name: DeclarationKey,
    callback: bool,
    optional: bool,
    alternatives: Vec<TermId>,
    metadata: usize,
    file_location: RawFormTerm,
}
/// Defers each contract's alternatives and appends the contracts, in order.
pub(crate) fn add(forms: &mut Forms, module: ModuleId, pending: Vec<PendingContract>) {
    for contract in pending {
        let alternatives = contract
            .alternatives
            .into_iter()
            .map(|id| forms.defer(module, id))
            .collect();
        forms.contracts.push(Contract {
            name: contract.name,
            callback: contract.callback,
            optional: contract.optional,
            alternatives,
            metadata: contract.metadata,
            file_location: contract.file_location,
        });
    }
}
/// Reads `module`'s specs and callbacks. Only reads shared state, so modules
/// can be prepared concurrently; `add` then applies them in module order.
/// `metadata` lists the module's metadata indices in order; `arena` is the
/// module's terms when the caller already shares a copy.
pub(crate) fn prepare_module(
    core: &CoreProgram,
    program: &Program,
    module: ModuleId,
    metadata: &[usize],
    records: &RawFormTerm,
    locations: &BTreeMap<usize, RawFormTerm>,
    arena: Option<Arc<ErlangTerms>>,
) -> Result<Vec<PendingContract>> {
    let source = &core.modules[module.0];
    let mut pending = Vec::new();
    let arena = arena.unwrap_or_else(|| {
        Arc::new(ErlangTerms {
            root: TermId(0),
            terms: source.terms.clone(),
        })
    });
    let raw = |id| RawFormTerm::new(arena.clone(), id);
    let mut optional = Vec::new();
    for meta in metadata
        .iter()
        .map(|&index| &program.metadata[index])
        .filter(|m| matches!(m.kind, MetadataKind::OptionalCallbacks))
    {
        optional.extend(crate::declaration_key::optional_callbacks(
            &source.name,
            &raw(meta.raw)?,
        )?);
    }
    let mut specs = BTreeMap::<DeclarationKey, TemporaryContract>::new();
    let mut callbacks = BTreeMap::<DeclarationKey, TemporaryContract>::new();
    for (metadata, meta) in metadata
        .iter()
        .map(|&index| (index, &program.metadata[index]))
        .filter(|(_, m)| matches!(m.kind, MetadataKind::Spec | MetadataKind::Callback))
    {
        for (key, alternatives) in
            crate::declaration_key::declarations(&source.name, &raw(meta.raw)?)?
        {
            let active = if matches!(meta.kind, MetadataKind::Callback) {
                &mut callbacks
            } else {
                &mut specs
            };
            if let Some(previous) = active.get(&key) {
                return Err(previous.duplicate_reason(core.contract_closure_profile.as_ref())?);
            }
            let is_optional = optional
                .iter()
                .map(|k| k.optional_membership_equals(&key).map_err(FormError::from))
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .any(|v| v);
            let mut b = ReasonBuilder::new();
            let m = b.atom(&source.name);
            let mfa = b.copy(key.raw());
            let source_location = &locations[&metadata];
            let ErlangTerm::Tuple(pair) = source_location.term() else {
                unreachable!()
            };
            let file = b.copy(&RawFormTerm::new(source_location.arena().clone(), pair[0])?);
            let location = b.copy(&RawFormTerm::new(source_location.arena().clone(), pair[1])?);
            let file_location = b.tuple(vec![file, location]);
            let mut pairs = Vec::new();
            let mut environments = Vec::new();
            for id in &alternatives {
                let form = raw(*id)?;
                let (site, body, constraints) = if pair_args(&form, "fun").is_some() {
                    ("-contract_from_form/7-fun-0-", form.clone(), None)
                } else if let Some(xs) = pair_args(&form, "bounded_fun") {
                    let body = raw(xs[0])?;
                    if pair_args(&body, "fun").is_none() {
                        return Err(FormError::Native(NativeTransferError::FunctionClause));
                    }
                    ("-contract_from_form/7-fun-1-", body, Some(raw(xs[1])?))
                } else {
                    return Err(FormError::Native(NativeTransferError::FunctionClause));
                };
                let body = b.copy(&body);
                let constraints = constraints.as_ref().map(|c| b.copy(c));
                let env = if let Some(constraints) = constraints {
                    vec![m, mfa, body, constraints, file]
                } else {
                    vec![m, mfa, body, location, file]
                };
                environments.push((site, env));
                let constraints = constraints.unwrap_or_else(|| b.list(Vec::new()));
                pairs.push(b.tuple(vec![body, constraints]));
            }
            let pairs = b.list(pairs);
            let mut flags = Vec::new();
            if is_optional {
                flags.push(b.atom("optional_callback"));
            }
            let flags = b.list(flags);
            let root = b.tuple(vec![file_location, pairs, flags]);
            let literals = b.finish(root);
            let at = |id| RawFormTerm::new(literals.arena().clone(), id);
            let closures = environments
                .into_iter()
                .map(|(site, ids)| {
                    Ok(TemporaryClosure {
                        site,
                        environment: {
                            let mut values = ids
                                .into_iter()
                                .map(at)
                                .collect::<std::result::Result<Vec<_>, String>>()?;
                            if site == "-contract_from_form/7-fun-1-" {
                                values.insert(2, records.clone());
                            }
                            values
                        },
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            active.insert(
                key.clone(),
                TemporaryContract {
                    file_location: at(file_location)?,
                    closures,
                    forms: at(pairs)?,
                    flags: at(flags)?,
                },
            );
            pending.push(PendingContract {
                name: key,
                callback: matches!(meta.kind, MetadataKind::Callback),
                optional: is_optional,
                alternatives,
                metadata,
                file_location: locations[&metadata].clone(),
            });
        }
    }
    Ok(pending)
}

#[cfg(test)]
mod location_snapshot_tests {
    use super::*;
    fn fixture() -> (CoreProgram, Program) {
        let mut core = CoreProgram::parse(r#"{"version":5,"profile":{"term_map_order":"native_to_list_with_iterator","otp":"28","erts":"16","elixir":"1.20.4","dialyzer":"5","core_options":[]},"modules":[]}"#).unwrap();
        let mut program = Program::build(&core).unwrap();
        for module in 0..3 {
            core.modules.push(crate::core::CoreModule {
                name: format!("m{module}"), source_forms: TermId(0), core: crate::core::NodeId(0),
                terms: vec![ErlangTerm::Nil, ErlangTerm::Integer((module + 10).to_string()), ErlangTerm::List { items: vec![TermId(1)], tail: TermId(0) }].into(),
                nodes: crate::shared_vec::SharedVec::new(), map_iterator_orders: BTreeMap::new(), contract_variable_maps: BTreeMap::new(),
            });
            program.metadata.push(crate::program::Metadata {
                module: ModuleId(module), kind: MetadataKind::Spec, raw: TermId(0),
                origin: crate::program::SourceOrigin { raw: TermId(2), file: None, line: None, column: None, generated: false },
                types: vec![], functions: vec![], behaviour: None, records: vec![],
            });
        }
        (core, program)
    }
    #[test]
    fn snapshot_preserves_all_location_terms_and_reuses_storage() {
        let (core, program) = fixture();
        let expected = locations(&core, &program).unwrap();
        let mut snapshot = LocationSnapshot::new(&core, &program);
        assert!(snapshot.values.is_none());
        let address = snapshot.get().unwrap() as *const _;
        let arenas = snapshot.get().unwrap().iter().map(|(k,v)| (*k, v.arena().clone())).collect::<BTreeMap<_,_>>();
        for _ in [2, 0, 1, 2] {
            let actual = snapshot.get().unwrap();
            assert_eq!(address, actual as *const _);
            assert_eq!(actual.len(), expected.len());
            for (key, value) in actual {
                assert_eq!(value.id(), expected[key].id());
                assert_eq!(value.arena().terms, expected[key].arena().terms);
                assert!(Arc::ptr_eq(value.arena(), &arenas[key]));
            }
        }
    }
    #[test]
    fn malformed_future_module_retains_first_global_error() {
        let (core, mut program) = fixture();
        program.metadata[2].origin.raw = TermId(1);
        let expected = locations(&core, &program).unwrap_err().to_string();
        let mut snapshot = LocationSnapshot::new(&core, &program);
        assert_eq!(snapshot.get().unwrap_err().to_string(), expected);
        assert!(snapshot.values.is_none());
    }
    #[test]
    fn empty_snapshot_remains_lazy_and_computes_empty_when_requested() {
        let (mut core, mut program) = fixture();
        core.modules.clear(); program.metadata.clear();
        let mut snapshot = LocationSnapshot::new(&core, &program);
        assert!(snapshot.values.is_none());
        assert!(snapshot.get().unwrap().is_empty());
    }
    #[test]
    #[ignore = "actual retained Core population diagnostic; run explicitly"]
    fn actual_location_cost_diagnostic() {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("reports/real-projects/fixes/full-elixir-warning-parity/local-resolution");
        let mut wire: serde_json::Value = serde_json::from_slice(&std::fs::read(base.join("runtime-context.json")).unwrap()).unwrap();
        let names = ["Elixir.String", "Elixir.Range", "Elixir.HashDict", "elixir_errors"];
        wire["modules"] = serde_json::Value::Array(names.iter().map(|name| serde_json::from_slice(&std::fs::read(base.join(format!("{name}-preclean.json"))).unwrap()).unwrap()).collect());
        let core = CoreProgram::from_value(&wire).unwrap();
        let program = Program::build(&core).unwrap();
        let expected = locations(&core, &program).unwrap();
        let mut old_us = Vec::new();
        let mut new_us = Vec::new();
        for _ in 0..12 {
            let start = std::time::Instant::now();
            for _ in 0..core.modules.len() { std::hint::black_box(locations(&core, &program).unwrap()); }
            old_us.push(start.elapsed().as_micros());
            let start = std::time::Instant::now();
            let mut snapshot = LocationSnapshot::new(&core, &program);
            for _ in 0..core.modules.len() { std::hint::black_box(snapshot.get().unwrap()); }
            new_us.push(start.elapsed().as_micros());
            for (key, value) in snapshot.get().unwrap() {
                assert_eq!(value.id(), expected[key].id());
                assert_eq!(value.arena().terms, expected[key].arena().terms);
            }
        }
        println!("{}", serde_json::json!({"modules":core.modules.len(), "terms":core.term_count, "locations":expected.len(),"legacy_global_builds_per_round":core.modules.len(),"snapshot_global_builds_per_round":1,"legacy_us":old_us,"snapshot_us":new_us,"all_raw_terms_equal":true,"scope":"locations only; unchanged Forms call excluded; no project benchmark claim"}));
    }

}

