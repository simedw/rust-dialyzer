use rust_dialyzer_poc::{beam_frontend, core::{CoreVersion,ErlangTerm}, external_term, native_runtime_hash::NativeAtomLayout, term_order};
use std::{cmp::Ordering, collections::BTreeSet, path::PathBuf};

#[test]
fn otp29_complete_raw_and_clean_core_match_the_compiler() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/otp29");
    for module in ["native_record_dependency29", "native_records29", "native_comprehensions29"] {
        let read = |suffix| std::fs::read(root.join(format!("{module}.{suffix}"))).unwrap();
        let compiled = beam_frontend::compile_beam_raw(&read("beam")).unwrap_or_else(|error| panic!("{module}: {error}"));
        let clean = beam_frontend::clean::clean(&compiled.raw_core).unwrap();
        for (suffix, actual) in [("raw_core.etf", &compiled.raw_core), ("clean_core.etf", &clean)] {
            let expected = external_term::decode(&read(suffix), external_term::Limits::default()).unwrap();
            assert_eq!(term_order::compare_map_keys(actual, actual.root, &expected, expected.root).unwrap(), Ordering::Equal, "{module} {suffix}");
        }
    }
}

#[test]
fn otp29_reference_direct_and_fused_adapters_agree_on_complete_core() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/otp29");
    for module in ["native_record_dependency29", "native_records29", "native_comprehensions29"] {
        let read = |suffix| std::fs::read(root.join(format!("{module}.{suffix}"))).unwrap();
        let source = beam_frontend::compile_beam_raw(&read("beam")).unwrap().source_forms;
        let raw = external_term::decode(&read("raw_core.etf"), external_term::Limits::default()).unwrap();
        let clean = external_term::decode(&read("clean_core.etf"), external_term::Limits::default()).unwrap();
        let mut atoms = BTreeSet::from(["nil".to_owned()]);
        for arena in [&source, &raw, &clean] {
            for term in &arena.terms {
                match term {
                    ErlangTerm::Atom(atom) => { atoms.insert(atom.clone()); }
                    ErlangTerm::ExternalFun(fun) => { atoms.insert(fun.module.clone()); atoms.insert(fun.function.clone()); }
                    ErlangTerm::Record(record) => { atoms.insert(record.module.clone()); atoms.insert(record.name.clone()); atoms.extend(record.fields.iter().map(|(name, _)| name.clone())); }
                    _ => (),
                }
            }
        }
        let layout = NativeAtomLayout::deterministic_native("native-atoms-v1:otp29-fixture".into(), atoms).unwrap();
        let make = || beam_frontend::Compilation { module: module.into(), source_forms: source.clone(), cleaned_core: clean.clone() };
        let reference = beam_frontend::adapter::module(make(), &layout).unwrap();
        let direct = beam_frontend::adapter::module_direct(make(), &layout).unwrap();
        let view = beam_frontend::clean_view::CleanView::new(raw).unwrap();
        let fused = beam_frontend::adapter::module_view(module.into(), source, &view, &layout).unwrap();
        for candidate in [&reference, &direct, &fused] { candidate.validate(module, CoreVersion::V6).unwrap(); }
        assert_eq!(reference.to_wire_value(), direct.to_wire_value(), "{module}: direct");
        assert_eq!(reference.to_wire_value(), fused.to_wire_value(), "{module}: fused");
    }
}
