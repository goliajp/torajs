//! Store pairing — two adjacent stores through one base into
//! neighbouring words become one STP.
//!
//! A fresh object's header and fields are written one `store` at a
//! time (`rc = 1`, `tag word = 1`, then each field), and each lowered
//! to its own constant move and STR. Two stores next to each other in
//! a block, through the same base value, of the same width (both W or
//! both X, both in general registers), at offsets `o` and `o + width`
//! with `o` width-aligned and in STP's reach, write exactly what one
//! `STP lo, hi, [base, #o]` writes: nothing runs between them, and
//! neither store's value depends on the other. A zero is stored from
//! the zero register — an f64 +0.0 too, whose bits are the integer
//! zero, so a float literal's zero-filled slots pair like any other —
//! and the same operand on both sides is materialized once. Any other
//! f64 constant is stored as its bit pattern from a general register
//! too, so it pairs like an integer.

use torajs_core::ssa::id_hash::HashMap;

use torajs_core::ssa::{Function, InstKind, Operand, Type};

use super::operand::{access_base, materialize_const_i64, materialize_operand_gpr, operand_is_f64};
use super::{OP_SCRATCH_LHS, OP_SCRATCH_RHS, OP_SCRATCH_TMP, write_u32};
use crate::enc::{stp_w_off, stp_x_off};
use crate::reg::Gpr;
use crate::regalloc::Assignment;

/// One planned pair, keyed by the position of its first store; the
/// second store is absorbed.
pub(super) struct StorePair {
    lo: Operand,
    hi: Operand,
    base: Operand,
    offset: u64,
    w32: bool,
}

/// Width-class of a store's value: `Some(true)` for a 32-bit store,
/// `Some(false)` for 64-bit, `None` for one that does not pair (an f64
/// value lives in the other register file; an f64 constant does not).
fn gpr_width(func: &Function, val: &Operand, alloc: &Assignment) -> Option<bool> {
    if zero_word(val) {
        return Some(matches!(val, Operand::ConstI32(_)));
    }
    if let Operand::ConstF64(_) = val {
        return Some(false);
    }
    if operand_is_f64(val, alloc) {
        return None;
    }
    Some(match val {
        Operand::ConstI32(_) => true,
        Operand::Value(v) => func.value_type(*v) == Type::I32,
        _ => false,
    })
}

pub(super) fn plan(func: &Function, alloc: &Assignment) -> HashMap<(u32, u32), StorePair> {
    let mut out = HashMap::default();
    for block in &func.blocks {
        let mut ii = 0;
        while ii + 1 < block.insts.len() {
            let (InstKind::Store(v1, p1, o1), InstKind::Store(v2, p2, o2)) =
                (&block.insts[ii].kind, &block.insts[ii + 1].kind)
            else {
                ii += 1;
                continue;
            };
            let same_base = matches!((p1, p2), (Operand::Value(a), Operand::Value(b)) if a == b);
            let widths = (gpr_width(func, v1, alloc), gpr_width(func, v2, alloc));
            let pair = match widths {
                (Some(w1), Some(w2)) if same_base && w1 == w2 => {
                    let size = if w1 { 4 } else { 8 };
                    let (lo, hi, offset) = if o2.checked_sub(*o1) == Some(size) {
                        (*v1, *v2, *o1)
                    } else if o1.checked_sub(*o2) == Some(size) {
                        (*v2, *v1, *o2)
                    } else {
                        (*v1, *v2, u64::MAX)
                    };
                    (offset.is_multiple_of(size) && offset <= 63 * size).then_some(StorePair {
                        lo,
                        hi,
                        base: *p1,
                        offset,
                        w32: w1,
                    })
                }
                _ => None,
            };
            match pair {
                Some(p) => {
                    out.insert((block.id.0, ii as u32), p);
                    ii += 2;
                }
                None => ii += 1,
            }
        }
    }
    out
}

pub(super) fn is_zero(op: &Operand) -> bool {
    matches!(
        op,
        Operand::ConstI64(0) | Operand::ConstI32(0) | Operand::ConstPtrNull
    )
}

/// A value stored from the zero register: an integer zero, null, or
/// +0.0 (not -0.0, whose sign bit is set).
pub(super) fn zero_word(op: &Operand) -> bool {
    is_zero(op) || matches!(op, Operand::ConstF64(x) if x.to_bits() == 0)
}

fn same_operand(a: &Operand, b: &Operand) -> bool {
    match (a, b) {
        (Operand::Value(x), Operand::Value(y)) => x == y,
        (Operand::ConstI64(x), Operand::ConstI64(y)) => x == y,
        (Operand::ConstI32(x), Operand::ConstI32(y)) => x == y,
        (Operand::ConstF64(x), Operand::ConstF64(y)) => x.to_bits() == y.to_bits(),
        _ => false,
    }
}

/// The stored word in a general register: an f64 constant as its bits.
pub(super) fn word_in_gpr(
    bytes: &mut Vec<u8>,
    op: &Operand,
    scratch: Gpr,
    alloc: &Assignment,
) -> Gpr {
    if let Operand::ConstF64(c) = op {
        materialize_const_i64(bytes, scratch, c.to_bits() as i64);
        return scratch;
    }
    materialize_operand_gpr(bytes, op, scratch, alloc)
}

pub(super) fn emit(bytes: &mut Vec<u8>, p: &StorePair, alloc: &Assignment) {
    let rt = if zero_word(&p.lo) {
        Gpr::XZR
    } else {
        word_in_gpr(bytes, &p.lo, OP_SCRATCH_LHS, alloc)
    };
    let rt2 = if zero_word(&p.hi) {
        Gpr::XZR
    } else if same_operand(&p.lo, &p.hi) {
        rt
    } else {
        word_in_gpr(bytes, &p.hi, OP_SCRATCH_TMP, alloc)
    };
    let size: u64 = if p.w32 { 4 } else { 8 };
    // a spilled alloca folds into `[SP, #slot + offset]` while the
    // seven scaled bits reach it
    let reach = (63 * size).saturating_sub(p.offset) as u32;
    let (rn, at) = access_base(bytes, &p.base, OP_SCRATCH_RHS, alloc, reach);
    let off = (p.offset + at) as i32;
    if p.w32 {
        write_u32(bytes, stp_w_off(rt, rt2, rn, off));
    } else {
        write_u32(bytes, stp_x_off(rt, rt2, rn, off));
    }
}

#[cfg(test)]
mod tests {
    use super::super::compile_function;
    use crate::enc;
    use crate::reg::Gpr;
    use torajs_core::ssa::{Function, InstKind, Operand, Terminator, Type};

    fn words(f: &Function) -> Vec<u32> {
        compile_function(f)
            .bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| u32::from_le_bytes(*c))
            .collect()
    }

    /// The pair shape with the base register field masked out.
    fn shape(w: u32) -> u32 {
        w & !(0x1F << 5)
    }

    fn header_init(stores: &[(Operand, u64)]) -> Function {
        let mut f = Function::new("t", Type::I64);
        let p = f.add_param(Type::Ptr, "p");
        let b = f.add_block();
        for (v, off) in stores {
            f.append_void(b, InstKind::Store(*v, Operand::Value(p), *off));
        }
        f.set_term(b, Terminator::Ret(Some(Operand::ConstI64(0))));
        f
    }

    #[test]
    fn neighbouring_words_store_as_pairs() {
        let f = header_init(&[
            (Operand::ConstI32(1), 0),
            (Operand::ConstI32(1), 4),
            (Operand::ConstI64(3), 8),
            (Operand::ConstPtrNull, 16),
        ]);
        let w = words(&f);
        let w_pair = shape(enc::stp_w_off(Gpr::X9, Gpr::X9, Gpr::X0, 0));
        let x_pair = shape(enc::stp_x_off(Gpr::X9, Gpr::XZR, Gpr::X0, 8));
        assert!(
            w.iter().any(|x| shape(*x) == w_pair),
            "one W pair, one constant"
        );
        assert!(
            w.iter().any(|x| shape(*x) == x_pair),
            "X pair from the zero register"
        );
        let single_str = |x: &u32| (x & 0xBFC0_0000) == 0xB900_0000;
        assert!(!w.iter().any(single_str), "no single STR left");
    }

    #[test]
    fn float_zeros_pair_from_the_zero_register() {
        let f = header_init(&[
            (Operand::ConstF64(0.0), 40),
            (Operand::ConstF64(0.0), 48),
            (Operand::ConstF64(-0.0), 56),
            (Operand::ConstF64(0.0), 64),
        ]);
        let w = words(&f);
        let zero_pair = shape(enc::stp_x_off(Gpr::XZR, Gpr::XZR, Gpr::X0, 40));
        assert!(w.iter().any(|x| shape(*x) == zero_pair), "+0.0 pair");
        // -0.0 is stored as its bit pattern from a general register,
        // so it pairs with its neighbour too
        let x_pairs = w.iter().filter(|x| (*x & 0x7FC0_0000) == 0x2900_0000);
        assert_eq!(x_pairs.count(), 2, "{w:08x?}");
        assert!(
            !w.iter().any(|x| (*x & 0xFFC0_0000) == 0xFD00_0000),
            "no STR d"
        );
    }

    /// A spilled alloca folds into the pair while slot plus offset is
    /// within the seven scaled bits; one word further the address
    /// takes an ADD and the pair keeps its own offset.
    #[test]
    fn a_spilled_alloca_folds_into_the_pair_within_its_reach() {
        use super::{StorePair, emit};
        use crate::reg::Reg;
        use crate::regalloc::Assignment;
        use torajs_core::ssa::ValueId;
        use torajs_core::ssa::id_hash::HashMap;

        let pair = StorePair {
            lo: Operand::ConstI64(0),
            hi: Operand::ConstI64(0),
            base: Operand::Value(ValueId(0)),
            offset: 8,
            w32: false,
        };
        let at = |slot: u32| {
            let a = Assignment::from_parts(
                HashMap::from_iter([(0, Reg::SpillGpr(slot))]),
                HashMap::from_iter([(0, slot)]),
                slot + 16,
                0,
                false,
            );
            let mut bytes = Vec::new();
            emit(&mut bytes, &pair, &a);
            bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| u32::from_le_bytes(*c))
                .collect::<Vec<u32>>()
        };
        assert_eq!(at(496), [enc::stp_x_off(Gpr::XZR, Gpr::XZR, Gpr::SP, 504)]);
        assert_eq!(
            at(504),
            [
                enc::add_imm(Gpr::X10, Gpr::SP, 504),
                enc::stp_x_off(Gpr::XZR, Gpr::XZR, Gpr::X10, 8),
            ]
        );
    }

    #[test]
    fn stores_apart_stay_single() {
        // +8 then +24: not neighbours
        let f = header_init(&[(Operand::ConstI64(3), 8), (Operand::ConstI64(4), 24)]);
        let w = words(&f);
        assert!(!w.iter().any(|x| (x & 0x7FC0_0000) == 0x2900_0000));
    }
}
