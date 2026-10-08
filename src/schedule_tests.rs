use super::*;

#[test]
fn ordering_is_callee_first_with_stable_components_and_reverse_users() {
    let graph =
        DependencyGraph::build(["d", "c", "b", "a"], [("a", "b"), ("b", "c"), ("a", "d")]).unwrap();
    assert_eq!(graph.vertices, vec!["a", "b", "c", "d"]);
    assert_eq!(
        graph.callee_first,
        vec![
            ComponentId(2),
            ComponentId(1),
            ComponentId(3),
            ComponentId(0)
        ]
    );
    assert_eq!(graph.reverse_users[2], vec![VertexId(1)]);
    let reversed =
        DependencyGraph::build(["a", "b", "c", "d"], [("a", "d"), ("b", "c"), ("a", "b")]).unwrap();
    assert_eq!(graph.callee_first, reversed.callee_first);
    assert_eq!(graph.vertex_component, reversed.vertex_component);
}

#[test]
fn mutual_recursion_self_loops_and_isolated_vertices_are_all_present() {
    let graph = DependencyGraph::build(0..5, [(0, 1), (1, 0), (2, 2), (3, 1)]).unwrap();
    assert_eq!(graph.components.len(), 4);
    assert_eq!(graph.vertex_component[0], graph.vertex_component[1]);
    assert!(
        graph
            .components
            .iter()
            .find(|c| c.id == ComponentId(0))
            .unwrap()
            .recursive
    );
    assert!(
        graph
            .components
            .iter()
            .find(|c| c.id == ComponentId(2))
            .unwrap()
            .recursive
    );
    assert!(
        !graph
            .components
            .iter()
            .find(|c| c.id == ComponentId(4))
            .unwrap()
            .recursive
    );
    assert_eq!(
        graph
            .components
            .iter()
            .map(|c| c.members.len())
            .sum::<usize>(),
        5
    );
}

#[test]
fn deep_chain_and_cycle_are_iterative() {
    let count = 20_000;
    let chain = DependencyGraph::build(0..count, (0..count - 1).map(|i| (i, i + 1))).unwrap();
    assert_eq!(chain.components.len(), count);
    assert_eq!(chain.callee_first.first(), Some(&ComponentId(count - 1)));
    assert_eq!(chain.callee_first.last(), Some(&ComponentId(0)));
    let cycle = DependencyGraph::build(0..count, (0..count).map(|i| (i, (i + 1) % count))).unwrap();
    assert_eq!(cycle.components.len(), 1);
    assert_eq!(cycle.components[0].members.len(), count);
    assert!(cycle.components[0].recursive);
}

#[test]
fn unregistered_edges_are_errors_and_empty_graph_is_complete() {
    assert!(
        DependencyGraph::build([0], [(0, 1)])
            .unwrap_err()
            .contains("target")
    );
    assert!(
        DependencyGraph::build([0], [(1, 0)])
            .unwrap_err()
            .contains("source")
    );
    let empty = DependencyGraph::<usize>::build([], []).unwrap();
    assert!(empty.callee_first.is_empty());
    empty.validate().unwrap();
}
