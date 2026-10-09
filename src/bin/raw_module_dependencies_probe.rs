use rust_dialyzer_poc::{
    core::{ErlangTerm, ErlangTerms, TermId},
    module_dependencies::ModuleDependencies,
    raw_literal::RawLiteralArena,
    term_order::compare_map_keys,
    type_exports,
};
use std::{
    io::{self, BufRead},
    sync::Arc,
};
fn list(a: &ErlangTerms, mut id: TermId) -> Vec<TermId> {
    let mut out = Vec::new();
    loop {
        match &a.terms[id.0] {
            ErlangTerm::Nil => return out,
            ErlangTerm::List { items, tail } => {
                out.extend(items);
                id = *tail;
            }
            _ => panic!("proof recipe requires proper list"),
        }
    }
}
fn forms(source: &Arc<ErlangTerms>, deps: TermId) -> Arc<ErlangTerms> {
    let mut terms = source.terms.clone();
    fn push(terms: &mut rust_dialyzer_poc::shared_vec::SharedVec<ErlangTerm>, term: ErlangTerm) -> TermId {
        let id = TermId(terms.len());
        terms.push(term);
        id
    }
    let atom = push(&mut terms, ErlangTerm::Atom("atom".into()));
    let zero = push(&mut terms, ErlangTerm::Integer("0".into()));
    let t = push(&mut terms, ErlangTerm::Atom("t".into()));
    let tag = push(&mut terms, ErlangTerm::Atom("remote_type".into()));
    let nil = push(&mut terms, ErlangTerm::Nil);
    let name = push(&mut terms, ErlangTerm::Tuple(vec![atom, zero, t]));
    let mut forms = Vec::new();
    for value in list(source, deps) {
        let module = push(&mut terms, ErlangTerm::Tuple(vec![atom, zero, value]));
        let args = push(
            &mut terms,
            ErlangTerm::List {
                items: vec![module, name, nil],
                tail: nil,
            },
        );
        forms.push(push(&mut terms, ErlangTerm::Tuple(vec![tag, zero, args])));
    }
    let root = push(
        &mut terms,
        ErlangTerm::List {
            items: forms,
            tail: nil,
        },
    );
    Arc::new(ErlangTerms { terms, root })
}
fn behaviour_result(
    r: &serde_json::Value,
) -> rust_dialyzer_poc::native_form_environment::RawFormTerm {
    use rust_dialyzer_poc::{
        compiled_definitions::PreparedDefinitions,
        core::CoreProgram,
        dependencies::{ImportPlan, PriorPlt},
        form_error::FormError,
        module_publication::{PublicationError, PublicationInput},
        native_error::NativeTransferError,
        native_form_environment::RawFormTerm,
        program::{ModuleId, Program},
    };
    let core = CoreProgram::parse(&r["core"].to_string()).unwrap();
    let program = Program::build(&core).unwrap();
    if r["id"].as_str().unwrap().starts_with("opaque_attribute/") {
        assert!(
            program.type_dependencies.is_empty(),
            "opaque attributes created type edges"
        );
    }
    let library = PriorPlt::parse(
        &std::fs::read_to_string(r["environment"].as_str().unwrap_or(
            "/private/tmp/rust-dialyzer-module-publication/data/empty_environment.json",
        ))
        .unwrap(),
        &core,
        &ImportPlan::cold_targets(&core),
    )
    .unwrap()
    .into_cold_store()
    .unwrap();
    let definitions = PreparedDefinitions::prepare(&core, &program, &library).unwrap();
    match (PublicationInput {
        core: &core,
        program: &program,
        definitions: &definitions,
        targets: &[ModuleId(0)],
        use_contracts: true,
    })
    .type_dependencies()
    {
        Ok(value) => value.to_raw(),
        Err(error) => {
            let reason = match error {
                PublicationError::Form(FormError::ErrorReason(reason)) => reason,
                PublicationError::Native(NativeTransferError::Badarg) => {
                    let a = Arc::new(ErlangTerms {
                        terms: vec![ErlangTerm::Atom("badarg".into())].into(),
                        root: TermId(0),
                    });
                    RawFormTerm::new(a, TermId(0)).unwrap()
                }
                other => panic!("unexpected publication boundary {other:?}"),
            };
            let mut terms = reason.arena().terms.clone();
            let error = TermId(terms.len());
            terms.push(ErlangTerm::Atom("error".into()));
            let root = TermId(terms.len());
            terms.push(ErlangTerm::Tuple(vec![error, reason.id()]));
            RawFormTerm::new(Arc::new(ErlangTerms { terms, root }), root).unwrap()
        }
    }
}
fn behaviour_warnings(
    r: &serde_json::Value,
) -> rust_dialyzer_poc::native_form_environment::RawFormTerm {
    use rust_dialyzer_poc::{
        callback_warnings::{self, BehaviourOccurrence, CallbackWarningModule},
        core::CoreProgram,
        form_error::FormError,
        lattice::PresentationContext,
        native_error::NativeTransferError,
        native_form_environment::RawFormTerm,
        program::Program,
        warning_payload::NativeWarning,
    };
    let core = CoreProgram::parse(&r["core"].to_string()).unwrap();
    let program = Program::build(&core).unwrap();
    let result = rust_dialyzer_poc::behaviour_metadata::collect(
        program
            .metadata
            .iter()
            .filter_map(|metadata| metadata.behaviour.as_ref()),
    )
    .and_then(|occurrences| {
        let behaviours = occurrences
            .iter()
            .map(|occurrence| BehaviourOccurrence {
                name: &occurrence.name,
                location: occurrence.location.raw(),
                callbacks: None,
            })
            .collect::<Vec<_>>();
        let arena = Arc::new(ErlangTerms::parse(&r["filename"]).unwrap());
        let filename = RawFormTerm::new(arena.clone(), arena.root).unwrap();
        callback_warnings::collect(&CallbackWarningModule {
            module: &core.modules[0].name,
            filename: &filename,
            behaviours: &behaviours,
            signatures: &Default::default(),
            exported: &Default::default(),
            declared: &Default::default(),
            function_origins: &Default::default(),
            presentation: &PresentationContext::default(),
        })
    });
    match result {
        Ok(warnings) => NativeWarning::list(&warnings),
        Err(error) => {
            let reason = match error {
                FormError::ErrorReason(reason) => reason,
                FormError::Native(NativeTransferError::FunctionClause) => RawFormTerm::new(
                    Arc::new(ErlangTerms {
                        terms: vec![ErlangTerm::Atom("function_clause".into())].into(),
                        root: TermId(0),
                    }),
                    TermId(0),
                )
                .unwrap(),
                FormError::Native(NativeTransferError::Badarg) => RawFormTerm::new(
                    Arc::new(ErlangTerms {
                        terms: vec![ErlangTerm::Atom("badarg".into())].into(),
                        root: TermId(0),
                    }),
                    TermId(0),
                )
                .unwrap(),
                other => panic!("unexpected source warning error {other:?}"),
            };
            let mut terms = reason.arena().terms.clone();
            let error = TermId(terms.len());
            terms.push(ErlangTerm::Atom("error".into()));
            let root = TermId(terms.len());
            terms.push(ErlangTerm::Tuple(vec![error, reason.id()]));
            RawFormTerm::new(Arc::new(ErlangTerms { terms, root }), root).unwrap()
        }
    }
}
fn main() {
    for line in io::stdin().lock().lines() {
        let r: serde_json::Value = serde_json::from_str(&line.unwrap()).unwrap();
        if r.get("op").is_some() {
            let arena = Arc::new(ErlangTerms::parse(&r["form"]).unwrap());
            let values = if r["op"] == "form" {
                type_exports::type_form_to_remote_modules(&arena, arena.root)
            } else {
                type_exports::constraint_form_to_remote_modules(&arena, arena.root)
            };
            if let Err(error) = &values {
                assert_eq!(*error, type_exports::ExportError::FunctionClause);
                let actual = ErlangTerms {
                    terms: vec![
                        ErlangTerm::Atom("error".into()),
                        ErlangTerm::Atom("function_clause".into()),
                        ErlangTerm::Tuple(vec![TermId(0), TermId(1)]),
                    ].into(),
                    root: TermId(2),
                };
                let expected = ErlangTerms::parse(&r["expected"]["error"]).unwrap();
                let exact = compare_map_keys(&actual, actual.root, &expected, expected.root)
                    .unwrap()
                    .is_eq();
                println!("{}", serde_json::json!({"id":r["id"],"exact":exact}));
                continue;
            }
            let values = values.unwrap();
            let expected = ErlangTerms::parse(&r["expected"]["value"]).unwrap();
            let ids = list(&expected, expected.root);
            let exact = values.len() == ids.len()
                && values.iter().zip(ids).all(|(v, id)| {
                    compare_map_keys(v.raw().arena(), v.raw().id(), &expected, id)
                        .unwrap()
                        .is_eq()
                });
            println!("{}", serde_json::json!({"id":r["id"],"exact":exact}));
            continue;
        }
        let actual = if r.get("filename").is_some() {
            behaviour_warnings(&r)
        } else if r.get("core").is_some() {
            behaviour_result(&r)
        } else {
            let a = Arc::new(ErlangTerms::parse(&r["source"]).unwrap());
            let literals = RawLiteralArena::new(a.clone()).unwrap();
            let mut rows = Vec::new();
            for id in list(&a, a.root) {
                let ErlangTerm::Tuple(fields) = &a.terms[id.0] else {
                    panic!()
                };
                let owner = literals.at(fields[0]).unwrap();
                let deps = if r["kind"] == "behaviour" {
                    list(&a, fields[1])
                        .into_iter()
                        .map(|id| literals.at(id).unwrap())
                        .collect()
                } else {
                    let forms = forms(&a, fields[1]);
                    type_exports::type_form_to_remote_modules(&forms, forms.root).unwrap()
                };
                rows.push((owner, deps));
            }
            if r["use_contracts"] == false && r["kind"] == "spec" {
                rows.clear();
            }
            ModuleDependencies::collect_type_rows(rows).to_raw()
        };
        let expected = ErlangTerms::parse(
            r["expected"]
                .get("value")
                .or_else(|| r["expected"].get("error"))
                .unwrap(),
        )
        .unwrap();
        let exact = compare_map_keys(actual.arena(), actual.id(), &expected, expected.root)
            .unwrap()
            .is_eq();
        println!(
            "{}",
            serde_json::json!({"id":r["id"],"exact":exact,"actual":type_exports::arena_to_value(&actual.arena().terms,actual.id())})
        );
    }
}
