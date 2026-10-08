use rust_dialyzer_poc::{
    core::CoreProgram,
    core::ErlangTerms,
    dependencies::{ImportPlan, PriorPlt},
    term_order::compare_map_keys,
};
use std::io::{self, BufRead};
fn main() -> Result<(), String> {
    let prior: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(std::env::args().nth(1).ok_or("prior")?)
            .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    for line in io::stdin().lock().lines() {
        let row: serde_json::Value =
            serde_json::from_str(&line.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let core = CoreProgram::parse(&row["core"].to_string())?;
        let mut prior = prior.clone();
        prior["version"] = 4.into();
        prior.as_object_mut().unwrap().remove("callbacks");
        prior.as_object_mut().unwrap().remove("callback_lists");
        prior["raw_callback_groups"] = row["expected_groups"].clone();
        let store = PriorPlt::parse(&prior.to_string(), &core, &ImportPlan::cold_targets(&core))?
            .into_cold_store()?;
        let library = store.callbacks.original().map_err(|e| format!("{e:?}"))?;
        let actual = library.raw();
        let expected = ErlangTerms::parse(&row["expected_groups"])?;
        let exact =
            compare_map_keys(actual.arena(), actual.id(), &expected, expected.root)?.is_eq();
        println!(
            "{}",
            serde_json::json!({"id":row["id"],"exact":exact,"rows":library.rows().count(),"callable":library.rows().filter(|r|r.callable().is_some()).count()})
        );
    }
    Ok(())
}
