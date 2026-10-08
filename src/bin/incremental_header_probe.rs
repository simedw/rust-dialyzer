use rust_dialyzer_poc::{
    external_term::{self, Limits},
    incremental_persistence::StoredIncrementalHeader,
    snapshot_file::{Kind, SnapshotFile},
};
use std::{
    io::{self, BufRead},
    path::Path,
};
fn main() -> Result<(), String> {
    for line in io::stdin().lock().lines() {
        let row: serde_json::Value =
            serde_json::from_str(&line.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let limits = Limits {
            bytes: 512 * 1024 * 1024,
            nodes: 20_000_000,
            expanded: 20_000_000,
        };
        let file = SnapshotFile::read(
            Kind::Incremental,
            Path::new(row["path"].as_str().ok_or("path")?),
            limits,
        )
        .map_err(|e| format!("{e:?}"))?;
        let header = StoredIncrementalHeader::read(&file, limits).map_err(|e| format!("{e:?}"))?;
        let mut output = Vec::new();
        for raw in [
            &header.version,
            &header.module_fingerprints,
            &header.implementation_fingerprints,
            &header.dependency_record,
            &header.warning_cache,
            &header.legal_warnings,
        ] {
            let encoded = external_term::encode_at(
                raw.arena(),
                raw.id(),
                limits,
                false,
                external_term::MapEncoding::Canonical,
            )
            .map_err(|e| format!("{e:?}"))?;
            output.push(
                encoded
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>(),
            );
        }
        println!("{}", serde_json::json!({"id":row["id"],"fields":output}));
    }
    Ok(())
}
