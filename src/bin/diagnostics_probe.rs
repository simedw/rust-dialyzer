use rust_dialyzer_poc::{
    bifs::registry::{Mfa, Registry},
    call_transfer::Signature,
    callback_warnings::{
        self, BehaviourOccurrence, CallbackDeclaration, CallbackWarningModule, DeclaredCallback,
    },
    contract_book::ContractBook,
    contract_runtime::ContractSummary,
    contract_warnings::{self, ContractWarningEntry, ContractWarningModule},
    core::ErlangTerms,
    form_context::FormContext,
    lattice::{PresentationContext, PresentationRecordField, Type},
    native_form_environment::RawFormTerm,
    term_order::compare_map_keys,
    warning_payload::NativeWarning,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, BufRead},
    sync::Arc,
};
fn raw(v: &Value) -> RawFormTerm {
    let a = Arc::new(ErlangTerms::parse(v).unwrap());
    RawFormTerm::new(a.clone(), a.root).unwrap()
}
fn name(v: &Value) -> Mfa {
    Mfa {
        module: v["module"].as_str().unwrap().into(),
        function: v["function"].as_str().unwrap().into(),
        arity: v["arity"].as_u64().unwrap() as usize,
    }
}
fn ty(v: &Value) -> Type {
    Type::from_value(v).unwrap()
}
fn summary(v: &Value) -> ContractSummary {
    ContractSummary::from_parts(
        v["alternatives"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| {
                (
                    ty(&a["signature"]),
                    a["constraints"]
                        .as_array()
                        .map(|xs| xs.iter().map(|x| (ty(&x[0]), ty(&x[1]))).collect())
                        .unwrap_or_default(),
                )
            })
            .collect(),
    )
    .unwrap()
}
struct Entry {
    mfa: Mfa,
    key: rust_dialyzer_poc::declaration_key::DeclarationKey,
    summary: ContractSummary,
    capture: RawFormTerm,
    file: RawFormTerm,
    location: RawFormTerm,
    optional: bool,
}
fn entry(v: &Value) -> Entry {
    Entry {
        mfa: name(v),
        key: {
            let n = name(v);
            rust_dialyzer_poc::declaration_key::DeclarationKey::from_callable(
                &rust_dialyzer_poc::program::FunctionName {
                    module: n.module.to_string(),
                    name: n.function.to_string(),
                    arity: n.arity,
                },
            )
        },
        summary: summary(v),
        capture: raw(&v["capture"]),
        file: raw(&v["file_raw"]),
        location: raw(&v["location_raw"]),
        optional: v["optional"].as_bool().unwrap_or(false),
    }
}
fn context(v: &Value) -> PresentationContext {
    let mut ctx = PresentationContext::default();
    for r in v
        .get("records")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        ctx.insert_record(
            r["tag"].as_str().unwrap().into(),
            r["fields"]
                .as_array()
                .unwrap()
                .iter()
                .map(|f| PresentationRecordField {
                    name: f["name"].as_str().unwrap().into(),
                    declared_type: ty(&f["type"]),
                })
                .collect(),
        );
    }
    ctx
}
fn main() {
    let bifs = Registry::load(include_str!("../../corpus/bifs.json")).unwrap();
    for line in io::stdin().lock().lines() {
        let v: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let ctx = context(&v);
        let signatures = v["signatures"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| {
                (
                    name(s),
                    Signature {
                        arguments: s["arguments"].as_array().unwrap().iter().map(ty).collect(),
                        result: ty(&s["result"]),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        let mut accepted_matched = None;
        let result = if v["kind"] == "contracts" {
            let entries = v["modules"]
                .as_array()
                .unwrap()
                .iter()
                .map(|m| {
                    m["ordered_entries"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(entry)
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            let mut book = ContractBook::new(
                entries
                    .iter()
                    .flatten()
                    .map(|e| (e.mfa.clone(), e.summary.clone()))
                    .collect(),
            );
            let types = signatures
                .iter()
                .map(|(name, sig)| {
                    (
                        name.clone(),
                        Type::function(Some(sig.arguments.clone()), sig.result.clone()),
                    )
                })
                .collect();
            let accepted = book.activate(&types, &bifs).unwrap();
            let expected = raw(&v["accepted_expected"]);
            let mut expected_pairs: Vec<rust_dialyzer_poc::core::TermId> = Vec::new();
            let mut cursor = expected.id();
            loop {
                match &expected.arena().terms[cursor.0] {
                    rust_dialyzer_poc::core::ErlangTerm::Nil => break,
                    rust_dialyzer_poc::core::ErlangTerm::List { items, tail } => {
                        expected_pairs.extend(items.iter().copied());
                        cursor = *tail
                    }
                    _ => panic!("original accepted contracts are proper list"),
                }
            }
            accepted_matched=Some(accepted.len()==expected_pairs.len()&&accepted.iter().zip(expected_pairs).all(|(name,id)|{
             let rust_dialyzer_poc::core::ErlangTerm::Tuple(pair)=&expected.arena().terms[id.0]else{return false};
             let rust_dialyzer_poc::core::ErlangTerm::Tuple(mfa)=&expected.arena().terms[pair[0].0]else{return false};
             let names=matches!(&expected.arena().terms[mfa[0].0],rust_dialyzer_poc::core::ErlangTerm::Atom(m)if m==&*name.module)&&matches!(&expected.arena().terms[mfa[1].0],rust_dialyzer_poc::core::ErlangTerm::Atom(f)if f==&*name.function)&&matches!(&expected.arena().terms[mfa[2].0],rust_dialyzer_poc::core::ErlangTerm::Integer(a)if a==&name.arity.to_string());
             let entry=entries.iter().flatten().find(|e|&e.mfa==name).unwrap();
             names&&compare_map_keys(entry.capture.arena(),entry.capture.id(),expected.arena(),pair[1]).unwrap().is_eq()
            }));
            let borrowed = entries
                .iter()
                .map(|entries| {
                    entries
                        .iter()
                        .map(|e| ContractWarningEntry {
                            name: &e.key,
                            summary: &e.summary,
                            capture: &e.capture,
                            file: &e.file,
                            location: &e.location,
                        })
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            let tables = v["modules"]
                .as_array()
                .unwrap()
                .iter()
                .map(|m| {
                    let t = raw(&m["table_raw"]);
                    (
                        m["name"].as_str().unwrap().into(),
                        rust_dialyzer_poc::resolver::ModuleTableHandle::at(
                            t.arena().clone(),
                            t.id(),
                        )
                        .unwrap(),
                    )
                })
                .collect();
            let forms = FormContext::new(
                tables,
                Arc::new(ErlangTerms::parse(&json!({"root":0,"terms":[{"kind":"nil"}]})).unwrap()),
            );
            let contexts = v["modules"]
                .as_array()
                .unwrap()
                .iter()
                .map(context)
                .collect::<Vec<_>>();
            let modules = borrowed
                .iter()
                .zip(&contexts)
                .map(|(entries, ctx)| ContractWarningModule {
                    ordered_contracts: entries,
                    presentation: ctx,
                    forms: &forms,
                })
                .collect::<Vec<_>>();
            contract_warnings::collect(&modules, &signatures, &bifs)
        } else {
            let callbacks = v["ordered_callbacks"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(b, vs)| {
                    (
                        b,
                        vs.as_array()
                            .map(|es| es.iter().map(entry).collect::<Vec<_>>()),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            let borrowed = callbacks
                .iter()
                .map(|(b, es)| {
                    (
                        *b,
                        es.as_ref().map(|es| {
                            es.iter()
                                .map(|e| CallbackDeclaration {
                                    name: e.key.clone(),
                                    summary: &e.summary,
                                    optional: e.optional,
                                })
                                .collect::<Vec<_>>()
                        }),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            let locations = v["behaviours"]
                .as_array()
                .unwrap()
                .iter()
                .map(|b| raw(&b["location_raw"]))
                .collect::<Vec<_>>();
            let behaviour_names = v["behaviours"]
                .as_array()
                .unwrap()
                .iter()
                .map(|b| {
                    rust_dialyzer_poc::raw_literal::RawLiteral::atom(b["name"].as_str().unwrap())
                })
                .collect::<Vec<_>>();
            let behaviours = v["behaviours"]
                .as_array()
                .unwrap()
                .iter()
                .zip(&locations)
                .zip(&behaviour_names)
                .map(|((b, location), name)| BehaviourOccurrence {
                    name,
                    location,
                    callbacks: borrowed
                        .get(&b["name"].as_str().unwrap().to_owned())
                        .and_then(|v| v.as_deref()),
                })
                .collect::<Vec<_>>();
            let declared = v["declared"]
                .as_array()
                .unwrap()
                .iter()
                .map(entry)
                .collect::<Vec<_>>();
            let declared = declared
                .iter()
                .map(|e| {
                    (
                        e.mfa.clone(),
                        DeclaredCallback {
                            summary: &e.summary,
                            file: &e.file,
                            location: &e.location,
                        },
                    )
                })
                .collect();
            let origins = v["functions"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| (name(e), raw(&e["origin_raw"])))
                .collect();
            let exported = v["exports"]
                .as_array()
                .unwrap()
                .iter()
                .map(name)
                .collect::<BTreeSet<_>>();
            let file = raw(&v["filename_raw"]);
            callback_warnings::collect(&CallbackWarningModule {
                module: v["module"].as_str().unwrap(),
                filename: &file,
                behaviours: &behaviours,
                signatures: &signatures,
                exported: &exported,
                declared: &declared,
                function_origins: &origins,
                presentation: &ctx,
            })
        };
        match result {
            Ok(ws) => {
                let actual = NativeWarning::list(&ws);
                let expected = raw(&v["expected"]);
                let matched =
                    compare_map_keys(actual.arena(), actual.id(), expected.arena(), expected.id())
                        .unwrap()
                        .is_eq();
                println!(
                    "{}",
                    json!({"matched":matched,"accepted_matched":accepted_matched,"warnings":ws.len(),"actual":rust_dialyzer_poc::type_exports::arena_to_value(&actual.arena().terms,actual.id())})
                );
            }
            Err(e) => println!("{}", json!({"error":e.report()})),
        }
    }
}
