use rust_dialyzer_poc::{
    beam_frontend::{adapter, Compilation},
    core::etf::module_from_etf,
    external_term::{decode, Limits},
    native_runtime_hash::NativeAtomLayout,
};
use std::{fs, path::Path};
#[test]
fn compact_native_arenas_match_independent_transport() {
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
        let actual = adapter::module(
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
