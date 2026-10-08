use rust_dialyzer_poc::{constraints::generation_capture_headers::*, labels, term_order};
use serde_json::Value;
fn reference(v: &Value) -> Result<SourceReference, String> {
    let text = |key: &str| {
        v[key]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| format!("header reference {key} missing"))
    };
    Ok(SourceReference(term_order::RuntimeIdentity {
        scope: text("scope")?,
        object: text("object")?,
    }))
}
fn table(v: &Value) -> Result<CodeServerTable, String> {
    if v == "clean" {
        Ok(CodeServerTable::Clean)
    } else {
        reference(v).map(CodeServerTable::Live)
    }
}
fn digraph(v: &Value) -> Result<DigraphCapture, String> {
    Ok(DigraphCapture {
        vertices: reference(&v["vertices"])?,
        edges: reference(&v["edges"])?,
        neighbours: reference(&v["neighbours"])?,
        cyclic: v["cyclic"].as_bool().ok_or("digraph cyclic missing")?,
    })
}
pub fn parse(v: &Value) -> Result<GenerationRuntimeCaptures, String> {
    let cg = &v["callgraph"];
    let callgraph = if cg.is_null() {
        None
    } else {
        Some(CallGraphCapture {
            digraph: digraph(&cg["digraph"])?,
            active: match cg["active"]["kind"].as_str() {
                Some("undefined") => ActiveDigraphCapture::Undefined,
                Some("digraph") => ActiveDigraphCapture::Digraph(digraph(&cg["active"]["value"])?),
                Some("outgoing") => {
                    ActiveDigraphCapture::Outgoing(reference(&cg["active"]["value"])?)
                }
                _ => return Err("unknown active graph".into()),
            },
            escaping: reference(&cg["escaping"])?,
            letrec: reference(&cg["letrec"])?,
            names: reference(&cg["names"])?,
            reverse_names: reference(&cg["reverse_names"])?,
            recursive_variables: reference(&cg["recursive_variables"])?,
            self_recursive: reference(&cg["self_recursive"])?,
            calls: reference(&cg["calls"])?,
        })
    };
    let cs = &v["codeserver"];
    let codeserver = if cs.is_null() {
        None
    } else {
        Some(CodeServerCapture {
            next_core_label: labels::CoreLabel(
                cs["next_core_label"]
                    .as_u64()
                    .ok_or("server next label missing")?,
            ),
            code: reference(&cs["code"])?,
            exported_types: table(&cs["exported_types"])?,
            records: table(&cs["records"])?,
            contracts: reference(&cs["contracts"])?,
            callbacks: reference(&cs["callbacks"])?,
            function_metadata: reference(&cs["function_metadata"])?,
            exports: table(&cs["exports"])?,
            temporary_exported_types: table(&cs["temporary_exported_types"])?,
            temporary_records: table(&cs["temporary_records"])?,
            temporary_contracts: table(&cs["temporary_contracts"])?,
            temporary_callbacks: table(&cs["temporary_callbacks"])?,
        })
    };
    let p = &v["plt"];
    let plt = if p.is_null() {
        None
    } else {
        Some(PltCapture {
            signatures: reference(&p["signatures"])?,
            types: reference(&p["types"])?,
            contracts: reference(&p["contracts"])?,
            callbacks: reference(&p["callbacks"])?,
            exported_types: reference(&p["exported_types"])?,
        })
    };
    Ok(GenerationRuntimeCaptures {
        callgraph,
        codeserver,
        plt,
    })
}
