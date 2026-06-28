use std::collections::{BinaryHeap, HashMap, HashSet};
use std::cmp::Reverse;

use petgraph::{
    algo::kosaraju_scc,
    graph::{DiGraph, NodeIndex},
    Direction,
};

use crate::{
    ast::ResolvedEnv,
    error::{EnvxError, Result},
};

// ─── Public entry point ───────────────────────────────────────────────────────

/// Build a dependency graph from `env` and return variable names in the order
/// they must be evaluated (dependencies before dependants).
///
/// **Graph construction**
/// One node per variable. For each variable whose template references `$OTHER`,
/// an edge `OTHER → variable` is added (meaning "evaluate OTHER first").
/// References to names that don't exist in `env` are silently ignored here —
/// the evaluator will raise `UndefinedVariable` when it encounters them.
///
/// **Stable ordering**
/// Uses Kahn's BFS algorithm with a min-heap keyed on the original IndexMap
/// insertion index. This guarantees that among variables with no mutual
/// dependency, the declaration order is preserved:  `A, B, C` stays `A, B, C`.
/// `petgraph::toposort` uses DFS post-order which reverses unrelated nodes.
pub fn build_and_sort(env: &ResolvedEnv) -> Result<Vec<String>> {
    let mut graph: DiGraph<String, ()> = DiGraph::new();
    let mut name_to_idx: HashMap<&str, NodeIndex> = HashMap::new();

    // Add one node per variable (in IndexMap order for deterministic output).
    for key in env.entries.keys() {
        let idx = graph.add_node(key.clone());
        name_to_idx.insert(key.as_str(), idx);
    }

    // Add dependency edges: dep_idx → dependant_idx  ("dep must come first").
    for (key, (template, _)) in &env.entries {
        let dependant = name_to_idx[key.as_str()];
        for dep in template.collect_var_refs() {
            if let Some(&dep_idx) = name_to_idx.get(dep.as_str()) {
                graph.add_edge(dep_idx, dependant, ());
            }
        }
    }

    kahn_sort(&graph)
}

// ─── Kahn's algorithm (stable) ────────────────────────────────────────────────

fn kahn_sort(graph: &DiGraph<String, ()>) -> Result<Vec<String>> {
    let n = graph.node_count();

    // Compute in-degree for every node.
    let mut in_degree: Vec<usize> = vec![0; n];
    for node in graph.node_indices() {
        for _ in graph.neighbors_directed(node, Direction::Incoming) {
            in_degree[node.index()] += 1;
        }
    }

    // Min-heap by node index so that ties break in insertion (declaration) order.
    let mut ready: BinaryHeap<Reverse<usize>> = BinaryHeap::new();
    for node in graph.node_indices() {
        if in_degree[node.index()] == 0 {
            ready.push(Reverse(node.index()));
        }
    }

    let mut order: Vec<String> = Vec::with_capacity(n);
    while let Some(Reverse(idx)) = ready.pop() {
        let node = NodeIndex::new(idx);
        order.push(graph[node].clone());
        for neighbor in graph.neighbors_directed(node, Direction::Outgoing) {
            let ni = neighbor.index();
            in_degree[ni] -= 1;
            if in_degree[ni] == 0 {
                ready.push(Reverse(ni));
            }
        }
    }

    if order.len() < n {
        // Remaining nodes with in_degree > 0 are part of a cycle.
        let hint = graph
            .node_indices()
            .find(|n| in_degree[n.index()] > 0)
            .expect("at least one node must be in the cycle");
        Err(EnvxError::CircularDependency {
            cycle: find_cycle_path(graph, hint),
        })
    } else {
        Ok(order)
    }
}

// ─── Cycle path finder ────────────────────────────────────────────────────────

/// Given the node that `toposort` identified as part of a cycle, find its
/// strongly connected component and trace a human-readable path through it.
fn find_cycle_path(graph: &DiGraph<String, ()>, hint: NodeIndex) -> String {
    let sccs = kosaraju_scc(graph);

    // Locate the SCC that contains `hint` and actually forms a cycle.
    let target = sccs.iter().find(|scc| {
        (scc.len() > 1 && scc.contains(&hint))
            || (scc.len() == 1
                && scc[0] == hint
                && graph.contains_edge(hint, hint))
    });

    match target {
        Some(nodes) => trace_cycle(graph, nodes),
        None => graph[hint].clone(), // shouldn't happen; graceful fallback
    }
}

/// Follow edges within `scc` starting from the first node until we return to
/// the start, building a path string like `"A → B → C → A"`.
///
/// Uses DFS limited to nodes within the SCC. The recursion is kept as a plain
/// inner `fn` (not a closure) so it can be called recursively.
fn trace_cycle(graph: &DiGraph<String, ()>, scc: &[NodeIndex]) -> String {
    let in_scc: HashSet<NodeIndex> = scc.iter().copied().collect();
    let start = scc[0];
    let mut path = vec![start];
    let mut visited: HashSet<NodeIndex> = std::iter::once(start).collect();

    fn dfs(
        g: &DiGraph<String, ()>,
        start: NodeIndex,
        cur: NodeIndex,
        in_scc: &HashSet<NodeIndex>,
        path: &mut Vec<NodeIndex>,
        visited: &mut HashSet<NodeIndex>,
    ) -> bool {
        for next in g.neighbors(cur) {
            if !in_scc.contains(&next) {
                continue;
            }
            if next == start {
                path.push(start); // close the loop
                return true;
            }
            if visited.insert(next) {
                path.push(next);
                if dfs(g, start, next, in_scc, path, visited) {
                    return true;
                }
                path.pop(); // backtrack
            }
        }
        false
    }

    if dfs(graph, start, start, &in_scc, &mut path, &mut visited) {
        path.iter()
            .map(|&n| graph[n].as_str())
            .collect::<Vec<_>>()
            .join(" → ")
    } else {
        // Fallback: list all SCC members (should not normally occur).
        scc.iter()
            .map(|&n| graph[n].as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use crate::{ast::ResolvedEnv, parser};

    fn make_env(src: &str) -> ResolvedEnv {
        use crate::ast::Statement;
        let path = PathBuf::from("/test/test.envx");
        let file = parser::parse(src, "test.envx", path.clone()).unwrap();
        let mut env = ResolvedEnv::default();
        for stmt in file.statements {
            if let Statement::Entry { key, template, source, .. } = stmt {
                env.entries.insert(key, (template, source));
            }
        }
        env.sources.insert(path, src.to_string());
        env
    }

    #[test]
    fn no_dependencies_preserves_declaration_order() {
        let env = make_env("A = \"1\"\nB = \"2\"\nC = \"3\"\n");
        let order = build_and_sort(&env).unwrap();
        assert_eq!(order, &["A", "B", "C"]);
    }

    #[test]
    fn linear_dependency_reversed() {
        // C depends on B, B depends on A → order must be A, B, C
        let env = make_env(
            "C = \"${{ $B }}\"\nB = \"${{ $A }}\"\nA = \"base\"\n",
        );
        let order = build_and_sort(&env).unwrap();
        let pos = |name: &str| order.iter().position(|k| k == name).unwrap();
        assert!(pos("A") < pos("B"));
        assert!(pos("B") < pos("C"));
    }

    #[test]
    fn diamond_dependency() {
        // B and C both depend on A; D depends on B and C.
        let env = make_env(
            "D = \"${{ $B }}-${{ $C }}\"\nB = \"${{ $A }}\"\nC = \"${{ $A }}\"\nA = \"x\"\n",
        );
        let order = build_and_sort(&env).unwrap();
        let pos = |name: &str| order.iter().position(|k| k == name).unwrap();
        assert!(pos("A") < pos("B"));
        assert!(pos("A") < pos("C"));
        assert!(pos("B") < pos("D"));
        assert!(pos("C") < pos("D"));
    }

    #[test]
    fn ref_to_undefined_var_is_not_a_dag_error() {
        // The DAG silently ignores unknown refs; the evaluator reports them.
        let env = make_env("X = \"${{ $UNDEFINED }}\"\n");
        assert!(build_and_sort(&env).is_ok());
    }

    #[test]
    fn direct_circular_dependency_is_error() {
        // A = ${{ $B }}, B = ${{ $A }}
        let env = make_env("A = \"${{ $B }}\"\nB = \"${{ $A }}\"\n");
        let err = build_and_sort(&env).unwrap_err();
        assert!(
            matches!(&err, EnvxError::CircularDependency { cycle }
                if cycle.contains('A') || cycle.contains('B')),
            "unexpected err: {:?}", err
        );
    }

    #[test]
    fn self_referential_var_is_error() {
        let env = make_env("A = \"${{ $A }}\"\n");
        assert!(matches!(
            build_and_sort(&env),
            Err(EnvxError::CircularDependency { .. })
        ));
    }

    #[test]
    fn three_node_cycle_is_error() {
        let env = make_env(
            "A = \"${{ $C }}\"\nB = \"${{ $A }}\"\nC = \"${{ $B }}\"\n",
        );
        let err = build_and_sort(&env).unwrap_err();
        assert!(matches!(err, EnvxError::CircularDependency { .. }));
        if let EnvxError::CircularDependency { cycle } = err {
            // Cycle string must name all three variables
            assert!(cycle.contains('A'), "cycle: {cycle}");
            assert!(cycle.contains('B'), "cycle: {cycle}");
            assert!(cycle.contains('C'), "cycle: {cycle}");
        }
    }
}
