use rust_dialyzer_poc::{
    classic_checked::{
        self, Change, CheckError, CheckOutcome, CheckedHeader, FingerprintTable, RepairPlan,
    },
    core::{ErlangTerm, ErlangTerms, TermId},
    module_dependencies::ModuleDependencies,
    native_error::NativeTransferError,
    native_form_environment::RawFormTerm,
    native_runtime_hash::NativeAtomLayout,
    raw_literal::{RawLiteral, RawLiteralArena},
    term_order::compare_map_keys,
    type_exports,
};
use std::{
    collections::BTreeSet,
    io::{self, BufRead},
    sync::Arc,
};
struct ProbeTerms(Vec<ErlangTerm>);
impl ProbeTerms {
    fn new() -> Self {
        Self(vec![ErlangTerm::Nil])
    }
    fn push(&mut self, t: ErlangTerm) -> TermId {
        let id = TermId(self.0.len());
        self.0.push(t);
        id
    }
    fn atom(&mut self, s: &str) -> TermId {
        self.push(ErlangTerm::Atom(s.into()))
    }
    fn tuple(&mut self, x: Vec<TermId>) -> TermId {
        self.push(ErlangTerm::Tuple(x))
    }
    fn list(&mut self, x: Vec<TermId>) -> TermId {
        if x.is_empty() {
            TermId(0)
        } else {
            self.push(ErlangTerm::List {
                items: x,
                tail: TermId(0),
            })
        }
    }
    fn string(&mut self, s: &str) -> TermId {
        let x = s
            .chars()
            .map(|c| self.push(ErlangTerm::Integer((c as u32).to_string())))
            .collect();
        self.list(x)
    }
    fn copy(&mut self, r: &RawFormTerm) -> TermId {
        let offset = self.0.len();
        let add = |id: TermId| TermId(id.0 + offset);
        for t in &r.arena().terms {
            self.0.push(match t {
                ErlangTerm::Tuple(x) => ErlangTerm::Tuple(x.iter().copied().map(add).collect()),
                ErlangTerm::List { items, tail } => ErlangTerm::List {
                    items: items.iter().copied().map(add).collect(),
                    tail: add(*tail),
                },
                ErlangTerm::Map(x) => {
                    ErlangTerm::Map(x.iter().map(|(k, v)| (add(*k), add(*v))).collect())
                }
                t => t.clone(),
            });
        }
        add(r.id())
    }
    fn finish(self, root: TermId) -> RawFormTerm {
        RawFormTerm::new(
            Arc::new(ErlangTerms {
                terms: self.0,
                root,
            }),
            root,
        )
        .unwrap()
    }
}
fn raw(v: &serde_json::Value) -> RawFormTerm {
    let a = Arc::new(ErlangTerms::parse(v).unwrap());
    RawFormTerm::new(a.clone(), a.root).unwrap()
}
fn literal(v: &serde_json::Value) -> RawLiteral {
    RawLiteral::new(raw(v)).unwrap()
}
fn list(v: &serde_json::Value) -> Vec<RawLiteral> {
    let a = Arc::new(ErlangTerms::parse(v).unwrap());
    let l = RawLiteralArena::new(a.clone()).unwrap();
    let mut id = a.root;
    let mut out = Vec::new();
    loop {
        match &a.terms[id.0] {
            ErlangTerm::Nil => return out,
            ErlangTerm::List { items, tail } => {
                out.extend(items.iter().map(|&id| l.at(id).unwrap()));
                id = *tail;
            }
            _ => panic!("proof list"),
        }
    }
}
fn list_raw(b: &mut ProbeTerms, values: &[RawLiteral]) -> rust_dialyzer_poc::core::TermId {
    let xs = values.iter().map(|v| b.copy(v.raw())).collect();
    b.list(xs)
}
fn changes(b: &mut ProbeTerms, values: &[Change]) -> rust_dialyzer_poc::core::TermId {
    let xs = values
        .iter()
        .map(|v| {
            let tag = b.atom(if matches!(v, Change::Differ(_)) {
                "differ"
            } else {
                "removed"
            });
            let module = b.copy(v.module().raw());
            b.tuple(vec![tag, module])
        })
        .collect();
    b.list(xs)
}
fn set(b: &mut ProbeTerms, values: &BTreeSet<RawLiteral>) -> rust_dialyzer_poc::core::TermId {
    let yes = b.atom("true");
    let xs = values.iter().map(|v| (b.copy(v.raw()), yes)).collect();
    b.push(ErlangTerm::Map(xs))
}
fn error(e: CheckError, public: bool) -> RawFormTerm {
    let e = match e {
        CheckError::Dependency(
            rust_dialyzer_poc::module_dependencies::DependencyError::Native(e),
        ) => CheckError::Native(e),
        e => e,
    };
    let mut b = ProbeTerms::new();
    let id = match e {
        CheckError::Dependency(
            rust_dialyzer_poc::module_dependencies::DependencyError::BadRecord(tail),
        ) => {
            let tag = b.atom("badrecord");
            let tail = b.copy(tail.raw());
            let reason = b.tuple(vec![tag, tail]);
            let tag = b.atom("error");
            b.tuple(vec![tag, reason])
        }
        CheckError::Dependency(
            rust_dialyzer_poc::module_dependencies::DependencyError::BadGenerator(tail),
        ) => {
            let tag = b.atom("bad_generator");
            let tail = b.copy(tail.raw());
            let reason = b.tuple(vec![tag, tail]);
            let tag = b.atom("error");
            b.tuple(vec![tag, reason])
        }
        CheckError::NoFileToRemove(file) => {
            let tag = b.atom("no_file_to_remove");
            let file = b.copy(file.raw());
            let payload = b.tuple(vec![tag, file]);
            let tag = b.atom("error");
            b.tuple(vec![tag, payload])
        }
        CheckError::DialyzerError(message) => {
            let tag = b.atom("dialyzer_error");
            let msg = b.string(&message);
            let payload = b.tuple(vec![tag, msg]);
            let throw = b.atom("throw");
            b.tuple(vec![throw, payload])
        }
        CheckError::Native(NativeTransferError::BadMatchBoolean(value)) => {
            let tag = b.atom("badmatch");
            let value = b.atom(if value { "true" } else { "false" });
            let reason = b.tuple(vec![tag, value]);
            if public {
                reason
            } else {
                let tag = b.atom("error");
                b.tuple(vec![tag, reason])
            }
        }
        CheckError::Native(e) => {
            let reason = b.atom(match e {
                NativeTransferError::Badarg => "badarg",
                NativeTransferError::FunctionClause => "function_clause",
                NativeTransferError::SystemLimit => "system_limit",
                _ => panic!("unhandled {e:?}"),
            });
            if public {
                reason
            } else {
                let tag = b.atom("error");
                b.tuple(vec![tag, reason])
            }
        }
        e => panic!("unexpected boundary {e:?}"),
    };
    b.finish(id)
}
fn run(
    r: &serde_json::Value,
    without_layout: bool,
) -> (Result<RawFormTerm, CheckError>, Vec<RawLiteral>) {
    let mut fingerprint_calls = Vec::new();
    let public = r["op"] == "check";
    let result = (|| -> Result<RawFormTerm, CheckError> {
        if r["op"] == "path" {
            return Ok(classic_checked::path_to_mod(&literal(&r["file"]))?
                .raw()
                .clone());
        }
        if r["op"] == "union" {
            let make = |value: &serde_json::Value| {
                let mut b = ProbeTerms::new();
                let key = b.atom("key");
                let value = b.copy(&raw(value));
                let root = b.push(ErlangTerm::Map(vec![(key, value)]));
                ModuleDependencies::from_raw_map(&b.finish(root)).unwrap()
            };
            let result = make(&r["left"])
                .union(&make(&r["right"]))
                .map_err(CheckError::Dependency)?;
            return Ok(result.get(&RawLiteral::atom("key")).unwrap().raw().clone());
        }
        if r["op"] == "union_maps" {
            let mut deps =
                ModuleDependencies::from_raw_dict(&raw(&r["dependencies_dict"])).unwrap();
            if r["filter_first"] == true {
                let seeds = list(&r["seeds"])
                    .into_iter()
                    .collect::<std::collections::BTreeSet<_>>();
                deps = deps
                    .retaining_values(|v| !seeds.contains(v))
                    .map_err(CheckError::Dependency)?;
            }
            let incoming = ModuleDependencies::from_raw_dict(&raw(&r["incoming_dict"])).unwrap();
            let mut result = deps.union(&incoming).map_err(CheckError::Dependency)?;
            if r["filter_after"] == true {
                let seeds = list(&r["seeds"])
                    .into_iter()
                    .collect::<std::collections::BTreeSet<_>>();
                result = result
                    .retaining_values(|v| !seeds.contains(v))
                    .map_err(CheckError::Dependency)?;
            }
            return Ok(result.to_raw());
        }
        if r["op"] == "strip" {
            let deps = if let Some(input) = r.get("dependencies_dict") {
                ModuleDependencies::from_raw_dict(&raw(input))
            } else {
                ModuleDependencies::from_raw_map(&raw(&r["dependencies"]))
                    .map_err(rust_dialyzer_poc::module_dependencies::DependencyError::from)
            }
            .unwrap();
            let seeds = list(&r["seeds"])
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>();
            return Ok(deps
                .retaining_values(|value| !seeds.contains(value))
                .map_err(CheckError::Dependency)?
                .to_raw());
        }
        let files = raw(&r["md5"]);
        let dependencies = if let Some(dict) = r.get("dependencies_dict") {
            raw(dict)
        } else {
            ModuleDependencies::from_raw_map(&raw(&r["dependencies"]))
                .unwrap()
                .to_raw_dict()
        };
        let mut b = ProbeTerms::new();
        let id = if public {
            let version = literal(&r["version"]);
            let current = literal(&r["current_version"]);
            let implementation = raw(&r["implementation"]);
            let layout = NativeAtomLayout::parse(&r["atom_layout"]).unwrap();
            let out = classic_checked::check(
                CheckedHeader {
                    version: &version,
                    files: &files,
                    implementation: &implementation,
                    dependencies: &dependencies,
                },
                &current,
                &list(&r["remove"]),
                &list(&r["add"]),
                if without_layout { None } else { Some(&layout) },
                |f| {
                    fingerprint_calls.push(f.clone());
                    classic_checked::fingerprint_file(f, 128 * 1024 * 1024)
                },
            )?;
            let delta = match &out {
                CheckOutcome::Differ { changes, .. } => changes.clone(),
                _ => Vec::new(),
            };
            match out.into_plan()? {
                RepairPlan::Reuse { .. } => b.atom("ok"),
                RepairPlan::Rebuild {
                    files,
                    analyze_files,
                } => {
                    assert_eq!(
                        analyze_files,
                        files
                            .rows()
                            .iter()
                            .map(|r| r.file.clone())
                            .collect::<Vec<_>>()
                    );
                    let tag = b.atom("old_version");
                    let files = b.copy(&files.to_raw());
                    b.tuple(vec![tag, files])
                }
                RepairPlan::Repair {
                    files,
                    analyze_files,
                    purge_modules,
                    dependencies,
                } => {
                    let tag = b.atom("differ");
                    let files = b.copy(&files.to_raw());
                    let diff = changes(&mut b, &delta);
                    let analyze = list_raw(&mut b, &analyze_files);
                    let purge = set(&mut b, &purge_modules);
                    let deps = b.copy(&dependencies.to_raw());
                    b.tuple(vec![tag, files, diff, analyze, purge, deps])
                }
            }
        } else {
            let delta = list(&r["diff"])
                .into_iter()
                .map(|v| {
                    let ErlangTerm::Tuple(fields) = v.raw().term() else {
                        panic!()
                    };
                    let module = v.at(fields[1]).unwrap();
                    match &v.raw().arena().terms[fields[0].0] {
                        ErlangTerm::Atom(t) if t == "differ" => Change::Differ(module),
                        ErlangTerm::Atom(t) if t == "removed" => Change::Removed(module),
                        _ => panic!(),
                    }
                })
                .collect::<Vec<_>>();
            let (analyze, purge, repaired) = classic_checked::expand(
                &FingerprintTable::from_raw(&files)?,
                &delta,
                &ModuleDependencies::from_raw_dict(&dependencies).unwrap(),
            )?;
            let analyze = list_raw(&mut b, &analyze);
            let purge = set(&mut b, &purge);
            let deps = b.copy(&repaired.to_raw());
            b.tuple(vec![analyze, purge, deps])
        };
        Ok(b.finish(id))
    })();
    (result, fingerprint_calls)
}
fn main() {
    let without_layout = std::env::args().any(|arg| arg == "--without-layout");
    for line in io::stdin().lock().lines() {
        let r: serde_json::Value = serde_json::from_str(&line.unwrap()).unwrap();
        let (result, calls) = run(&r, without_layout);
        let a = match result {
            Ok(a) => a,
            Err(CheckError::RuntimeOrder(error)) => {
                println!(
                    "{}",
                    serde_json::json!({"id":r["id"],"boundary":format!("{error:?}")})
                );
                continue;
            }
            Err(e) => error(
                e,
                r["op"] == "check" && r["expected"].get("value").is_some(),
            ),
        };
        let expected = raw(r["expected"]
            .get("value")
            .or_else(|| r["expected"].get("error"))
            .unwrap());
        let mut expected_id = expected.id();
        let mut projected = false;
        if r["op"] == "check"
            && let ErlangTerm::Tuple(xs) = expected.term()
            && xs.len() == 2
            && matches!(&expected.arena().terms[xs[1].0], ErlangTerm::List { .. })
            && matches!(&expected.arena().terms[xs[0].0],ErlangTerm::Atom(tag) if ["badarg","function_clause","system_limit"].contains(&tag.as_str()))
        {
            expected_id = xs[0];
            projected = true;
        }
        let exact = compare_map_keys(a.arena(), a.id(), expected.arena(), expected_id)
            .unwrap()
            .is_eq();
        let calls_exact = r.get("fingerprint_calls").map(|value| {
            let expected = raw(value);
            let mut b = ProbeTerms::new();
            let root = list_raw(&mut b, &calls);
            let actual = b.finish(root);
            compare_map_keys(actual.arena(), actual.id(), expected.arena(), expected.id())
                .unwrap()
                .is_eq()
        });
        println!(
            "{}",
            serde_json::json!({"id":r["id"],"exact":exact,"public_exit_reason_projection":projected,"fingerprint_calls_exact":calls_exact,"actual":type_exports::arena_to_value(&a.arena().terms,a.id())})
        );
    }
}
