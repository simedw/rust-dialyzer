use rust_dialyzer_poc::{
    beam_frontend::{self, Compilation, adapter, clean_view::CleanView},
    core::{ErlangTerm, ErlangTerms},
    external_term::{Limits, decode},
    native_runtime_hash::NativeAtomLayout,
};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

fn compare(name: &str, forms: ErlangTerms, raw: ErlangTerms, layout: &NativeAtomLayout) {
    let cleaned = beam_frontend::clean::clean(&raw).unwrap();
    let view = CleanView::new(raw).unwrap();
    let reference = adapter::module(
        Compilation {
            module: name.to_owned(),
            source_forms: forms.clone(),
            cleaned_core: cleaned,
        },
        layout,
    )
    .unwrap();
    let fused = adapter::module_view(name.to_owned(), forms, &view, layout).unwrap();
    assert_eq!(
        fused, reference,
        "{name}: complete compact module, IDs, map order and contracts"
    );
}
#[test]
fn fused_emission_matches_reference_in_imported_atom_scopes() {
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
        compare(
            name,
            decode(&read("forms.etf"), Limits::default()).unwrap(),
            decode(&read("raw_core.etf"), Limits::default()).unwrap(),
            &layout,
        );
    }
}
#[test]
fn fused_emission_matches_reference_across_all_raw_core_fixtures() {
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
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/beam_core_port");
    let mut files = Vec::new();
    collect(&root, &mut files);
    files.sort();
    let mut tested = 0;
    for path in files {
        let base = path
            .to_string_lossy()
            .trim_end_matches(".raw_core.etf")
            .to_owned();
        let forms_path = format!("{base}.forms.etf");
        if !Path::new(&forms_path).exists() {
            continue;
        }
        let forms = decode(&fs::read(forms_path).unwrap(), Limits::default()).unwrap();
        let raw = decode(&fs::read(&path).unwrap(), Limits::default()).unwrap();
        let view = CleanView::new(raw.clone()).unwrap();
        let mut atoms = view.atoms().unwrap().into_iter().collect::<BTreeSet<_>>();
        atoms.insert("nil".to_owned());
        for term in &forms.terms {
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
        let layout =
            NativeAtomLayout::deterministic_native("native-atoms-v1:fused-fixtures".into(), atoms)
                .unwrap();
        let name = path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .trim_end_matches(".raw_core.etf")
            .to_owned();
        compare(&name, forms, raw, &layout);
        tested += 1;
    }
    assert!(
        tested >= 30,
        "expected broad raw Core fixture coverage, found {tested}"
    );
}
