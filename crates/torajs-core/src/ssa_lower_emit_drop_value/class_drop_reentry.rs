//! Bounded native depth for `__drop_<C>` fns that reach themselves.
//!
//! A `__drop_<C>` calls the `__drop_` fn of every class it releases in
//! place, so classes that hold each other (`A { b: B }`, `B { a: A }`)
//! give release fns that call each other once per level of the data,
//! and a chain a million levels deep nests a million frames. The
//! worklist's own fallback call, made when its stack is full, is the
//! same edge from a fn to itself.
//!
//! Once every body is built, the calls between these fns form a graph;
//! a depth-first walk over it finds the back edges, and every cycle
//! holds at least one. A call along a back edge goes through
//! `__torajs_drop_release(child, __drop_<D>)` instead (torajs-value-drop):
//! a compare against the stack pointer, then the same call, or below
//! the floor near the stack bottom a deferral run in a loop. Calls
//! along the other edges, and every fn that reaches no cycle, are left
//! as they are.

use std::collections::HashMap;

use crate::ssa::{self, FuncId, InstKind, Module, Operand, Type};
use crate::ssa_lower::intern_fn_sig;
use crate::ssa_lower_emit_drop_value::class_drop_setup::ClassDropFn;

const DROP_RELEASE_SYM: &str = "__torajs_drop_release";

/// Route every back-edge call between the `__drop_` fns through the
/// runtime's depth-bounded release.
pub(crate) fn route(
    class_drops: &[ClassDropFn],
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    module: &mut Module,
) {
    let fids: Vec<FuncId> = class_drops.iter().map(|d| d.fid).collect();
    let back = back_edges(&fids, module);
    if back.is_empty() {
        return;
    }
    let sig = Type::FnSig(intern_fn_sig(fn_sigs, vec![Type::Ptr], Type::Void));
    let release = FuncId(module.funcs.len() as u32);
    let mut decl = ssa::Function::new(DROP_RELEASE_SYM, Type::Void);
    decl.add_param(Type::Ptr, "p");
    decl.add_param(sig, "f");
    module.funcs.push(decl);
    for (u, v) in back {
        let f = &mut module.funcs[fids[u].0 as usize];
        let callee = fids[v];
        for b in 0..f.blocks.len() {
            let mut i = 0;
            while i < f.blocks[b].insts.len() {
                let InstKind::Call(c, args) = &f.blocks[b].insts[i].kind else {
                    i += 1;
                    continue;
                };
                if *c != callee {
                    i += 1;
                    continue;
                }
                let child = args[0];
                let addr = f.insert_inst(ssa::BlockId(b as u32), i, InstKind::FnAddr(callee), sig);
                f.blocks[b].insts[i + 1].kind =
                    InstKind::Call(release, vec![child, Operand::Value(addr)]);
                i += 2;
            }
        }
    }
}

/// `(caller, callee)` index pairs into `fids` closing a cycle: the
/// edges a depth-first walk from each fn in order meets while the
/// callee is still on the walk's path (a self-call included).
fn back_edges(fids: &[FuncId], module: &Module) -> Vec<(usize, usize)> {
    let index: HashMap<FuncId, usize> = fids.iter().enumerate().map(|(i, f)| (*f, i)).collect();
    let callees: Vec<Vec<usize>> = fids
        .iter()
        .map(|fid| {
            let mut out: Vec<usize> = Vec::new();
            for blk in &module.funcs[fid.0 as usize].blocks {
                for inst in &blk.insts {
                    if let InstKind::Call(c, _) = &inst.kind
                        && let Some(&j) = index.get(c)
                        && !out.contains(&j)
                    {
                        out.push(j);
                    }
                }
            }
            out
        })
        .collect();
    // 0 unvisited, 1 on the current path, 2 finished
    let mut state = vec![0u8; fids.len()];
    let mut back = Vec::new();
    for root in 0..fids.len() {
        if state[root] != 0 {
            continue;
        }
        state[root] = 1;
        let mut path: Vec<(usize, usize)> = vec![(root, 0)];
        while let Some(top) = path.last_mut() {
            let u = top.0;
            let Some(&v) = callees[u].get(top.1) else {
                state[u] = 2;
                path.pop();
                continue;
            };
            top.1 += 1;
            match state[v] {
                0 => {
                    state[v] = 1;
                    path.push((v, 0));
                }
                1 => back.push((u, v)),
                _ => {}
            }
        }
    }
    back
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module_with_calls(edges: &[&[u32]]) -> (Module, Vec<FuncId>) {
        let mut m = Module::default();
        let mut fids = Vec::new();
        for (i, _) in edges.iter().enumerate() {
            let mut f = ssa::Function::new(format!("__drop_{i}"), Type::Void);
            f.add_param(Type::Ptr, "p");
            f.add_block();
            m.funcs.push(f);
            fids.push(FuncId(i as u32));
        }
        for (i, callees) in edges.iter().enumerate() {
            let f = &mut m.funcs[i];
            for c in *callees {
                f.append_void(
                    ssa::BlockId(0),
                    InstKind::Call(FuncId(*c), vec![Operand::ConstPtrNull]),
                );
            }
        }
        (m, fids)
    }

    #[test]
    fn every_cycle_keeps_one_back_edge() {
        // 0 -> 1 -> 2 -> 0, 2 -> 2, 0 -> 3 (acyclic tail)
        let (m, fids) = module_with_calls(&[&[1, 3], &[2], &[0, 2], &[]]);
        let mut back = back_edges(&fids, &m);
        back.sort();
        assert_eq!(back, vec![(2, 0), (2, 2)]);
    }

    #[test]
    fn acyclic_graph_has_no_back_edge() {
        let (m, fids) = module_with_calls(&[&[1, 2], &[2], &[]]);
        assert!(back_edges(&fids, &m).is_empty());
    }

    #[test]
    fn back_edge_call_goes_through_the_release() {
        let (mut m, fids) = module_with_calls(&[&[1], &[0]]);
        let drops: Vec<ClassDropFn> = fids
            .iter()
            .map(|fid| ClassDropFn {
                sid: ssa::StructId(fid.0),
                fid: *fid,
                name: m.funcs[fid.0 as usize].name.clone(),
            })
            .collect();
        let mut sigs = Vec::new();
        route(&drops, &mut sigs, &mut m);
        assert_eq!(m.funcs.len(), 3);
        assert_eq!(m.funcs[2].name, DROP_RELEASE_SYM);
        // 0 -> 1 is the tree edge, 1 -> 0 the back edge
        assert!(matches!(
            m.funcs[0].blocks[0].insts[0].kind,
            InstKind::Call(FuncId(1), _)
        ));
        let insts = &m.funcs[1].blocks[0].insts;
        assert!(matches!(insts[0].kind, InstKind::FnAddr(FuncId(0))));
        assert!(matches!(&insts[1].kind, InstKind::Call(FuncId(2), a) if a.len() == 2));
    }
}
