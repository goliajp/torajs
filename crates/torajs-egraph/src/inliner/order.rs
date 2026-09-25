//! Bottom-up caller order — the call-graph postorder LLVM's CGSCC
//! inliner walks. A caller is visited only after every function it
//! calls directly (within a recursion cycle, in DFS finish order), so
//! when it clones a callee it gets the callee's body with that
//! callee's own sites already inlined. Walking in module order instead
//! let `scale` clone `__new_Vec2` before `__new_Vec2` had inlined its
//! constructor: the ctor call reached `main` through `scale` and kept
//! the instance on the heap.

use torajs_core::ssa::{InstKind, Module};

/// Every function index, callees before callers. Roots are taken in
/// module order, so the result is deterministic.
pub(super) fn bottom_up(module: &Module) -> Vec<usize> {
    let n = module.funcs.len();
    let callees: Vec<Vec<usize>> = module
        .funcs
        .iter()
        .map(|f| {
            let mut out = Vec::new();
            for blk in &f.blocks {
                for inst in &blk.insts {
                    if let InstKind::Call(fid, _) = &inst.kind {
                        out.push(fid.0 as usize);
                    }
                }
            }
            out
        })
        .collect();
    let mut seen = vec![false; n];
    let mut order = Vec::with_capacity(n);
    // (function, index of the next callee to descend into)
    let mut stack: Vec<(usize, usize)> = Vec::new();
    for root in 0..n {
        if seen[root] {
            continue;
        }
        seen[root] = true;
        stack.push((root, 0));
        while let Some(top) = stack.last_mut() {
            let (f, next) = *top;
            if let Some(&c) = callees[f].get(next) {
                top.1 += 1;
                if c < n && !seen[c] {
                    seen[c] = true;
                    stack.push((c, 0));
                }
            } else {
                order.push(f);
                stack.pop();
            }
        }
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{Block, BlockId, FuncId, Function, Inst, Terminator, Type};

    fn calling(name: &str, callees: &[u32]) -> Function {
        let insts = callees
            .iter()
            .map(|c| Inst {
                result: None,
                kind: InstKind::Call(FuncId(*c), vec![]),
                origin: None,
            })
            .collect();
        Function {
            name: name.into(),
            params: vec![],
            ret: Type::Void,
            blocks: vec![Block {
                id: BlockId(0),
                insts,
                term: Terminator::Ret(None),
            }],
            values: vec![],
            current_origin: None,
        }
    }

    #[test]
    fn callees_come_before_their_callers() {
        // 0 scale → 1 new → 2 ctor; 3 main → {0, 1}; 4 self-recursive
        let m = Module {
            funcs: vec![
                calling("scale", &[1]),
                calling("new", &[2]),
                calling("ctor", &[]),
                calling("main", &[0, 1]),
                calling("rec", &[4]),
            ],
            ..Default::default()
        };
        let order = bottom_up(&m);
        let pos = |f: usize| order.iter().position(|&x| x == f).unwrap();
        assert_eq!(order.len(), 5);
        assert!(pos(2) < pos(1) && pos(1) < pos(0) && pos(0) < pos(3));
        assert!(order.contains(&4));
    }
}
