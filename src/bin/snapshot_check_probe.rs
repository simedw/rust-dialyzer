use rust_dialyzer_poc::incremental::{Inputs, SnapshotHeader, SnapshotProfile, plan};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, BufRead},
};
#[derive(Deserialize)]
struct Request {
    id: usize,
    previous: Option<SnapshotHeader>,
    current: BTreeMap<String, String>,
    profile: SnapshotProfile,
    legal_warnings: BTreeSet<String>,
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    for line in io::stdin().lock().lines() {
        let r: Request = serde_json::from_str(&line?)?;
        let input = Inputs::from_snapshot(
            r.previous.as_ref(),
            &r.current,
            &r.profile,
            &r.legal_warnings,
        );
        println!(
            "{}",
            serde_json::json!({"id":r.id,"check":input.check,"changed":input.changed,"removed":input.removed,"plan":plan(&input)})
        );
    }
    Ok(())
}
