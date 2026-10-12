use rust_dialyzer_poc::{
    beam_frontend::{Compilation, adapter, clean_view::CleanView},
    core::{ErlangTerm, ErlangTerms, TermId, etf::module_from_etf},
    external_term::{Limits, decode},
    native_runtime_hash::NativeAtomLayout,
};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

fn atoms(arena: &ErlangTerms) -> BTreeSet<String> {
    let mut result = BTreeSet::new();
    let mut visited = vec![false; arena.terms.len()];
    let mut pending = vec![arena.root];
    while let Some(TermId(id)) = pending.pop() {
        if std::mem::replace(&mut visited[id], true) {
            continue;
        }
        match &arena.terms[id] {
            ErlangTerm::Atom(name) => {
                result.insert(name.clone());
            }
            ErlangTerm::ExternalFun {
                module, function, ..
            } => {
                result.insert(module.clone());
                result.insert(function.clone());
            }
            ErlangTerm::Tuple(children) => pending.extend(children.iter().copied()),
            ErlangTerm::List { items, tail } => {
                pending.extend(items.iter().copied());
                pending.push(*tail);
            }
            ErlangTerm::Map(entries) => {
                pending.extend(entries.iter().flat_map(|&(key, value)| [key, value]));
            }
            _ => {}
        }
    }
    result
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/beam_core_port")
}
fn read_terms(path: impl AsRef<Path>) -> ErlangTerms {
    decode(&fs::read(path).unwrap(), Limits::default()).unwrap()
}

#[test]
fn fused_emission_matches_independent_compiler_transport() {
    let root = fixture_root();
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
        let path = |suffix: &str| root.join(format!("{name}.{suffix}"));
        let layout = NativeAtomLayout::parse(
            &serde_json::from_slice(&fs::read(path("layout.json")).unwrap()).unwrap(),
        )
        .unwrap();
        let view = CleanView::new(read_terms(path("raw_core.etf"))).unwrap();
        let actual =
            adapter::module_view(name.into(), read_terms(path("forms.etf")), &view, &layout)
                .unwrap();
        let expected = module_from_etf(&fs::read(path("native.etf")).unwrap()).unwrap();
        assert_eq!(
            actual, expected,
            "{name}: full compact module including IDs and provenance"
        );
    }
}

#[test]
fn borrowed_cleanup_matches_all_captured_clean_core_modules_and_atom_sets() {
    fn collect(dir: &Path, files: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                collect(&path, files);
            } else if path.to_string_lossy().ends_with(".raw_core.etf") {
                files.push(path);
            }
        }
    }
    let mut files = Vec::new();
    collect(&fixture_root(), &mut files);
    files.sort();
    let mut checked = 0;
    let mut kinds = BTreeSet::new();
    for raw_path in files {
        let base = raw_path
            .to_string_lossy()
            .trim_end_matches(".raw_core.etf")
            .to_owned();
        let clean_path = format!("{base}.clean_core.etf");
        let forms_path = format!("{base}.forms.etf");
        if !Path::new(&clean_path).exists() || !Path::new(&forms_path).exists() {
            continue;
        }
        let forms = read_terms(forms_path);
        let clean = read_terms(clean_path);
        let view = CleanView::new(read_terms(&raw_path)).unwrap();
        let view_atoms: BTreeSet<_> = view.atoms().unwrap().into_iter().collect();
        assert_eq!(
            view_atoms,
            atoms(&clean),
            "{}: exact cleaned namespace",
            raw_path.display()
        );
        let mut names = atoms(&forms);
        names.extend(view_atoms);
        names.insert("nil".into());
        let layout =
            NativeAtomLayout::deterministic_native("native-atoms-v1:fused-fixtures".into(), names)
                .unwrap();
        let name = raw_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .trim_end_matches(".raw_core.etf")
            .to_owned();
        let reference = adapter::module(
            Compilation {
                module: name.clone(),
                source_forms: forms.clone(),
                cleaned_core: clean,
            },
            &layout,
        )
        .unwrap();
        let actual = adapter::module_view(name, forms, &view, &layout).unwrap();
        assert_eq!(
            actual,
            reference,
            "{}: all terms, nodes, IDs and maps",
            raw_path.display()
        );
        kinds.extend(actual.nodes.iter().map(|node| node.expr.kind()));
        checked += 1;
    }
    assert!(
        checked >= 20,
        "fixture inventory unexpectedly small: {checked}"
    );
    for kind in [
        "module", "literal", "var", "binary", "bitstr", "map", "map_pair", "fun", "letrec", "try",
        "catch", "case", "clause", "apply", "call", "primop",
    ] {
        assert!(
            kinds.contains(kind),
            "missing compiler fixture coverage for {kind}"
        );
    }
}
