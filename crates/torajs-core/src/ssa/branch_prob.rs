//! Cold-arm prediction — the part of Ball & Larus (PLDI 1993,
//! "Branch Prediction for Free") this compiler can decide
//! mechanically. Feeds [`super::block_freq`].
//!
//! A conditional has a cold arm when the first matching rule says so:
//!
//! 1. **Throw probe** — `icmp ne (load (global_ref THROW_ACTIVE)), 0`:
//!    the throw arm is taken only when an exception is in flight.
//! 2. **Unreachable arm** — an arm ending in `unreachable`.
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

use crate::ssa::id_hash::{HashMap, HashSet};

use crate::ssa::{
    BinOp, BlockId, Function, IPred, InstKind, Operand, THROW_ACTIVE_SYM, Terminator, ValueId,
};

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
    let defs = single_defs(func);
    let cells = length_cells(func, &defs);
    let blocks: HashMap<BlockId, usize> = func
        .blocks
        .iter()
        .enumerate()
        .map(|(i, b)| (b.id, i))
        .collect();
    let calls = |b: BlockId| {
        func.blocks[blocks[&b]].insts.iter().any(|i| {
            matches!(
                i.kind,
                InstKind::Call(..) | InstKind::CallIndirect(..) | InstKind::RcDecAny(_)
            )
        })
    };
    let ends_unreachable =
        |b: BlockId| matches!(func.blocks[blocks[&b]].term, Terminator::Unreachable);

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
        let cond = match cond {
            Operand::Value(v) => defs.get(v).copied(),
            _ => None,
        };
        let calling_arm = || if calls(t) { t } else { e };
        let one_calls = calls(t) != calls(e);
        let arm = if cond.is_some_and(|c| is_throw_probe(c, &defs)) {
            Some((t, P_NEVER))
        } else if ends_unreachable(t) != ends_unreachable(e) {
            Some((if ends_unreachable(t) { t } else { e }, P_NEVER))
        } else if one_calls && cond.is_some_and(|c| is_flag_guard(c, &defs)) {
            Some((calling_arm(), P_SLOW_PATH))
        } else if let Some(oob) = cond.and_then(|c| out_of_bounds_arm(c, &defs, &cells, t, e)) {
            Some((oob, P_SLOW_PATH))
        } else if one_calls && cond.is_some_and(|c| is_boxed_guard(c, &defs, 1)) {
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

/// Defining inst of every value written exactly once. φ destruction
/// gives a merged value one `copy` per predecessor; such values match
/// no rule, except a register cell every def of which copies a load of
/// the same word through the same pointer, or the same mask of one (a
/// header word kept in a register across a loop and read again after
/// the calls that may write it): it reads as that load or mask.
fn single_defs(func: &Function) -> HashMap<ValueId, &InstKind> {
    let mut all: HashMap<ValueId, Vec<&InstKind>> = HashMap::default();
    for b in &func.blocks {
        for i in &b.insts {
            if let Some(r) = i.result {
                all.entry(r).or_default().push(&i.kind);
            }
        }
    }
    let single = |v: &ValueId| match all.get(v).map(Vec::as_slice) {
        Some([k]) => Some(*k),
        _ => None,
    };
    // a copied load, or a copied mask of one: (def, pointer, offset, mask)
    let loaded = |k: &InstKind| {
        let InstKind::Copy(_, Operand::Value(t)) = k else {
            return None;
        };
        match single(t)? {
            l @ InstKind::Load(_, ptr, off) => Some((l, *ptr, *off, None)),
            m @ InstKind::BinOp(BinOp::And, Operand::Value(w), Operand::ConstI64(mask)) => {
                match single(w)? {
                    InstKind::Load(_, ptr, off) => Some((m, *ptr, *off, Some(*mask))),
                    _ => None,
                }
            }
            _ => None,
        }
    };
    let cell = |ds: &[&InstKind]| {
        let (def, p, o, m) = loaded(ds[0])?;
        ds[1..]
            .iter()
            .all(|d| loaded(d).is_some_and(|(_, p2, o2, m2)| (p2, o2, m2) == (p, o, m)))
            .then_some(def)
    };
    all.iter()
        .filter_map(|(v, ds)| match ds.as_slice() {
            [k] => Some((*v, *k)),
            _ => cell(ds).map(|l| (*v, l)),
        })
        .collect()
}

fn value_def<'a>(op: &Operand, defs: &HashMap<ValueId, &'a InstKind>) -> Option<&'a InstKind> {
    match op {
        Operand::Value(v) => defs.get(v).copied(),
        _ => None,
    }
}

fn is_zero(op: &Operand) -> bool {
    matches!(op, Operand::ConstI64(0))
}

/// `icmp ne (load (global_ref THROW_ACTIVE)), 0`.
fn is_throw_probe(cond: &InstKind, defs: &HashMap<ValueId, &InstKind>) -> bool {
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
fn is_const(op: &Operand, defs: &HashMap<ValueId, &InstKind>) -> bool {
    matches!(op, Operand::ConstI64(_))
        || matches!(
            value_def(op, defs),
            Some(InstKind::Copy(_, Operand::ConstI64(_)))
        )
}

/// `icmp eq|ne (and (load p +off), MASK), K` — a test of header bits.
fn is_flag_guard(cond: &InstKind, defs: &HashMap<ValueId, &InstKind>) -> bool {
    let InstKind::ICmp(IPred::Ne | IPred::Eq, masked, k) = cond else {
        return false;
    };
    if !is_const(k, defs) {
        return false;
    }
    let Some(InstKind::BinOp(BinOp::And, word, Operand::ConstI64(_))) = value_def(masked, defs)
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
fn is_boxed_guard(cond: &InstKind, defs: &HashMap<ValueId, &InstKind>, depth: u32) -> bool {
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
fn is_int_pair_test(cond: &InstKind, defs: &HashMap<ValueId, &InstKind>) -> bool {
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
fn length_cells(func: &Function, defs: &HashMap<ValueId, &InstKind>) -> HashSet<ValueId> {
    let mut out = HashSet::default();
    for b in &func.blocks {
        for i in &b.insts {
            if let (Some(r), InstKind::Copy(_, src)) = (i.result, &i.kind)
                && !defs.contains_key(&r)
                && is_len_load(value_def(src, defs))
            {
                out.insert(r);
            }
        }
    }
    out
}

/// The arm an unsigned compare against the array length word takes
/// when the index is out of bounds.
fn out_of_bounds_arm(
    cond: &InstKind,
    defs: &HashMap<ValueId, &InstKind>,
    cells: &HashSet<ValueId>,
    then_blk: BlockId,
    else_blk: BlockId,
) -> Option<BlockId> {
    let InstKind::ICmp(pred @ (IPred::Ult | IPred::Uge), _, len) = cond else {
        return None;
    };
    let in_cell = |op: &Operand| matches!(op, Operand::Value(v) if cells.contains(v));
    let is_len = is_len_load(value_def(len, defs))
        || in_cell(len)
        || matches!(value_def(len, defs), Some(InstKind::Copy(_, c)) if in_cell(c));
    is_len.then_some(if *pred == IPred::Ult {
        else_blk
    } else {
        then_blk
    })
}
