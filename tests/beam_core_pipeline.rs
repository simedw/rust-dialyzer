use rust_dialyzer_poc::beam_frontend::pipeline::{load, ContextSource};
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
    assert!(load(
        &[],
        ContextSource::Native {
            typesig_beam: &typesig,
            contracts_beam: &contracts
        }
    )
    .is_err());
    let duplicate = vec![files[0].clone(), files[0].clone()];
    assert!(load(
        &duplicate,
        ContextSource::Native {
            typesig_beam: &typesig,
            contracts_beam: &contracts
        }
    )
    .err()
    .unwrap()
    .contains("duplicate"));
}
