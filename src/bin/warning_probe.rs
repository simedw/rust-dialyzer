use rust_dialyzer_poc::warnings::{Profile, Registry, Tag};
use serde::Deserialize;
use serde_json::json;
use std::io::{self, BufRead};
#[derive(Deserialize)]
struct Request {
    initial: Vec<Tag>,
    options: Vec<String>,
}
fn main() {
    let registry =
        Registry::load(include_str!("../../corpus/warnings.json")).expect("verified catalogue");
    for line in io::stdin().lock().lines() {
        let result = line
            .map_err(|e| e.to_string())
            .and_then(|s| serde_json::from_str::<Request>(&s).map_err(|e| e.to_string()))
            .and_then(|r| registry.apply(&Profile::from_tags(r.initial), &r.options));
        println!(
            "{}",
            match result {
                Ok(profile) => json!({"tags":profile.tags()}),
                Err(error) => json!({"error":error}),
            }
        );
    }
}
