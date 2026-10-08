use rust_dialyzer_poc::{
    core::ErlangTerms,
    warning_render::{FilenameOption, LocationOption, Options, Renderer},
};
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn main() {
    let renderer = Renderer::default();
    for line in io::stdin().lock().lines() {
        let row: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let arena = ErlangTerms::parse(&row["warning"]).unwrap();
        let options = Options {
            indent: row["options"]["indent"].as_bool().unwrap(),
            filename: if row["options"]["filename"] == "fullpath" {
                FilenameOption::Fullpath
            } else {
                FilenameOption::Basename
            },
            location: if row["options"]["location"] == "line" {
                LocationOption::Line
            } else {
                LocationOption::Column
            },
        };
        let result = match renderer.format_warning(&arena, arena.root, options) {
            Ok(text) => {
                json!({"id":row["id"],"matched":text==row["expected"],"status":"rendered","text":text})
            }
            Err(error) => {
                json!({"id":row["id"],"matched":false,"status":"unsupported","error":error.to_string()})
            }
        };
        println!("{result}");
    }
}
