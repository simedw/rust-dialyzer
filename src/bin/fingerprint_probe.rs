use rust_dialyzer_poc::{
    core::{CoreProgram, ErlangTerms},
    fingerprint,
};
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn main() -> Result<(), String> {
    for line in io::stdin().lock().lines() {
        let row: Value =
            serde_json::from_str(&line.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let core = CoreProgram::parse(
            &std::fs::read_to_string(row["core_json"].as_str().ok_or("Core path expected")?)
                .map_err(|e| e.to_string())?,
        )?;
        let modules = core.modules.iter().map(|module| {
            let arena = ErlangTerms { root: module.source_forms, terms: module.terms.clone() };
            let digest = fingerprint::module_fingerprint(&arena, core.profile.term_map_order)?;
            Ok(json!({"name":module.name,"md5":digest.iter().map(|byte|format!("{byte:02x}")).collect::<String>()}))
        }).collect::<Result<Vec<_>, String>>()?;
        println!("{}", json!({"id":row["id"],"modules":modules}));
    }
    Ok(())
}
