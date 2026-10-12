use rust_dialyzer_poc::{
    beam_frontend::{Compilation, adapter},
    core::etf::module_from_etf,
    external_term::{Limits, decode},
    native_runtime_hash::NativeAtomLayout,
};
use std::{fs, path::Path};
#[test]
fn direct_native_arenas_match_independent_transport() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/beam_core_port");
    for name in [
        "Elixir.NativeSlice",
        "Elixir.Walkthrough.Numbers",
        "Elixir.Walkthrough.Checkout",
        "Elixir.MapFun",
        "native_calls",
        "native_guards",
        "core_lower_binary",
        "core_lower_nifs",
        "core_lower_implicit_nifs",
        "core_lower_try",
    ] {
        let read = |suffix: &str| fs::read(root.join(format!("{name}.{suffix}"))).unwrap();
        let layout =
            NativeAtomLayout::parse(&serde_json::from_slice(&read("layout.json")).unwrap())
                .unwrap();
        let actual = adapter::module_direct(
            Compilation {
                module: name.into(),
                source_forms: decode(&read("forms.etf"), Limits::default()).unwrap(),
                cleaned_core: decode(&read("clean_core.etf"), Limits::default()).unwrap(),
            },
            &layout,
        )
        .unwrap();
        let expected = module_from_etf(&read("native.etf")).unwrap();
        assert_eq!(actual.name, expected.name);
        assert_eq!(actual.source_forms, expected.source_forms);
        assert_eq!(actual.core, expected.core, "{name}: root");
        assert_eq!(
            actual.terms.len(),
            expected.terms.len(),
            "{name}: term count"
        );
        for (index, (a, b)) in actual.terms.iter().zip(expected.terms.iter()).enumerate() {
            assert_eq!(a, b, "{name}: term{index}");
        }
        assert_eq!(
            actual.nodes.len(),
            expected.nodes.len(),
            "{name}: node count"
        );
        for (index, (a, b)) in actual.nodes.iter().zip(expected.nodes.iter()).enumerate() {
            assert_eq!(a, b, "{name}: node{index}");
        }
        assert_eq!(
            actual.map_iterator_orders, expected.map_iterator_orders,
            "{name}: map provenance"
        );
        assert_eq!(
            actual.contract_variable_maps, expected.contract_variable_maps,
            "{name}: contract variables"
        );
    }
}

#[test]
fn direct_emission_matches_reference_across_all_clean_core_fixtures() {
    use rust_dialyzer_poc::{core::ErlangTerm, native_runtime_hash::NativeAtomLayout};
    use std::collections::BTreeSet;
    fn collect(dir: &Path, files: &mut Vec<std::path::PathBuf>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                collect(&path, files);
            } else if path.to_string_lossy().ends_with(".clean_core.etf") {
                files.push(path);
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/beam_core_port");
    let mut files = vec![];
    collect(&root, &mut files);
    files.sort();
    let mut kinds = BTreeSet::new();
    for path in &files {
        let base = path
            .to_string_lossy()
            .trim_end_matches(".clean_core.etf")
            .to_owned();
        let forms_path = format!("{base}.forms.etf");
        if !Path::new(&forms_path).exists() {
            continue;
        }
        let forms = decode(&fs::read(forms_path).unwrap(), Limits::default()).unwrap();
        let clean = decode(&fs::read(path).unwrap(), Limits::default()).unwrap();
        let mut atoms = BTreeSet::from(["nil".to_owned()]);
        for arena in [&forms, &clean] {
            for term in &arena.terms {
                match term {
                    ErlangTerm::Atom(name) => {
                        atoms.insert(name.clone());
                    }
                    ErlangTerm::ExternalFun {
                        module, function, ..
                    } => {
                        atoms.insert(module.clone());
                        atoms.insert(function.clone());
                    }
                    _ => {}
                }
            }
        }
        let layout =
            NativeAtomLayout::deterministic_native("native-atoms-v1:direct-fixtures".into(), atoms)
                .unwrap();
        let name = path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .trim_end_matches(".clean_core.etf")
            .to_owned();
        let make = || Compilation {
            module: name.clone(),
            source_forms: forms.clone(),
            cleaned_core: clean.clone(),
        };
        let reference = adapter::module(make(), &layout).unwrap();
        let direct = adapter::module_direct(make(), &layout).unwrap();
        assert_eq!(
            direct,
            reference,
            "{}: entire compact module",
            path.display()
        );
        kinds.extend(direct.nodes.iter().map(|node| node.expr.kind()));
    }
    assert!(files.len() >= 20, "fixture inventory unexpectedly small");
    for kind in [
        "module", "literal", "var", "binary", "bitstr", "map", "map_pair", "fun", "letrec", "try",
        "catch", "case", "clause", "apply", "call", "primop",
    ] {
        assert!(
            kinds.contains(kind),
            "missing real compiler fixture coverage for {kind}"
        );
    }
}

mod corpus {
    use rust_dialyzer_poc::{
        beam_frontend::{Compilation, adapter, debug, forms},
        core::{CoreModule, etf::module_from_etf},
        external_term::{Limits, decode_interned},
        native_runtime_hash::NativeAtomLayout,
    };
    use serde_json::{Value, json};
    use std::fs;
    fn compare(a: &CoreModule, b: &CoreModule) -> Result<(), String> {
        if a.name != b.name || a.core != b.core || a.source_forms != b.source_forms {
            return Err("module roots differ".into());
        }
        if a.terms.len() != b.terms.len() {
            return Err(format!(
                "term counts {} != {}",
                a.terms.len(),
                b.terms.len()
            ));
        }
        for (i, (a, b)) in a.terms.iter().zip(b.terms.iter()).enumerate() {
            if a != b {
                return Err(format!("term{i}: {a:?} != {b:?}"));
            }
        }
        if a.nodes.len() != b.nodes.len() {
            return Err(format!(
                "node counts {} != {}",
                a.nodes.len(),
                b.nodes.len()
            ));
        }
        for (i, (a, b)) in a.nodes.iter().zip(b.nodes.iter()).enumerate() {
            if a != b {
                return Err(format!("node{i}: {a:?} != {b:?}"));
            }
        }
        if a.map_iterator_orders != b.map_iterator_orders {
            return Err("map iterator orders differ".into());
        }
        if a.contract_variable_maps != b.contract_variable_maps {
            return Err("contract variable maps differ".into());
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires independently captured corpus compiler oracles and manifest"]
    fn entire_corpus_direct_adapter_matches_compiler_transport() {
        let plan: Value = serde_json::from_slice(
            &fs::read(std::env::var("BEAM_CORE_ADAPTER_PLAN").unwrap()).unwrap(),
        )
        .unwrap();
        let report_path = std::env::var("BEAM_CORE_ADAPTER_REPORT").unwrap();
        let layouts = plan["layouts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| NativeAtomLayout::parse(v).unwrap())
            .collect::<Vec<_>>();
        let rows = rust_dialyzer_poc::ordered_parallel::setup_map(
            plan["modules"].as_array().unwrap(),
            |row| {
                let run = || -> Result<(), String> {
                    let read =
                        |key: &str| fs::read(row[key].as_str().unwrap()).map_err(|e| e.to_string());
                    let debug = debug::read_beam(&read("beam")?, Limits::default())?;
                    let actual = adapter::module_direct(
                        Compilation {
                            module: debug.module.clone(),
                            source_forms: forms(&debug)?,
                            cleaned_core: decode_interned(
                                &read("cleaned_core")?,
                                Limits {
                                    bytes: 256 * 1024 * 1024,
                                    nodes: 100_000_000,
                                    expanded: 100_000_000,
                                },
                            )
                            .map_err(|e| format!("oracle Core: {e}"))?,
                        },
                        &layouts[row["layout"].as_u64().unwrap() as usize],
                    )?;
                    let expected = module_from_etf(&read("native_core")?)
                        .map_err(|e| format!("expected transport: {e}"))?;
                    compare(&actual, &expected)
                };
                match run() {
                    Ok(()) => json!({"beam":row["beam"],"equal":true}),
                    Err(e) => json!({"beam":row["beam"],"equal":false,"error":e}),
                }
            },
        );
        let failures = rows.iter().filter(|r| r["equal"] != true).count();
        fs::write(report_path, serde_json::to_vec_pretty(&rows).unwrap()).unwrap();
        assert_eq!(
            failures,
            0,
            "{failures}/{} adapter modules differ; see report",
            rows.len()
        );
    }
}
