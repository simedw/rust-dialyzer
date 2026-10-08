use rust_dialyzer_poc::{
    external_term::Limits,
    snapshot_file::{self, Field, Kind, SnapshotFile},
    type_exports::arena_to_value,
};
use serde_json::{Value, json};
use std::{
    io::{self, BufRead},
    path::Path,
};
fn run(request: &Value) -> Value {
    let kind = match request["kind"].as_str() {
        Some("classic") => Kind::Classic,
        Some("incremental") => Kind::Incremental,
        _ => return json!({"error":"probe_kind"}),
    };
    let limits = Limits {
        nodes: request["max_nodes"].as_u64().unwrap_or(2_000_000) as usize,
        expanded: request["max_expanded"].as_u64().unwrap_or(2_000_000) as usize,
        ..Limits::default()
    };
    let snapshot = SnapshotFile::read(kind, Path::new(request["path"].as_str().unwrap()), limits);
    match snapshot {
        Err(error) => json!({"id":request["id"],"error":format!("{error:?}")}),
        Ok(snapshot) => {
            let arena = snapshot.arena();
            let mut result =
                json!({"id":request["id"],"arena":arena_to_value(&arena.terms,arena.root)});
            if request["incremental_data"] == true {
                result["incremental_data"] = match snapshot.incremental_data(Limits::default()) {
                    Ok(handle) => arena_to_value(&handle.arena().terms, handle.root()),
                    Err(error) => json!({"error":format!("{error:?}")}),
                };
            }
            if request["decode_info"] == true {
                result["info"] = match snapshot.field(Field::Info).decode_binary(Limits::default())
                {
                    Ok(handle) => arena_to_value(&handle.arena().terms, handle.root()),
                    Err(error) => json!({"error":format!("{error:?}")}),
                };
            }
            if let Some(output) = request["write_path"].as_str() {
                match snapshot.encoded(limits) {
                    Ok(bytes) => {
                        if let Err(error) = snapshot_file::atomic_write(Path::new(output), &bytes) {
                            result["write_error"] = json!(format!("{:?}", error.kind()));
                        }
                    }
                    Err(error) => result["write_error"] = json!(format!("{error:?}")),
                }
            }
            result
        }
    }
}
fn main() {
    for line in io::stdin().lock().lines() {
        let request: Value = serde_json::from_str(&line.unwrap()).unwrap();
        println!("{}", run(&request));
    }
}
