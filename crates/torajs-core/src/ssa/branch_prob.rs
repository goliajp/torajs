//! Cold-arm prediction — the part of Ball & Larus (PLDI 1993,
//! "Branch Prediction for Free") this compiler can decide
//! mechanically. Feeds [`super::block_freq`].
//!
//! A conditional has a cold arm when the first matching rule says so:
//!
//! 1. **Throw probe** — `icmp ne (load (global_ref THROW_ACTIVE)), 0`:
//!    the throw arm is taken only when an exception is in flight.
//! 2. **Unreachable arm** — an arm ending in `unreachable`, or one that
//!    raises an error: it ends in a branch on the throw probe whose
//!    nothing-thrown side is `unreachable` (the nullish check in front
//!    of a property read).
//! 3. **Header flag guard** — `icmp eq|ne (and (load p +off), MASK), K`
//!    with `off` inside the refcount-and-flags word and `K` a constant,
//!    where exactly one arm calls something: the latin1 / frozen / COW
//!    bit tests and the array element-kind test guarding a runtime slow
//!    path. Object fields start past the 32-byte header, so a user's
//!    `(o.flags & 4) !== 0` never reads as one; nor does a load of a
//!    typed stack slot (`alloca`), which also sits at offset 0.
//! 4. **Length guard** — `icmp ult i, len` / `icmp uge i, len` with
//!    `len` the array length word (`load p +8`, or the register cell
//!    `len_promote` keeps it in across a loop): the out-of-bounds arm
//!    (a grow on a store, `undefined` on a read, the empty arm of a
//!    pop / shift, whose guard is `icmp uge 0, len`). Only bounds
//!    checks compare unsigned; loop conditions use `slt`.
//! 5. **Boxed-value guard** — a compare of `ptrtoint v` (the null test
//!    of a pointer the lowerer loaded) or of its tag bits (`and` with a
//!    mask, `lshr 48`) with a constant, or an `and` / `or` of such
//!    compares, where exactly one arm calls something: is-string /
//!    is-number / both-int tests in front of the generic kernel.
//!    `ptrtoint` comes only from the lowerer: no source construct
//!    exposes a NaN-box's bits.
//! 6. **Int pair test** — an `and` of two compares of a boxed value's
//!    tag (`lshr (ptrtoint v), 48`) with a constant: the test the
//!    lowerer puts in front of arithmetic on two `any` operands, whose
//!    then arm is the int fast path (the small-integer speculation of
//!    every JS engine). The else arm, the generic number / string path,
//!    is cold even though neither arm calls: when it is the one that
//!    runs, its float temporaries cost spill traffic, never a call.
//!    Not when an operand is a call's answer: the generic kernels box
//!    a double whenever an input was not an int (`+s` on a numeric
//!    string), so the else arm is the one that runs.
//!
//! Every other branch predicts nothing, and nothing splits on it: the
//! lowerer emits many conditionals no rule reads (loop versioning on a
//! combination of header bits, fused guards), and splitting those
//! evenly halves the estimate for whichever copy actually runs.
//!
//! Two classic heuristics are deliberately absent. The call heuristic
//! ("an arm that calls is rarely taken") misreads user code such as
//! `if (q.length > 16) q.shift()`, taken on almost every iteration;
//! it applies here only through rules 3–5, where the compare is one
//! the lowerer itself emitted. The opcode heuristic ("equality is
//! usually false") lost 14% on collatz's `n % 2 === 0` when block
//! layout tried it.
//!
//! A guard arm predicted cold but actually hot costs a store and a
//! reload around a call that itself costs far more.

use std::cell::OnceCell;

use crate::ssa::{
    BinOp, BlockId, Function, IPred, InstKind, Operand, THROW_ACTIVE_SYM, Terminator, ValueId,
};

mod defs;
use defs::{Defs, single_defs, value_def};

/// Odds of an arm that runs only when an exception is in flight or
/// the program is about to trap.
pub const P_NEVER: f64 = 1.0 / 1024.0;
/// Odds of a runtime slow path whose trigger is data the program
/// controls (a non-latin1 string, a frozen array, a grow).
pub const P_SLOW_PATH: f64 = 1.0 / 64.0;

/// Byte offset of an array's length word in its heap header.
const ARR_LEN_OFFSET: u64 = 8;

/// Every predicted-cold edge: the branching block, the arm it rarely
/// takes, and the odds of taking it.
pub fn cold_arms(func: &Function) -> Vec<(BlockId, BlockId, f64)> {
    if !(func.blocks.iter()).any(|b| matches!(b.term, Terminator::CondBr { .. })) {
        return Vec::new();
    }
    let defs = single_defs(func);
    // only a bounds check asks, and most functions have none
    let cells = OnceCell::new();
    let in_cell = |v: ValueId| {
        let cells = cells.get_or_init(|| length_cells(func, &defs));
        cells.get(v.0 as usize) == Some(&true)
    };
    let arms = Arms {
        func,
        blocks: positions(func),
        defs: &defs,
    };
    let (calls, ends_unreachable, throws) = (
        |b| arms.calls(b),
        |b| arms.ends_unreachable(b),
        |b| arms.throws(b),
    );

    let mut out = Vec::new();
    for b in &func.blocks {
        let Terminator::CondBr {
            cond,
            then_blk,
            else_blk,
        } = &b.term
        else {
            continue;
        };
        let (t, e) = (*then_blk, *else_blk);
        if t == e {
            continue;
        }
        let cond = value_def(cond, &defs);
        let calling_arm = || if calls(t) { t } else { e };
        // scans both arms: asked last
        let one_calls = || calls(t) != calls(e);
        let arm = if cond.is_some_and(|c| is_throw_probe(c, &defs)) {
            Some((t, P_NEVER))
        } else if ends_unreachable(t) != ends_unreachable(e) {
            Some((if ends_unreachable(t) { t } else { e }, P_NEVER))
        } else if throws(t) != throws(e) {
            Some((if throws(t) { t } else { e }, P_NEVER))
        } else if cond.is_some_and(|c| is_flag_guard(c, &defs)) && one_calls() {
            Some((calling_arm(), P_SLOW_PATH))
        } else if let Some(oob) = cond.and_then(|c| out_of_bounds_arm(c, &defs, &in_cell, t, e)) {
            Some((oob, P_SLOW_PATH))
        } else if cond.is_some_and(|c| is_boxed_guard(c, &defs, 1)) && one_calls() {
            Some((calling_arm(), P_SLOW_PATH))
        } else if cond.is_some_and(|c| is_int_pair_test(c, &defs)) {
            Some((e, P_SLOW_PATH))
        } else {
            None
        };
        if let Some((cold, p)) = arm {
            out.push((b.id, cold, p));
        }
    }
    out
}

/// What the cold-arm rules ask of a branch's target block.
struct Arms<'f, 'd> {
    func: &'f Function,
    /// Each block's position, by id.
    blocks: Vec<usize>,
    defs: &'d Defs<'f>,
}

impl Arms<'_, '_> {
    fn term(&self, b: BlockId) -> &Terminator {
        &self.func.blocks[self.blocks[b.0 as usize]].term
    }

    fn calls(&self, b: BlockId) -> bool {
        self.func.blocks[self.blocks[b.0 as usize]]
            .insts
            .iter()
            .any(|i| {
                matches!(
                    i.kind,
                    InstKind::Call(..) | InstKind::CallIndirect(..) | InstKind::RcDecAny(_)
                )
            })
    }

    fn ends_unreachable(&self, b: BlockId) -> bool {
        matches!(self.term(b), Terminator::Unreachable)
    }

    /// A block that raises an error: its branch on the throw probe goes
    /// on only into `unreachable` when nothing was thrown.
    fn throws(&self, b: BlockId) -> bool {
        match self.term(b) {
            Terminator::CondBr { cond, else_blk, .. } => {
                self.ends_unreachable(*else_blk)
                    && value_def(cond, self.defs).is_some_and(|c| is_throw_probe(c, self.defs))
            }
            _ => false,
        }
    }
}

/// Each block's position, by id.
fn positions(func: &Function) -> Vec<usize> {
    let span = (func.blocks.iter()).map(|b| b.id.0 as usize + 1).max();
    let mut blocks = vec![usize::MAX; span.unwrap_or(0)];
    for (i, b) in func.blocks.iter().enumerate() {
        blocks[b.id.0 as usize] = i;
    }
    blocks
}

fn is_zero(op: &Operand) -> bool {
    matches!(op, Operand::ConstI64(0))
}

/// `icmp ne (load (global_ref THROW_ACTIVE)), 0`.
fn is_throw_probe(cond: &InstKind, defs: &Defs) -> bool {
    let InstKind::ICmp(IPred::Ne, flag, zero) = cond else {
        return false;
    };
    if !is_zero(zero) {
        return false;
    }
    let Some(InstKind::Load(_, addr, 0)) = value_def(flag, defs) else {
        return false;
    };
    matches!(value_def(addr, defs), Some(InstKind::GlobalRef(s)) if s == THROW_ACTIVE_SYM)
}

/// Bytes of the refcount-and-flags word at the head of every heap cell.
const FLAGS_WORD_BYTES: u64 = 8;

/// A constant: an immediate, or the value `iconst_hoist` minted for one.
fn is_const(op: &Operand, defs: &Defs) -> bool {
    matches!(op, Operand::ConstI64(_))
        || matches!(
            value_def(op, defs),
            Some(InstKind::Copy(_, Operand::ConstI64(_)))
        )
}

/// `icmp eq|ne (and (load p +off), MASK), K` — a test of header bits.
fn is_flag_guard(cond: &InstKind, defs: &Defs) -> bool {
    let InstKind::ICmp(IPred::Ne | IPred::Eq, masked, k) = cond else {
        return false;
    };
    // a guard on the 32-bit flags half (a field store's frozen / exotic
    // test) spells its mask and constant as i32
    if !is_const(k, defs) && !matches!(k, Operand::ConstI32(_)) {
        return false;
    }
    let Some(InstKind::BinOp(BinOp::And, word, Operand::ConstI64(_) | Operand::ConstI32(_))) =
        value_def(masked, defs)
    else {
        return false;
    };
    let Some(InstKind::Load(_, p, off)) = value_def(word, defs) else {
        return false;
    };
    // a typed slot mem2reg left in place also loads at offset 0; an
    // `alloca_bytes` may be a stack-promoted cell with a real header
    *off < FLAGS_WORD_BYTES && !matches!(value_def(p, defs), Some(InstKind::Alloca(_)))
}

/// A compare of `ptrtoint v`, or of its tag bits, with a constant; or
/// an `and` / `or` of two such compares (`depth` bounds the nesting).
fn is_boxed_guard(cond: &InstKind, defs: &Defs, depth: u32) -> bool {
    let is_bits = |op: &Operand| matches!(value_def(op, defs), Some(InstKind::PtrToInt(_)));
    match cond {
        InstKind::ICmp(IPred::Eq | IPred::Ne, x, k) if is_const(k, defs) => {
            is_bits(x)
                || matches!(
                    value_def(x, defs),
                    Some(InstKind::BinOp(BinOp::And | BinOp::LShr, w, Operand::ConstI64(_)))
                        if is_bits(w)
                )
        }
        InstKind::BinOp(BinOp::And | BinOp::Or, a, b) if depth > 0 => [a, b]
            .iter()
            .all(|op| value_def(op, defs).is_some_and(|c| is_boxed_guard(c, defs, depth - 1))),
        _ => false,
    }
}

/// `and (icmp eq (lshr (ptrtoint a), 48), K), (icmp eq (lshr (ptrtoint b), 48), K)`.
fn is_int_pair_test(cond: &InstKind, defs: &Defs) -> bool {
    let not_a_call_answer = |w: &Operand| match value_def(w, defs) {
        Some(InstKind::PtrToInt(v)) => !matches!(
            value_def(v, defs),
            Some(InstKind::Call(..) | InstKind::CallIndirect(..))
        ),
        _ => false,
    };
    let tag = |op: &Operand| match value_def(op, defs) {
        Some(InstKind::ICmp(IPred::Eq, x, k)) if is_const(k, defs) => matches!(
            value_def(x, defs),
            Some(InstKind::BinOp(BinOp::LShr, w, Operand::ConstI64(48))) if not_a_call_answer(w)
        ),
        _ => false,
    };
    matches!(cond, InstKind::BinOp(BinOp::And, a, b) if tag(a) && tag(b))
}

fn is_len_load(kind: Option<&InstKind>) -> bool {
    matches!(kind, Some(InstKind::Load(_, _, off)) if *off == ARR_LEN_OFFSET)
}

/// The multi-def values one of whose defs copies a length word: the
/// cell `len_promote` keeps a loop's array length in (its preheader
/// def reads the word; the rest are the loop's length stores and the
/// re-reads after a kernel call).
fn length_cells(func: &Function, defs: &Defs) -> Vec<bool> {
    let mut out = vec![false; defs.0.len()];
    for b in &func.blocks {
        for i in &b.insts {
            if let (Some(r), InstKind::Copy(_, src)) = (i.result, &i.kind)
                && value_def(&Operand::Value(r), defs).is_none()
                && is_len_load(value_def(src, defs))
            {
                out[r.0 as usize] = true;
            }
        }
    }
    out
}

/// The arm an unsigned compare against the array length word takes
/// when the index is out of bounds.
fn out_of_bounds_arm(
    cond: &InstKind,
    defs: &Defs,
    in_cell: &impl Fn(ValueId) -> bool,
    then_blk: BlockId,
    else_blk: BlockId,
) -> Option<BlockId> {
    let InstKind::ICmp(pred @ (IPred::Ult | IPred::Uge), _, len) = cond else {
        return None;
    };
    let in_cell = |op: &Operand| matches!(op, Operand::Value(v) if in_cell(*v));
    let is_len = is_len_load(value_def(len, defs))
        || in_cell(len)
        || matches!(value_def(len, defs), Some(InstKind::Copy(_, c)) if in_cell(c));
    is_len.then_some(if *pred == IPred::Ult {
        else_blk
    } else {
        then_blk
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssa::{Block, FuncId, Inst, Type, ValueId};

    fn func(blocks: Vec<Block>) -> Function {
        Function {
            name: "test".to_string(),
            params: vec![],
            ret: Type::Void,
            blocks,
            values: vec![],
            current_origin: None,
        }
    }

    fn block(id: u32, insts: Vec<Inst>, term: Terminator) -> Block {
        Block {
            id: BlockId(id),
            insts,
            term,
        }
    }

    fn inst(result: u32, kind: InstKind) -> Inst {
        Inst {
            result: Some(ValueId(result)),
            kind,
            origin: None,
        }
    }

    fn call() -> Inst {
        Inst {
            result: None,
            kind: InstKind::Call(FuncId(0), vec![]),
            origin: None,
        }
    }

    fn v(id: u32) -> Operand {
        Operand::Value(ValueId(id))
    }

    fn cond_br(cond: u32, then_blk: u32, else_blk: u32) -> Terminator {
        Terminator::CondBr {
            cond: v(cond),
            then_blk: BlockId(then_blk),
            else_blk: BlockId(else_blk),
        }
    }

    /// A field store's frozen / exotic test reads the 32-bit flags half
    /// and spells its mask and constant as i32.
    #[test]
    fn an_i32_flag_guard_predicts_its_calling_arm_cold() {
        let guard = vec![
            inst(1, InstKind::Load(Type::I32, v(0), 4)),
            inst(
                2,
                InstKind::BinOp(BinOp::And, v(1), Operand::ConstI32(-2146435072)),
            ),
            inst(3, InstKind::ICmp(IPred::Ne, v(2), Operand::ConstI32(0))),
        ];
        let f = func(vec![
            block(0, guard, cond_br(3, 2, 1)),
            block(1, vec![], Terminator::Br(BlockId(3))),
            block(2, vec![call()], Terminator::Br(BlockId(3))),
            block(3, vec![], Terminator::Ret(None)),
        ]);
        assert_eq!(cold_arms(&f), vec![(BlockId(0), BlockId(2), P_SLOW_PATH)]);
    }

    /// The nullish check in front of a property read: the arm calls the
    /// raising helper, branches on the throw probe, and traps when
    /// nothing was thrown.
    #[test]
    fn an_arm_that_raises_is_cold() {
        let test = vec![inst(
            1,
            InstKind::ICmp(IPred::Eq, v(0), Operand::ConstPtrNull),
        )];
        let raise = vec![
            call(),
            inst(2, InstKind::GlobalRef(THROW_ACTIVE_SYM.to_string())),
            inst(3, InstKind::Load(Type::I64, v(2), 0)),
            inst(4, InstKind::ICmp(IPred::Ne, v(3), Operand::ConstI64(0))),
        ];
        let f = func(vec![
            block(0, test, cond_br(1, 1, 2)),
            block(1, raise, cond_br(4, 3, 4)),
            block(2, vec![], Terminator::Ret(None)),
            block(3, vec![], Terminator::Ret(None)),
            block(4, vec![], Terminator::Unreachable),
        ]);
        assert!(cold_arms(&f).contains(&(BlockId(0), BlockId(1), P_NEVER)));
    }
}
