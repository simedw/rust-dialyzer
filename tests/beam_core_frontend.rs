use rust_dialyzer_poc::{
    beam_frontend::{clean, debug, forms},
    external_term::{decode, encode, Limits},
};
use std::{fs, path::PathBuf};

fn fixture(name: &str, suffix: &str) -> Vec<u8> {
    fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/beam_core_port")
            .join(format!("{name}.{suffix}")),
    )
    .unwrap()
}
fn assert_oracle(name: &str, stage: &str, actual: &rust_dialyzer_poc::core::ErlangTerms) {
    let expected = decode(&fixture(name, &format!("{stage}.etf")), Limits::default()).unwrap();
    // The same encoder canonicalizes map iteration order while retaining names,
    // annotations, float bits, clauses and every declaration. No alpha renaming.
    assert_eq!(
        encode(actual, Limits::default(), false).unwrap(),
        encode(&expected, Limits::default(), false).unwrap(),
        "{name} {stage} oracle mismatch"
    );
}
#[test]
fn complete_modules_match_pinned_compiler_at_every_boundary() {
    for name in [
        "Elixir.NativeSlice",
        "Elixir.Walkthrough.Numbers",
        "Elixir.Walkthrough.Checkout",
        "Elixir.MapFun",
        "native_calls",
        "native_guards",
    ] {
        let module = debug::read_beam(&fixture(name, "beam"), Limits::default()).unwrap();
        assert_oracle(name, "debug", &module.terms);
        let recovered = forms(&module).unwrap();
        assert_oracle(name, "forms", &recovered);
        let core = rust_dialyzer_poc::beam_frontend::raw_core(&module, &recovered).unwrap();
        assert_oracle(name, "raw_core", &core);
        assert_oracle(name, "clean_core", &clean::clean(&core).unwrap());
    }
}

#[test]
fn expanded_elixir_constructs_match_full_forms() {
    for name in [
        "Elixir.NativeFor",
        "Elixir.NativePure",
        "Elixir.NativeControl",
        "Elixir.NativeRescue",
        "Elixir.NativeRewrite",
        "Elixir.NativeRecord",
        "Elixir.BinaryMapFn",
    ] {
        let module = debug::read_beam(&fixture(name, "beam"), Limits::default()).unwrap();
        assert_oracle(name, "forms", &forms(&module).unwrap());
    }
}

#[test]
fn core_counts_match_cerl_tree_size() {
    for (name, raw, cleaned) in [
        ("Elixir.NativeSlice", 451, 391),
        ("Elixir.Walkthrough.Numbers", 583, 513),
        ("Elixir.Walkthrough.Checkout", 399, 355),
        ("Elixir.MapFun", 468, 388),
    ] {
        let core = decode(&fixture(name, "raw_core.etf"), Limits::default()).unwrap();
        let clean = decode(&fixture(name, "clean_core.etf"), Limits::default()).unwrap();
        assert_eq!(
            rust_dialyzer_poc::beam_frontend::stats::core_node_count(&core).unwrap(),
            raw,
            "{name}"
        );
        assert_eq!(
            rust_dialyzer_poc::beam_frontend::stats::core_node_count(&clean).unwrap(),
            cleaned,
            "{name}"
        );
    }
}
