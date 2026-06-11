//! W5 (ann-width RFC §5.5) — growth-cycle detection on the slot
//! assignment graph.
//!
//! A slot whose value feeds back into itself through a growth op
//! (Mul, or Add/Sub with a non-constant increment) grows without
//! bound across iterations. The value stays integral — so the W1
//! poison seeds never fire — yet it is not i64-safe: past 2^53 the
//! f64 semantics round where i64 stays exact, and past 2^63 i64
//! wraps (repro S7: `n = n * 3 + 1` looped 40 steps). Such slots
//! must take the F64 representation; interval + float_demote (1b-ii)
//! pull the hot path back to i64 under runtime guards.
//!
//! "Feeds back into itself" is a property of the assignment graph,
//! not of any single statement: `tmp = n * 3; n = tmp + 1` and the
//! cross-function `return fib(n - 1) + fib(n - 2)` are the same
//! disease. So the detection is graph-global — Tarjan SCC over the
//! constraint edges, then every SCC containing an internal growth
//! edge (including a growth self-loop) seeds all its members F64.

use super::{Dep, SlotKey};
use std::collections::HashMap;

/// Append every slot that sits on an assignment cycle containing a
/// growth edge to `seeds`. Called once, before the poison fixpoint.
pub(super) fn seed_growth_cycles(edges: &HashMap<SlotKey, Vec<Dep>>, seeds: &mut Vec<SlotKey>) {
    // Index the node set: every key plus every edge destination.
    let mut index_of: HashMap<&SlotKey, usize> = HashMap::new();
    let mut nodes: Vec<&SlotKey> = Vec::new();
    for (src, dsts) in edges {
        if !index_of.contains_key(src) {
            index_of.insert(src, nodes.len());
            nodes.push(src);
        }
        for (d, _) in dsts {
            if !index_of.contains_key(d) {
                index_of.insert(d, nodes.len());
                nodes.push(d);
            }
        }
    }
    let n = nodes.len();
    let mut adj: Vec<Vec<(usize, bool)>> = vec![Vec::new(); n];
    for (src, dsts) in edges {
        let s = index_of[src];
        for (d, growth) in dsts {
            adj[s].push((index_of[d], *growth));
        }
    }

    let scc_of = tarjan(n, &adj);

    // An SCC is growth-poisoned when any growth edge stays inside it
    // (u → v with scc(u) == scc(v) covers self-loops too).
    let mut poisoned: Vec<bool> = vec![false; n];
    for (u, dsts) in adj.iter().enumerate() {
        for &(v, growth) in dsts {
            if growth && scc_of[u] == scc_of[v] {
                poisoned[scc_of[u]] = true;
            }
        }
    }
    for (i, node) in nodes.iter().enumerate() {
        if poisoned[scc_of[i]] {
            seeds.push((*node).clone());
        }
    }
}

/// Iterative Tarjan — returns the SCC id of every node. Component
/// ids are arbitrary but equal within one SCC. Iterative because the
/// assignment graph depth is module-sized, not stack-sized.
fn tarjan(n: usize, adj: &[Vec<(usize, bool)>]) -> Vec<usize> {
    const UNSET: usize = usize::MAX;
    let mut index = vec![UNSET; n];
    let mut lowlink = vec![0usize; n];
    let mut on_stack = vec![false; n];
    let mut scc_of = vec![0usize; n];
    let mut stack: Vec<usize> = Vec::new();
    let mut next_index = 0usize;
    let mut scc_count = 0usize;
    // Explicit DFS frames: (node, next edge position to visit).
    let mut frames: Vec<(usize, usize)> = Vec::new();

    for root in 0..n {
        if index[root] != UNSET {
            continue;
        }
        frames.push((root, 0));
        while let Some(top) = frames.last_mut() {
            let v = top.0;
            if top.1 == 0 {
                index[v] = next_index;
                lowlink[v] = next_index;
                next_index += 1;
                stack.push(v);
                on_stack[v] = true;
            }
            let ei = top.1;
            if ei < adj[v].len() {
                top.1 += 1;
                let (w, _) = adj[v][ei];
                if index[w] == UNSET {
                    frames.push((w, 0));
                } else if on_stack[w] {
                    lowlink[v] = lowlink[v].min(index[w]);
                }
            } else {
                frames.pop();
                if let Some(&(parent, _)) = frames.last() {
                    lowlink[parent] = lowlink[parent].min(lowlink[v]);
                }
                if lowlink[v] == index[v] {
                    while let Some(w) = stack.pop() {
                        on_stack[w] = false;
                        scc_of[w] = scc_count;
                        if w == v {
                            break;
                        }
                    }
                    scc_count += 1;
                }
            }
        }
    }
    scc_of
}
