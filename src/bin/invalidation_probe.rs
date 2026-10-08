use rust_dialyzer_poc::invalidation::{ReverseDependencies, expand};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::io::{self, BufRead};

#[derive(Deserialize)]
struct Request {
    changed: BTreeSet<String>,
    removed: BTreeSet<String>,
    dependencies: ReverseDependencies,
    current: BTreeSet<String>,
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    for line in io::stdin().lock().lines() {
        let request: Request = serde_json::from_str(&line?)?;
        println!(
            "{}",
            serde_json::to_string(&expand(
                &request.changed,
                &request.removed,
                &request.dependencies,
                &request.current
            ))?
        );
    }
    Ok(())
}
