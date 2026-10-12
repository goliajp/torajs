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
//! The view kernels (`__torajs_substr_concat_str_substr` and its
//! siblings) are concats too: a Str left operand dropped behind one
//! becomes `__torajs_str_append_substr`, and they sit inside a window
//! like any other string kernel. Once the pairs are rewritten,
//! `chain` flattens a chain whose result is only appended onto
//! another cell and dropped, so the intermediate cell is never built,
//! and `literal` then hands every short static operand to its kernel
//! by value. Last, `reserve` seeds a loop accumulator that starts as
//! `''` and stays in its frame with an empty cell that has room.
//!
//! Runs late, after the inliner and the egraph loop, so the pairs
//! spliced in from inlined callee bodies are visible too.
//! `TORAJS_STR_APPEND_OFF=1` skips it.

// chain flattening, the literal forms and the kernel table live
// beside this file
mod chain;
pub(crate) mod kernels;
pub(crate) mod literal;
mod reserve;
#[cfg(test)]
pub(crate) mod test_support;

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{
    Function, Inst, InstKind, Module, Operand, StringId, Terminator, ValueId, visit_value_operands,
};

use kernels::{Kernels, Kind};
use literal::LiteralForms;

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
    /// Chains whose pieces went straight onto the cell they were
    /// appended to.
    pub flattened: u32,
    /// Kernel calls whose short static operand became immediates.
    pub by_value: u32,
    /// Loop accumulators seeded with a reserved cell instead of `''`.
    pub reserved: u32,
}

/// Rewrite every `concat` + `drop-left` pair in the module, flatten
/// the chains that only feed an append, then pass short static
/// operands by value.
pub fn rewrite_str_appends(module: &mut Module) -> StrAppendStats {
    let mut stats = StrAppendStats::default();
    let Some(kernels) = Kernels::find(module) else {
        return stats; // program concatenates no strings at all
    };
    let forms = LiteralForms::find(module);
    for func in module.funcs.iter_mut() {
        if func.is_declaration() {
            continue;
        }
        let statics = static_strings(func);
        for block in func.blocks.iter_mut() {
            rewrite_pairs(&mut block.insts, &kernels, &statics, &mut stats);
        }
        let mut uses = use_counts(func);
        for block in func.blocks.iter_mut() {
            stats.flattened += chain::flatten_block(block, &mut func.values, &kernels, &mut uses);
        }
        stats.by_value += forms.apply(func, &kernels, &statics);
    }
    stats.reserved = reserve::reserve_accumulators(module);
    stats
}

fn rewrite_pairs(
    insts: &mut Vec<Inst>,
    kernels: &Kernels,
    statics: &HashMap<ValueId, StringId>,
    stats: &mut StrAppendStats,
) {
    let mut i = 0;
    while i < insts.len() {
        let Some((right, left)) = borrowed_str_left(&insts[i], kernels) else {
            i += 1;
            continue;
        };
        let Some(append) = kernels.flavor(Kind::Str, true, right) else {
            i += 1;
            continue;
        };
        match closing_drop(insts, i, &left, kernels, statics) {
            Some(at) => {
                if let InstKind::Call(fid, _) = &mut insts[i].kind {
                    *fid = append;
                }
                insts.remove(at);
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

/// Values whose only definition is one static string — immortal cells
/// no binding's buffer can be — with the literal each names.
fn static_strings(func: &Function) -> HashMap<ValueId, StringId> {
    let mut statics = HashMap::default();
    let mut others = HashSet::default();
    for inst in func.blocks.iter().flat_map(|b| b.insts.iter()) {
        let Some(result) = inst.result else { continue };
        match inst.kind {
            InstKind::StaticStrRef(sid) => {
                if statics.insert(result, sid).is_some_and(|prev| prev != sid) {
                    others.insert(result);
                }
            }
            _ => {
                others.insert(result);
            }
        }
    }
    statics.retain(|v, _| !others.contains(v));
    statics
}

/// Operand occurrences per value, terminators included.
fn use_counts(func: &Function) -> HashMap<ValueId, u32> {
    let mut uses: HashMap<ValueId, u32> = HashMap::default();
    for block in &func.blocks {
        for inst in &block.insts {
            visit_value_operands(&inst.kind, |v| *uses.entry(v).or_insert(0) += 1);
        }
        visit_terminator_values(&block.term, |v| *uses.entry(v).or_insert(0) += 1);
    }
    uses
}

/// The value a terminator reads, if any.
fn visit_terminator_values(term: &Terminator, mut f: impl FnMut(ValueId)) {
    match term {
        Terminator::CondBr {
            cond: Operand::Value(v),
            ..
        }
        | Terminator::Ret(Some(Operand::Value(v))) => f(*v),
        _ => {}
    }
}

/// Index of the `str_drop` of `left` that follows the concat at `at`,
/// when every instruction between the two is a string kernel call or a
/// static string, and touches nothing but values defined inside the
/// window and static strings.
fn closing_drop(
    insts: &[Inst],
    at: usize,
    left: &Operand,
    kernels: &Kernels,
    statics: &HashMap<ValueId, StringId>,
) -> Option<usize> {
    let Operand::Value(left_id) = *left else {
        return None;
    };
    let mut fresh: HashSet<ValueId> = insts[at].result.into_iter().collect();
    for (j, inst) in insts.iter().enumerate().skip(at + 1) {
        if drops_value(inst, kernels, left) {
            return Some(j);
        }
        match &inst.kind {
            InstKind::StaticStrRef(_) => {}
            InstKind::Call(fid, _) if kernels.is_kernel(*fid) => {}
            _ => return None,
        }
        if inst.result == Some(left_id) {
            return None;
        }
        let mut foreign = false;
        visit_value_operands(&inst.kind, |v| {
            foreign |= !fresh.contains(&v) && !statics.contains_key(&v);
        });
        if foreign {
            return None;
        }
        fresh.extend(inst.result);
    }
    None
}

/// The right operand's kind and the left operand of `inst` when it
/// is a concat that borrows a Str left operand.
fn borrowed_str_left(inst: &Inst, kernels: &Kernels) -> Option<(Kind, Operand)> {
    let (k, args) = kernels.call(inst)?;
    (k.left == Kind::Str && !k.consumes_left).then(|| (k.right, args[0]))
}

/// True when `inst` is a result-less `str_drop` of exactly `left`.
/// Result-less matters: removing an instruction that defines a value
/// would orphan its uses.
fn drops_value(inst: &Inst, kernels: &Kernels, left: &Operand) -> bool {
    matches!(&inst.kind, InstKind::Call(fid, _) if *fid == kernels.str_drop)
        && kernels.dropped(inst) == Some(left)
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;
    use torajs_core::ssa::{FuncId, Type};

    fn body(m: &Module) -> &[Inst] {
        &m.funcs[BODY].blocks[0].insts
    }

    fn callee(inst: &Inst) -> Option<FuncId> {
        match &inst.kind {
            InstKind::Call(f, _) => Some(*f),
            _ => None,
        }
    }

    #[test]
    fn an_adjacent_drop_of_the_left_operand_becomes_an_append() {
        let mut m = module_with(vec![concat(4, 2, 3), drop_of(2)]);
        let stats = rewrite_str_appends(&mut m);
        assert_eq!(stats.rewritten, 1);
        assert_eq!(stats.left_borrowed, 0);
        assert_eq!(body(&m).len(), 1, "the drop moved into the call");
        assert_eq!(callee(&body(&m)[0]), Some(STR_APPEND));
    }

    #[test]
    fn a_drop_of_the_right_operand_is_not_the_pair() {
        let mut m = module_with(vec![concat(4, 2, 3), drop_of(3)]);
        let stats = rewrite_str_appends(&mut m);
        assert_eq!(stats.rewritten, 0);
        assert_eq!(stats.left_borrowed, 1);
        assert_eq!(body(&m).len(), 2);
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
        assert_eq!(body(&m).len(), 3, "both drops moved into their calls");
        assert_eq!(callee(&body(&m)[0]), Some(STR_APPEND));
        assert_eq!(callee(&body(&m)[2]), Some(STR_APPEND));
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
        assert_eq!(callee(&body(&m)[0]), Some(STR_CONCAT));
        let kernels = Kernels::find(&m).unwrap();
        assert!(body(&m).iter().any(|i| drops_value(i, &kernels, &v(2))));
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
        assert_eq!(callee(&body(&m)[0]), Some(STR_CONCAT));
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
        assert_eq!(body(&m).len(), 3);
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
        assert_eq!(body(&m).len(), 2);
    }

    #[test]
    fn a_module_without_the_append_kernel_is_left_alone() {
        let mut m = module_with(vec![concat(4, 2, 3), drop_of(2)]);
        m.funcs[STR_APPEND.0 as usize].name = "__torajs_something_else".into();
        let stats = rewrite_str_appends(&mut m);
        assert_eq!(stats.rewritten, 0);
        assert_eq!(body(&m).len(), 2);
    }

    #[test]
    fn a_str_left_operand_dropped_behind_a_view_concat_becomes_a_view_append() {
        let mut m = module_with(vec![call(SUBSTR_CONCAT_STR_SUBSTR, 4, &[2, 3]), drop_of(2)]);
        let stats = rewrite_str_appends(&mut m);
        assert_eq!(stats.rewritten, 1);
        assert_eq!(calls(&m, 0), vec![(STR_APPEND_SUBSTR, Some(4), vec![2, 3])]);
    }

    #[test]
    fn a_view_left_operand_has_no_in_place_form() {
        let mut m = module_with(vec![
            call(SUBSTR_CONCAT_SUBSTR_STR, 4, &[2, 3]),
            Inst {
                result: None,
                kind: InstKind::Call(SUBSTR_DROP, vec![v(2)]),
                origin: None,
            },
        ]);
        let stats = rewrite_str_appends(&mut m);
        assert_eq!(stats.rewritten, 0);
        assert_eq!(stats.left_borrowed, 0, "not a candidate at all");
        assert_eq!(body(&m).len(), 2);
    }

    #[test]
    fn a_view_read_in_the_window_is_foreign_like_any_other_operand() {
        // `s = s + a + view`: the inner pair folds onto the view
        // append, the outer window closes on the view it reads
        let mut m = module_with(vec![
            concat(4, 2, 3),
            call(SUBSTR_CONCAT_STR_SUBSTR, 5, &[4, 6]),
            drop_of(4),
            drop_of(2),
        ]);
        let stats = rewrite_str_appends(&mut m);
        assert_eq!(stats.rewritten, 1);
        assert_eq!(stats.left_borrowed, 1);
        assert_eq!(callee(&body(&m)[0]), Some(STR_CONCAT));
        assert_eq!(callee(&body(&m)[1]), Some(STR_APPEND_SUBSTR));
    }
}
