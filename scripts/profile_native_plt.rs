use rust_dialyzer_poc::{external_term::Limits, snapshot_file::{Field,Kind,SnapshotFile}, core::ErlangTerm};
use std::{path::Path, time::Instant};
fn main() {
    let path = std::env::args().nth(1).expect("PLT path");
    let nodes = std::env::args().nth(2).map(|n| n.parse().expect("node limit")).unwrap_or(2_000_000);
    let limits = Limits { nodes, expanded: nodes, ..Limits::default() };
    let mut samples = Vec::new();
    for i in 0..4 {
        let start = Instant::now();
        let snapshot = SnapshotFile::read(Kind::Classic, Path::new(&path), limits).expect("physical PLT");
        let mut nodes = Vec::new();
        for field in [Field::Info,Field::Contracts,Field::Callbacks,Field::Types,Field::ExportedTypes] {
            let mut handle = snapshot.field(field);
            if matches!(handle.term(), ErlangTerm::Binary(_)) {handle = handle.decode_binary(limits).expect("inner table");}
            nodes.push(handle.arena().terms.len());
            std::hint::black_box(handle);
        }
        let elapsed = start.elapsed().as_secs_f64();
        if i > 0 {samples.push(elapsed);}
        if i==0 {eprintln!("outer_nodes={} field_arena_nodes={nodes:?}",snapshot.arena().terms.len());}
        std::hint::black_box(snapshot);
    }
    println!("{}",serde_json::json!({"seconds":samples,"node_limit":nodes,"scope":"Native physical PLT read/decode only; excludes dict conversion, library import, compiler and inference"}));
}
