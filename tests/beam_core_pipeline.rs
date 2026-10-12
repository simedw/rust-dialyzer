use rust_dialyzer_poc::beam_frontend::pipeline::{ContextSource, load};
use std::{fs, path::Path};
#[test]
fn native_pipeline_builds_valid_analyzer_program_without_a_vm() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/beam_core_port");
    let typesig = fs::read(root.join("runtime_context/dialyzer_typesig.beam")).unwrap();
    let contracts = fs::read(root.join("runtime_context/dialyzer_contracts.beam")).unwrap();
    let files = [
        "Elixir.NativeSlice",
        "Elixir.Walkthrough.Numbers",
        "native_calls",
    ]
    .map(|name| {
        root.join(format!("{name}.beam"))
            .to_str()
            .unwrap()
            .to_owned()
    });
    let loaded = load(
        &files,
        ContextSource::Native {
            typesig_beam: &typesig,
            contracts_beam: &contracts,
        },
    )
    .unwrap();
    assert_eq!(loaded.core.modules.len(), 3);
    assert_eq!(
        loaded.timings["identity_origin"],
        "deterministic_native_scope"
    );
    for module in &loaded.core.modules {
        assert!(!module.nodes.is_empty());
        assert!(module.core.0 < module.nodes.len());
    }
    assert!(
        load(
            &[],
            ContextSource::Native {
                typesig_beam: &typesig,
                contracts_beam: &contracts
            }
        )
        .is_err()
    );
    let duplicate = vec![files[0].clone(), files[0].clone()];
    assert!(
        load(
            &duplicate,
            ContextSource::Native {
                typesig_beam: &typesig,
                contracts_beam: &contracts
            }
        )
        .err()
        .unwrap()
        .contains("duplicate")
    );
}

#[test]
fn fused_parallel_discovery_preserves_native_scope_modules_and_imported_context() {
    use rust_dialyzer_poc::beam_frontend::{
        pipeline::{Emission, load_emission},
        runtime_context::pinned_profile,
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/beam_core_port");
    let typesig = fs::read(root.join("runtime_context/dialyzer_typesig.beam")).unwrap();
    let contracts = fs::read(root.join("runtime_context/dialyzer_contracts.beam")).unwrap();
    let files = [
        "Elixir.NativeSlice",
        "Elixir.Walkthrough.Numbers",
        "Elixir.MapFun",
        "core_lower_binary",
        "core_lower_after",
        "native_calls",
    ]
    .map(|name| {
        root.join(format!("{name}.beam"))
            .to_str()
            .unwrap()
            .to_owned()
    });
    let source = || ContextSource::Native {
        typesig_beam: &typesig,
        contracts_beam: &contracts,
    };
    let reference = load_emission(&files, source(), Emission::Reference).unwrap();
    let fused = load_emission(&files, source(), Emission::Fused).unwrap();
    let scope = |loaded: &rust_dialyzer_poc::beam_frontend::pipeline::LoadedNative| {
        loaded.core.atom_layout.as_ref().unwrap().to_wire()
    };
    assert_eq!(
        scope(&fused),
        scope(&reference),
        "all atom identities and scope digest"
    );
    assert_eq!(
        fused.core.modules, reference.core.modules,
        "all compact terms/nodes/IDs/provenance"
    );
    let mut reversed = files.to_vec();
    reversed.reverse();
    let reversed = load_emission(&reversed, source(), Emission::Fused).unwrap();
    assert_eq!(
        scope(&reversed),
        scope(&reference),
        "scope is independent of input order"
    );
    for module in &reversed.core.modules {
        assert_eq!(
            Some(module),
            reference
                .core
                .modules
                .iter()
                .find(|m| m.name == module.name)
        );
    }

    let typesig_profile: serde_json::Value = serde_json::from_slice(
        &fs::read(root.join("runtime_context/dialyzer_typesig.json")).unwrap(),
    )
    .unwrap();
    let contracts_profile: serde_json::Value = serde_json::from_slice(
        &fs::read(root.join("runtime_context/dialyzer_contracts.json")).unwrap(),
    )
    .unwrap();
    let context = serde_json::json!({
        "version": 6, "profile": pinned_profile(), "atom_layout": scope(&reference),
        "contract_closure_profile": contracts_profile,
    });
    let imported = || ContextSource::Imported {
        context: &context,
        closure_profile: &typesig_profile,
    };
    let imported_reference = load_emission(&files, imported(), Emission::Reference).unwrap();
    let imported_fused = load_emission(&files, imported(), Emission::Fused).unwrap();
    assert_eq!(
        scope(&imported_fused),
        context["atom_layout"],
        "imported layout is retained"
    );
    assert_eq!(imported_fused.core.modules, imported_reference.core.modules);
    assert_eq!(
        imported_fused.timings["identity_origin"],
        "imported_context"
    );
}
