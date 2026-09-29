//! `concat` + `drop-left` → `append` — hand the left operand of a
//! string concat to the kernel that may grow it in place.
//!
//! `ssa_lower` emits string `+` as a borrow of both operands, so the
//! kernel behind it must mint a fresh cell and copy the whole left
//! side in. Where the left operand is a temp the fold owns, or the
//! old value of the very binding being reassigned, the emitted
//! stream drops it on the next instruction:
//!
//! ```text
//! %232 = call __torajs_str_concat(%252, %225)
//! call __torajs_str_drop(%252)
//! ```
//!
//! That pair says something `concat` alone cannot see: the caller is
//! finished with `%252`. `__torajs_str_append` takes that reference
//! instead of borrowing it, which lets it ask whether anyone else
//! holds the cell and, when nobody does, write into its slack rather
//! than reallocate. The `acc = acc + piece` loop stops being
//! quadratic.
//!
//! Soundness — the rewrite moves exactly one reference release from
//! the caller into the callee, so the refcount arithmetic is
//! unchanged. Two things make that safe rather than merely equal:
//!
//! * **A closed window.** Between the concat and the drop of its left
//!   operand only the string kernels themselves may run, and only on
//!   values that cannot be the left operand's cell under another
//!   name: values the window itself defined, and static strings.
//!   That is the `s = s + a + b` shape, where the rest of the chain
//!   sits between the first concat and the release of the old `s`.
//!   Anything else could read the cell (or an un-inc'd copy of the
//!   same pointer — the block-tail `copy` this IR uses for
//!   loop-carried bindings makes those real), and an in-place append
//!   would show it the appended bytes early.
//! * **Nothing may read the left operand afterwards.** Guaranteed by
//!   the program that was already there: past its own `drop` the
//!   cell may already be freed, so any later read was invalid before
//!   this pass existed. When the cell turns out to be shared the
//!   kernel falls back to the old sequence and the answer is a
//!   fresh cell, exactly as before.
//!
//! `s + s` is the one shape adjacency cannot rule out — two SSA
//! values may carry one pointer — so the kernel pointer-compares its
//! operands and declines. This pass does not try to prove it.
//!
//! Runs late, after the inliner and the egraph loop, so the pairs
//! spliced in from inlined callee bodies are visible too.
//! `TORAJS_STR_APPEND_OFF=1` skips it.

use std::collections::HashSet;

use torajs_core::ssa::{
    FuncId, Function, Inst, InstKind, Module, Operand, ValueId, visit_value_operands,
};

const STR_CONCAT: &str = "__torajs_str_concat";
const STR_DROP: &str = "__torajs_str_drop";
const STR_APPEND: &str = "__torajs_str_append";

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct StrAppendStats {
    /// `concat` calls rewritten to `append` (each also deletes one
    /// `str_drop`).
    pub rewritten: u32,
    /// `concat` calls left alone because no `str_drop` of the left
    /// operand closed a window behind them.
    pub left_borrowed: u32,
    /// Of `rewritten`, the pairs that had other kernel calls between
    /// them.
    pub across_window: u32,
}

/// Rewrite every `concat` + `drop-left` pair in the module.
pub fn rewrite_str_appends(module: &mut Module) -> StrAppendStats {
    let mut stats = StrAppendStats::default();
    let Some(concat_fid) = find_func(module, STR_CONCAT) else {
        return stats; // program concatenates no strings at all
    };
    let (Some(drop_fid), Some(append_fid)) =
        (find_func(module, STR_DROP), find_func(module, STR_APPEND))
    else {
        return stats;
    };
    let kernels = [concat_fid, drop_fid, append_fid];
    for func in module.funcs.iter_mut() {
        if func.is_declaration() {
            continue;
        }
        let statics = static_strings(func);
        for block in func.blocks.iter_mut() {
            let mut i = 0;
            while i < block.insts.len() {
                let Some(left) = concat_left(&block.insts[i], concat_fid) else {
                    i += 1;
                    continue;
                };
                match closing_drop(&block.insts, i, &left, drop_fid, &kernels, &statics) {
                    Some(at) => {
                        if let InstKind::Call(fid, _) = &mut block.insts[i].kind {
                            *fid = append_fid;
                        }
                        block.insts.remove(at);
                        stats.rewritten += 1;
                        if at > i + 1 {
                            stats.across_window += 1;
                        }
                    }
                    None => stats.left_borrowed += 1,
                }
                i += 1;
            }
        }
    }
    stats
}

/// Values whose only definition is a static string: immortal cells no
/// binding's buffer can be.
fn static_strings(func: &Function) -> HashSet<ValueId> {
    let mut statics = HashSet::new();
    let mut others = HashSet::new();
    for inst in func.blocks.iter().flat_map(|b| b.insts.iter()) {
        let Some(result) = inst.result else { continue };
        if matches!(inst.kind, InstKind::StaticStrRef(_)) {
            statics.insert(result);
        } else {
            others.insert(result);
        }
    }
    statics.retain(|v| !others.contains(v));
    statics
}

/// Index of the `str_drop` of `left` that follows the concat at `at`,
/// when every instruction between the two is a string kernel call or a
/// static string, and touches nothing but values defined inside the
/// window and static strings.
fn closing_drop(
    insts: &[Inst],
    at: usize,
    left: &Operand,
    drop_fid: FuncId,
    kernels: &[FuncId],
    statics: &HashSet<ValueId>,
) -> Option<usize> {
    let Operand::Value(left_id) = *left else {
        return None;
    };
    let mut fresh: HashSet<ValueId> = insts[at].result.into_iter().collect();
    for (j, inst) in insts.iter().enumerate().skip(at + 1) {
        if drops_value(inst, drop_fid, left) {
            return Some(j);
        }
        match &inst.kind {
            InstKind::StaticStrRef(_) => {}
            InstKind::Call(fid, _) if kernels.contains(fid) => {}
            _ => return None,
        }
        if inst.result == Some(left_id) {
            return None;
        }
        let mut foreign = false;
        visit_value_operands(&inst.kind, |v| {
            foreign |= !fresh.contains(&v) && !statics.contains(&v);
        });
        if foreign {
            return None;
        }
        fresh.extend(inst.result);
    }
    None
}

fn find_func(module: &Module, name: &str) -> Option<FuncId> {
    module
        .funcs
        .iter()
        .position(|f| f.name == name)
        .map(|i| FuncId(i as u32))
}

/// The left operand of `inst` when it is a two-argument call to the
/// concat kernel.
fn concat_left(inst: &Inst, concat_fid: FuncId) -> Option<Operand> {
    match &inst.kind {
        InstKind::Call(fid, args) if *fid == concat_fid && args.len() == 2 => Some(args[0].clone()),
        _ => None,
    }
}

/// True when `inst` is a result-less `str_drop` of exactly `left`.
/// Result-less matters: removing an instruction that defines a value
/// would orphan its uses.
fn drops_value(inst: &Inst, drop_fid: FuncId, left: &Operand) -> bool {
    inst.result.is_none()
        && matches!(&inst.kind, InstKind::Call(fid, args)
            if *fid == drop_fid && args.len() == 1 && args[0] == *left)
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{Block, BlockId, StringId, Terminator, Type, ValueInfo};

    fn v(id: u32) -> Operand {
        Operand::Value(ValueId(id))
    }

    fn decl(name: &str) -> Function {
        Function {
            name: name.into(),
            params: vec![],
            ret: Type::Str,
            blocks: vec![],
            values: vec![],
            current_origin: None,
        }
    }

    /// Module whose function 0/1/2 are the three kernels this pass
    /// keys on, plus a body holding `insts`.
    fn module_with(insts: Vec<Inst>) -> Module {
        let values = (0..8)
            .map(|_| ValueInfo {
                ty: Type::Str,
                name: None,
            })
            .collect();
        let body = Function {
            name: "f".into(),
            params: vec![],
            ret: Type::Str,
            blocks: vec![Block {
                id: BlockId(0),
                insts,
                term: Terminator::Ret(None),
            }],
            values,
            current_origin: None,
        };
        Module {
            funcs: vec![decl(STR_CONCAT), decl(STR_DROP), decl(STR_APPEND), body],
            ..Module::default()
        }
    }

    fn concat(result: u32, left: u32, right: u32) -> Inst {
        Inst {
            result: Some(ValueId(result)),
            kind: InstKind::Call(FuncId(0), vec![v(left), v(right)]),
            origin: None,
        }
    }

    fn drop_of(x: u32) -> Inst {
        Inst {
            result: None,
            kind: InstKind::Call(FuncId(1), vec![v(x)]),
            origin: None,
        }
    }

    #[test]
    fn an_adjacent_drop_of_the_left_operand_becomes_an_append() {
        let mut m = module_with(vec![concat(4, 2, 3), drop_of(2)]);
        let stats = rewrite_str_appends(&mut m);
        assert_eq!(stats.rewritten, 1);
        assert_eq!(stats.left_borrowed, 0);
        let insts = &m.funcs[3].blocks[0].insts;
        assert_eq!(insts.len(), 1, "the drop moved into the call");
        assert!(matches!(&insts[0].kind, InstKind::Call(f, _) if *f == FuncId(2)));
    }

    #[test]
    fn a_drop_of_the_right_operand_is_not_the_pair() {
        let mut m = module_with(vec![concat(4, 2, 3), drop_of(3)]);
        let stats = rewrite_str_appends(&mut m);
        assert_eq!(stats.rewritten, 0);
        assert_eq!(stats.left_borrowed, 1);
        assert_eq!(m.funcs[3].blocks[0].insts.len(), 2);
    }

    fn static_str(result: u32) -> Inst {
        Inst {
            result: Some(ValueId(result)),
            kind: InstKind::StaticStrRef(StringId(0)),
            origin: None,
        }
    }

    #[test]
    fn the_rest_of_a_chain_may_sit_between_the_concat_and_the_drop() {
        // `s = s + a + b` with `b` a literal: the second link and the
        // release of its temp run before the old `s` is released.
        let mut m = module_with(vec![
            concat(4, 2, 3),
            static_str(6),
            concat(5, 4, 6),
            drop_of(4),
            drop_of(2),
        ]);
        let stats = rewrite_str_appends(&mut m);
        assert_eq!(stats.rewritten, 2);
        assert_eq!(stats.across_window, 1);
        let insts = &m.funcs[3].blocks[0].insts;
        assert_eq!(insts.len(), 3, "both drops moved into their calls");
        assert!(matches!(&insts[0].kind, InstKind::Call(f, _) if *f == FuncId(2)));
        assert!(matches!(&insts[2].kind, InstKind::Call(f, _) if *f == FuncId(2)));
    }

    #[test]
    fn a_static_string_defined_before_the_concat_may_be_read_in_the_window() {
        let mut m = module_with(vec![
            static_str(6),
            concat(4, 2, 3),
            concat(5, 4, 6),
            drop_of(4),
            drop_of(2),
        ]);
        assert_eq!(rewrite_str_appends(&mut m).rewritten, 2);
    }

    #[test]
    fn a_value_from_before_the_concat_read_in_the_window_closes_it() {
        // value 3 may be the left operand's cell under another name
        let mut m = module_with(vec![
            concat(4, 2, 3),
            concat(5, 4, 3),
            drop_of(4),
            drop_of(2),
        ]);
        let stats = rewrite_str_appends(&mut m);
        assert_eq!(stats.rewritten, 1, "only the inner, adjacent pair");
        let insts = &m.funcs[3].blocks[0].insts;
        assert!(matches!(&insts[0].kind, InstKind::Call(f, _) if *f == FuncId(0)));
        assert!(insts.iter().any(|i| drops_value(i, FuncId(1), &v(2))));
    }

    #[test]
    fn the_left_operand_read_again_in_the_window_closes_it() {
        // `s = s + a + s`
        let mut m = module_with(vec![
            concat(4, 2, 3),
            concat(5, 4, 2),
            drop_of(4),
            drop_of(2),
        ]);
        let stats = rewrite_str_appends(&mut m);
        assert_eq!(stats.rewritten, 1);
        assert!(matches!(
            &m.funcs[3].blocks[0].insts[0].kind,
            InstKind::Call(f, _) if *f == FuncId(0)
        ));
    }

    #[test]
    fn an_instruction_between_the_two_closes_the_window() {
        // Anything but a string kernel in the gap may observe the
        // cell — including a block-tail `copy` carrying the same
        // pointer under another name.
        let between = Inst {
            result: Some(ValueId(5)),
            kind: InstKind::Copy(Type::Str, v(2)),
            origin: None,
        };
        let mut m = module_with(vec![concat(4, 2, 3), between, drop_of(2)]);
        let stats = rewrite_str_appends(&mut m);
        assert_eq!(stats.rewritten, 0);
        assert_eq!(m.funcs[3].blocks[0].insts.len(), 3);
    }

    #[test]
    fn a_chain_of_folds_rewrites_every_link() {
        // `a + b + c` — the fold's intermediate is a temp it owns, so
        // each round drops its own left operand.
        let mut m = module_with(vec![
            concat(4, 1, 2),
            drop_of(1),
            concat(5, 4, 3),
            drop_of(4),
        ]);
        let stats = rewrite_str_appends(&mut m);
        assert_eq!(stats.rewritten, 2);
        assert_eq!(m.funcs[3].blocks[0].insts.len(), 2);
    }

    #[test]
    fn a_module_without_the_append_kernel_is_left_alone() {
        let mut m = module_with(vec![concat(4, 2, 3), drop_of(2)]);
        m.funcs[2].name = "__torajs_something_else".into();
        let stats = rewrite_str_appends(&mut m);
        assert_eq!(stats.rewritten, 0);
        assert_eq!(m.funcs[3].blocks[0].insts.len(), 2);
    }
}
