//! `to_str` + `concat` + `drop` → `concat_num` — fuse a number's
//! string conversion into the concat that is its only reader.
//!
//! Both spellings of "prefix plus a number" — the explicit
//! `lit + i.toString()` and the implicit `lit + i` coerce — lower
//! to the same three adjacent instructions:
//!
//! ```text
//! %t = call __torajs_i64_to_str(%n)
//! %r = call __torajs_str_concat(%a, %t)
//! call __torajs_str_drop(%t)
//! ```
//!
//! The middle Str cell exists for exactly one instruction, yet the
//! program pays its full alloc + digit copy + drop round-trip. The
//! rewrite hands `%n` to `__torajs_str_concat_i64(%a, %n)` (f64
//! mirror likewise), which formats the digits into a stack buffer
//! and writes them straight into the single result allocation.
//!
//! Soundness — the strict three-in-a-row window is the proof:
//!
//! * Nothing reads `%t` between its mint and its drop (there is no
//!   instruction between them), so deleting the cell is invisible
//!   to every other value.
//! * A use of `%t` after its `drop` was a read of a freed cell
//!   before this pass existed — the same argument
//!   [`crate::str_append`] leans on.
//! * `%t` cannot alias `%a`: it is the result of the `to_str` call
//!   one instruction earlier, which returns a fresh cell.
//!
//! Runs before `str_append` — this fusion is the more specific
//! shape (it deletes a whole cell round-trip, not just a copy), and
//! a number-on-the-left `%t` would otherwise be claimed as that
//! pass's drop-left operand. `TORAJS_CONCAT_NUM_FUSE_OFF=1` skips.

use torajs_core::ssa::{FuncId, Inst, InstKind, Module, Operand, ValueId};

const I64_TO_STR: &str = "__torajs_i64_to_str";
const F64_TO_STR: &str = "__torajs_f64_to_str";
const STR_CONCAT: &str = "__torajs_str_concat";
const STR_DROP: &str = "__torajs_str_drop";
const CONCAT_I64: &str = "__torajs_str_concat_i64";
const CONCAT_F64: &str = "__torajs_str_concat_f64";

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ConcatNumFuseStats {
    /// i64 triples rewritten onto `__torajs_str_concat_i64`.
    pub rewritten_i64: u32,
    /// f64 triples rewritten onto `__torajs_str_concat_f64`.
    pub rewritten_f64: u32,
    /// `to_str` results that did not sit in the exact window
    /// (different consumer, number on the left, non-adjacent drop).
    pub near_miss: u32,
}

/// Rewrite every strict `to_str` + `concat`(right) + `drop` triple
/// in the module.
pub fn fuse_concat_nums(module: &mut Module) -> ConcatNumFuseStats {
    let mut stats = ConcatNumFuseStats::default();
    let (Some(concat_fid), Some(drop_fid)) =
        (find_func(module, STR_CONCAT), find_func(module, STR_DROP))
    else {
        return stats;
    };
    let i64_pair = find_func(module, I64_TO_STR).zip(find_func(module, CONCAT_I64));
    let f64_pair = find_func(module, F64_TO_STR).zip(find_func(module, CONCAT_F64));
    if i64_pair.is_none() && f64_pair.is_none() {
        return stats;
    }
    for func in module.funcs.iter_mut() {
        if func.is_declaration() {
            continue;
        }
        for block in func.blocks.iter_mut() {
            let mut i = 0;
            while i < block.insts.len() {
                let lane = to_str_result(&block.insts[i], i64_pair, f64_pair);
                let Some((t, n, target_fid, is_i64)) = lane else {
                    i += 1;
                    continue;
                };
                let concat_ok = block
                    .insts
                    .get(i + 1)
                    .and_then(|inst| concat_right_of(inst, concat_fid, t))
                    .is_some();
                let drop_ok = block
                    .insts
                    .get(i + 2)
                    .is_some_and(|inst| drops_value(inst, drop_fid, t));
                if !(concat_ok && drop_ok) {
                    stats.near_miss += 1;
                    i += 1;
                    continue;
                }
                let InstKind::Call(fid, args) = &mut block.insts[i + 1].kind else {
                    unreachable!("concat_right_of matched a Call");
                };
                *fid = target_fid;
                args[1] = n;
                block.insts.remove(i + 2);
                block.insts.remove(i);
                if is_i64 {
                    stats.rewritten_i64 += 1;
                } else {
                    stats.rewritten_f64 += 1;
                }
                // The rewritten concat now sits at index `i`; step
                // past it.
                i += 1;
            }
        }
    }
    stats
}

fn find_func(module: &Module, name: &str) -> Option<FuncId> {
    module
        .funcs
        .iter()
        .position(|f| f.name == name)
        .map(|i| FuncId(i as u32))
}

/// `(result, number operand, fused target, is_i64)` when `inst` is
/// a one-argument call to either `to_str` kernel with a result.
fn to_str_result(
    inst: &Inst,
    i64_pair: Option<(FuncId, FuncId)>,
    f64_pair: Option<(FuncId, FuncId)>,
) -> Option<(ValueId, Operand, FuncId, bool)> {
    let InstKind::Call(fid, args) = &inst.kind else {
        return None;
    };
    if args.len() != 1 {
        return None;
    }
    let t = inst.result?;
    if let Some((to_str, target)) = i64_pair
        && *fid == to_str
    {
        return Some((t, args[0].clone(), target, true));
    }
    if let Some((to_str, target)) = f64_pair
        && *fid == to_str
    {
        return Some((t, args[0].clone(), target, false));
    }
    None
}

/// `Some(())` when `inst` is a two-argument concat whose RIGHT
/// operand is exactly `t` and whose left is not (the `s + s`
/// aliasing guard `str_append` needs does not arise here — the
/// left side of a fresh `to_str` result can only be another value
/// — but the check is one comparison, so keep it).
fn concat_right_of(inst: &Inst, concat_fid: FuncId, t: ValueId) -> Option<()> {
    match &inst.kind {
        InstKind::Call(fid, args)
            if *fid == concat_fid
                && args.len() == 2
                && args[1] == Operand::Value(t)
                && args[0] != Operand::Value(t) =>
        {
            Some(())
        }
        _ => None,
    }
}

/// True when `inst` is a result-less `str_drop` of exactly `t`.
fn drops_value(inst: &Inst, drop_fid: FuncId, t: ValueId) -> bool {
    inst.result.is_none()
        && matches!(&inst.kind, InstKind::Call(fid, args)
            if *fid == drop_fid && args.len() == 1 && args[0] == Operand::Value(t))
}
