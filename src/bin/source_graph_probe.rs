use rust_dialyzer_poc::{
    native_runtime_hash::{NativeAtomLayout, NativeHashKey},
    source_digraph::SourceDigraph,
    source_graph_algorithms as algorithms,
};
use serde_json::{Value, json};
use std::io::{self, BufRead};
#[path = "support/graph_keys.rs"]
mod graph_keys;
fn keys(value: &Value) -> Result<Vec<NativeHashKey>, String> {
    value
        .as_array()
        .ok_or("keys array")?
        .iter()
        .map(graph_keys::key)
        .collect()
}
fn wire(keys: impl IntoIterator<Item = NativeHashKey>) -> Value {
    json!(
        keys.into_iter()
            .map(|k| graph_keys::wire(&k))
            .collect::<Vec<_>>()
    )
}
fn snapshot(
    graph: &SourceDigraph<NativeHashKey>,
    roots: &[NativeHashKey],
) -> Result<Value, String> {
    let vertices = graph.ordered_vertices().map_err(|e| e.to_string())?;
    let sub = algorithms::reaching_subgraph(graph, roots).map_err(|e| e.to_string())?;
    let scc = |graph: &SourceDigraph<NativeHashKey>| -> Result<Value, String> {
        Ok(json!(
            algorithms::strong_components(graph)
                .map_err(|e| e.to_string())?
                .into_iter()
                .map(wire)
                .collect::<Vec<_>>()
        ))
    };
    Ok(
        json!({"vertices":wire(vertices.clone()),"edges":wire(graph.ordered_edges().map_err(|e|e.to_string())?),"adjacency":vertices.iter().map(|v|json!({"vertex":graph_keys::wire(v),"in":wire(graph.incoming(v).unwrap_or(&[]).iter().cloned()),"out":wire(graph.outgoing(v).unwrap_or(&[]).iter().cloned()),"in_edges":wire(graph.in_edges(v).iter().cloned()),"out_edges":wire(graph.out_edges(v).iter().cloned())})).collect::<Vec<_>>(),"scc":scc(graph)?,"reaching":wire(algorithms::reaching(graph,roots)),"subgraph":{"vertices":wire(sub.ordered_vertices().map_err(|e|e.to_string())?),"edges":wire(sub.ordered_edges().map_err(|e|e.to_string())?),"scc":scc(&sub)?}}),
    )
}
fn main() -> Result<(), String> {
    for line in io::stdin().lock().lines() {
        let row: Value =
            serde_json::from_str(&line.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let layout = NativeAtomLayout::parse(&row["profile"]).map_err(|e| e.to_string())?;
        let mut graph = SourceDigraph::new(Some(layout), false);
        for v in keys(&row["confirmed"])? {
            graph.add_vertex(v, true).map_err(|e| e.to_string())?;
        }
        for edge in keys(&row["edges"])? {
            let NativeHashKey::Tuple(pair) = &edge else {
                return Err("edge tuple".into());
            };
            if pair.len() != 2 {
                return Err("edge pair".into());
            }
            for v in pair {
                if !graph.contains(v) {
                    graph
                        .add_vertex(v.clone(), false)
                        .map_err(|e| e.to_string())?;
                }
            }
            graph
                .add_edge(edge.clone(), pair[0].clone(), pair[1].clone())
                .map_err(|e| e.to_string())?;
        }
        let roots = keys(&row["roots"])?;
        let before = snapshot(&graph, &roots)?;
        let remove = graph
            .ordered_vertices()
            .map_err(|e| e.to_string())?
            .into_iter()
            .filter(|v| !graph.is_confirmed(v))
            .rev()
            .collect::<Vec<_>>();
        for v in remove {
            graph.delete_vertex(&v).map_err(|e| e.to_string())?;
        }
        let after = snapshot(&graph, &roots)?;
        let matched = before == row["before"] && after == row["after"];
        println!(
            "{}",
            json!({"id":row["id"],"matches":matched,"before":if before==row["before"]{Value::Null}else{before},"after":if after==row["after"]{Value::Null}else{after}})
        );
    }
    Ok(())
}
