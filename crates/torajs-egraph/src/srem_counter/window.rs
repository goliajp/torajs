//! The divisor's `p ≥ 1` read off a test on the way to the remainder.

use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::{BinOp, BlockId, IPred, InstKind, Operand, Terminator, ValueId};

use super::Cfg;
use crate::interval::transfer::op_const_int;

/// A branch dominating block `b` that reaches it only when `p ≥ 1`:
/// its passing successor has no other predecessor and dominates `b`,
/// and its condition is either a compare of `p` against 1 (`p < 1`,
/// `p ≤ 0` branching away, `p ≥ 1`, `p > 0` branching in) or `g != 0`
/// branching away with `g` an `or` tree holding `(p - 1) >>> k` for
/// some `2 ≤ k < 64` — so `1 ≤ p ≤ 2^(64-k)`, the window check
/// frem_guard puts in front of its integer arm. Answers `Some(p - 1)`
/// when the test spells it.
pub(super) fn tested_window(
    cfg: &Cfg,
    dom: &DominatorTree,
    b: usize,
    p: ValueId,
) -> Option<Option<ValueId>> {
    // the test and the site must read one number
    let p = root(cfg, p);
    if !cfg.once(p) {
        return None;
    }
    let mut cur = BlockId(b as u32);
    while let Some(d) = dom.immediate_dominator(cur) {
        cur = d;
        let Terminator::CondBr {
            cond: Operand::Value(c),
            then_blk,
            else_blk,
        } = &cfg.func.blocks[d.0 as usize].term
        else {
            continue;
        };
        let Some(InstKind::ICmp(pred, Operand::Value(a), z)) = cfg.one_def(*c) else {
            continue;
        };
        let z = op_const_int(z);
        let (pass, pm1) = match (pred, z) {
            (IPred::Ne, Some(0)) => (else_blk, window_leaf(cfg, *a, p, 8)),
            (IPred::Eq, Some(0)) => (then_blk, window_leaf(cfg, *a, p, 8)),
            (IPred::Slt, Some(1)) | (IPred::Sle, Some(0)) if root(cfg, *a) == p => {
                (else_blk, Some(None))
            }
            (IPred::Sge, Some(1)) | (IPred::Sgt, Some(0)) if root(cfg, *a) == p => {
                (then_blk, Some(None))
            }
            _ => continue,
        };
        if then_blk != else_blk
            && cfg.preds(*pass).len() == 1
            && dom.dominates(*pass, BlockId(b as u32))
            && pm1.is_some()
        {
            return pm1;
        }
    }
    None
}

/// The value `v` copies, followed back through one-def copies of
/// one-def values (or of a parameter) that run outside every loop:
/// each runs at most once per call, so the two hold the same number
/// wherever `v` is defined.
fn root(cfg: &Cfg, mut v: ValueId) -> ValueId {
    for _ in 0..8 {
        if !cfg.once(v) {
            break;
        }
        match cfg.one_def(v) {
            Some(InstKind::Copy(_, Operand::Value(s))) if cfg.once(*s) => v = *s,
            _ => break,
        }
    }
    v
}

/// In the `or` tree under `g`, a leaf `lshr (sub p, 1), k`; answers
/// `Some(Some(sub))`.
fn window_leaf(cfg: &Cfg, g: ValueId, p: ValueId, depth: u32) -> Option<Option<ValueId>> {
    match cfg.one_def(g)? {
        InstKind::BinOp(BinOp::Or, l, r) if depth > 0 => [l, r].into_iter().find_map(|o| match o {
            Operand::Value(v) => window_leaf(cfg, *v, p, depth - 1),
            _ => None,
        }),
        InstKind::BinOp(BinOp::LShr, Operand::Value(s), k)
            if op_const_int(k).is_some_and(|k| (2..64).contains(&k)) =>
        {
            matches!(
                cfg.one_def(*s)?,
                InstKind::BinOp(BinOp::Sub, Operand::Value(a), Operand::ConstI64(1))
                    if root(cfg, *a) == p
            )
            .then_some(Some(*s))
        }
        _ => None,
    }
}
