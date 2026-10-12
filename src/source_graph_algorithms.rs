use crate::{
    native_runtime_hash::RuntimeHashError,
    source_digraph::{RuntimeGraphKey, SourceDigraph},
};
use std::collections::BTreeSet;

pub fn revpostorder<V: RuntimeGraphKey>(
    graph: &SourceDigraph<V>,
) -> Result<Vec<V>, RuntimeHashError> {
    enum Visit<V> {
        Enter(V),
        Exit(V),
    }
    let mut seen = BTreeSet::new();
    let mut finished = Vec::new();
    for root in graph.ordered_vertices()? {
        let mut stack = vec![Visit::Enter(root)];
        while let Some(visit) = stack.pop() {
            match visit {
                Visit::Enter(v) => {
                    if seen.insert(v.clone()) {
                        stack.push(Visit::Exit(v.clone()));
                        stack.extend(
                            graph
                                .outgoing(&v)
                                .unwrap_or_default()
                                .iter()
                                .rev()
                                .cloned()
                                .map(Visit::Enter),
                        );
                    }
                }
                Visit::Exit(v) => finished.push(v),
            }
        }
    }
    finished.reverse();
    Ok(finished)
}
fn forest<V: RuntimeGraphKey>(
    graph: &SourceDigraph<V>,
    roots: impl IntoIterator<Item = V>,
) -> Vec<Vec<V>> {
    let mut seen = BTreeSet::new();
    let mut components = Vec::new();
    for root in roots {
        let mut pending = vec![root];
        let mut component = Vec::new();
        while let Some(v) = pending.pop() {
            if seen.insert(v.clone()) {
                pending.extend(graph.incoming(&v).unwrap_or_default().iter().rev().cloned());
                component.push(v);
            }
        }
        if !component.is_empty() {
            component.reverse();
            components.push(component);
        }
    }
    components.reverse();
    components
}
pub fn strong_components<V: RuntimeGraphKey>(
    graph: &SourceDigraph<V>,
) -> Result<Vec<Vec<V>>, RuntimeHashError> {
    Ok(forest(graph, revpostorder(graph)?))
}
pub fn reaching<V: RuntimeGraphKey>(graph: &SourceDigraph<V>, roots: &[V]) -> Vec<V> {
    forest(graph, roots.iter().cloned())
        .into_iter()
        .flatten()
        .collect()
}
pub fn topsort<V: RuntimeGraphKey>(
    graph: &SourceDigraph<V>,
) -> Result<Option<Vec<V>>, RuntimeHashError> {
    let order = revpostorder(graph)?;
    Ok(
        (forest(graph, order.iter().cloned()).len() == graph.ordered_vertices()?.len())
            .then_some(order),
    )
}
pub fn reaching_subgraph<V: RuntimeGraphKey>(
    graph: &SourceDigraph<V>,
    roots: &[V],
) -> Result<SourceDigraph<V>, RuntimeHashError> {
    let mut sub = SourceDigraph::new(Some(graph.layout()?.clone()), false);
    for vertex in reaching(graph, roots) {
        if graph.contains(&vertex) {
            sub.add_vertex(vertex.clone(), graph.is_confirmed(&vertex))?;
        }
    }
    for vertex in sub.ordered_vertices()? {
        for edge in graph.out_edges(&vertex) {
            if let Some((from, to)) = graph.edge(edge)
                && sub.contains(to)
            {
                sub.add_edge(edge.clone(), from.clone(), to.clone())?;
            }
        }
    }
    Ok(sub)
}
