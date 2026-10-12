use rust_dialyzer_poc::{
    beam_frontend::{adapter, debug, forms, Compilation},
    core::{etf::module_from_etf, CoreModule},
    external_term::{decode_interned, Limits},
    native_runtime_hash::NativeAtomLayout,
};
use serde_json::{json, Value};
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
fn entire_corpus_adapter_matches_compiler_transport() {
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
                let actual = adapter::module(
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
