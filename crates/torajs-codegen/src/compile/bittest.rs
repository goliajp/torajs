//! Bit-test branch — a CondBr on `(x & K) != 0` or `(x & K) == 0`,
//! where K has one set bit, or two that no logical immediate covers,
//! branches on the bits themselves with TBNZ / TBZ.
//!
//! The AND path pays the mask's materialization (MOVZ + MOVK for a
//! two-bit word such as the frozen | redefined-field pair a typed
//! field store tests), the AND, and a CBZ / CBNZ; the bit tests pay
//! one instruction per bit. The AND and the compare are absorbed: the
//! AND's input is still in its register at the terminator, because
//! nothing between the AND and the branch emits.
//!
//! TBZ / TBNZ reach ±32 KiB. A function whose bit-test branch lands
//! further away compiles again without these fusions (see
//! `compile_function_with`).

use torajs_core::ssa::id_hash::HashMap;

use torajs_core::ssa::{BinOp, Block, BlockId, IPred, InstKind, Operand, ValueId};

use super::brfuse::{FusedCmp, FusedSelPred, invert_ipred};
use super::operand::materialize_operand_gpr;
use super::{BranchFixup, BranchKind, Fixups, OP_SCRATCH_LHS, write_u32};
use crate::enc::{b_imm26, logical_imm, tbnz, tbz};
use crate::regalloc::Assignment;

/// The bits of `k` a branch tests one by one: one bit (twice), or two
/// bits whose mask is not a logical immediate (an encodable mask is
/// already AND #imm + CBZ, no longer than two bit tests). The compare
/// against zero must have the mask's width: an I32 mask reads the low
/// word only.
fn mask_bits(k: &Operand, zero: &Operand) -> Option<[u32; 2]> {
    let (word, sext) = match (k, zero) {
        (Operand::ConstI32(c), Operand::ConstI32(0)) => (*c as u32 as u64, *c as i64 as u64),
        (Operand::ConstI64(c), Operand::ConstI64(0)) => (*c as u64, *c as u64),
        _ => return None,
    };
    let lo = word.trailing_zeros();
    let hi = 63 - word.leading_zeros();
    match word.count_ones() {
        1 => Some([lo, lo]),
        2 if logical_imm(sext).is_none() => Some([lo, hi]),
        _ => None,
    }
}

/// The tested value and bits when `block`'s tail is `a = and x, K;
/// c = icmp eq|ne a, 0` with `a` read only by the compare.
pub(crate) fn pick(
    block: &Block,
    pred: IPred,
    lhs: &Operand,
    rhs: &Operand,
    uses: &HashMap<ValueId, u32>,
) -> Option<(Operand, [u32; 2])> {
    let (IPred::Eq | IPred::Ne, Operand::Value(a)) = (pred, lhs) else {
        return None;
    };
    let and = block.insts.get(block.insts.len().checked_sub(2)?)?;
    let InstKind::BinOp(BinOp::And, l, r) = &and.kind else {
        return None;
    };
    if and.result != Some(*a) || uses.get(a) != Some(&1) {
        return None;
    }
    let (x, k) = match (l, r) {
        (x @ Operand::Value(_), k) | (k, x @ Operand::Value(_)) => (x, k),
        _ => return None,
    };
    Some((*x, mask_bits(k, rhs)?))
}

/// Emit the bit-test form of a fused CondBr: branch to `target` when
/// the (possibly inverted) predicate holds, else reach `other` — by
/// falling through, or by the trailing B when `trailing`.
pub(crate) fn emit_condbr(
    bytes: &mut Vec<u8>,
    fixups: &mut Fixups,
    fc: &FusedCmp,
    invert: bool,
    (target, other): (BlockId, BlockId),
    trailing: bool,
    alloc: &Assignment,
) {
    let (FusedSelPred::Int(p), Some([b0, b1])) = (fc.pred, fc.bits) else {
        unreachable!("a bit-test fuse is an integer compare with its bits");
    };
    let any_set = (if invert { invert_ipred(p) } else { p }) == IPred::Ne;
    let rt = materialize_operand_gpr(bytes, &fc.lhs, OP_SCRATCH_LHS, alloc);
    let mut branch = |bytes: &mut Vec<u8>, word: u32, to: BlockId, kind: BranchKind| {
        fixups.push(BranchFixup {
            site_byte_offset: bytes.len() as u32,
            target_block: to,
            kind,
        });
        write_u32(bytes, word);
    };
    if any_set {
        branch(bytes, tbnz(rt, b0, 0), target, BranchKind::Imm14);
        if b1 != b0 {
            branch(bytes, tbnz(rt, b1, 0), target, BranchKind::Imm14);
        }
    } else {
        if b1 != b0 {
            branch(bytes, tbnz(rt, b0, 0), other, BranchKind::Imm14);
        }
        branch(bytes, tbz(rt, b1, 0), target, BranchKind::Imm14);
    }
    if trailing {
        branch(bytes, b_imm26(0), other, BranchKind::Imm26);
    }
}

#[cfg(test)]
mod tests {
    use super::super::compile_function;
    use super::*;
    use torajs_core::ssa::{Function, Terminator, Type};

    #[test]
    fn mask_bits_takes_one_bit_or_two_unencodable_bits() {
        let i32z = Operand::ConstI32(0);
        let i64z = Operand::ConstI64(0);
        let guard = 0x8010_0000u32 as i32;
        assert_eq!(mask_bits(&Operand::ConstI32(guard), &i32z), Some([20, 31]));
        assert_eq!(
            mask_bits(&Operand::ConstI64(1 << 21), &i64z),
            Some([21, 21])
        );
        assert_eq!(
            mask_bits(&Operand::ConstI64(1 << 63), &i64z),
            Some([63, 63])
        );
        // adjacent bits are one logical immediate
        assert_eq!(mask_bits(&Operand::ConstI64(0b110), &i64z), None);
        assert_eq!(mask_bits(&Operand::ConstI64(0b10101), &i64z), None);
        // the compare must read the mask's width
        assert_eq!(mask_bits(&Operand::ConstI32(guard), &i64z), None);
    }

    /// `h = load i32 [p + 4]; a = and h, K; c = icmp <pred> a, 0;
    /// condbr c` with two returning arms
    fn guard_func(pred: IPred, k: i32) -> Function {
        let mut f = Function::new("g", Type::I64);
        let b0 = f.add_block();
        let b1 = f.add_block();
        let b2 = f.add_block();
        let p = f.add_param(Type::Ptr, "p");
        let h = f.append_inst(
            b0,
            InstKind::Load(Type::I32, Operand::Value(p), 4),
            Type::I32,
            None,
        );
        let a = f.append_inst(
            b0,
            InstKind::BinOp(BinOp::And, Operand::Value(h), Operand::ConstI32(k)),
            Type::I32,
            None,
        );
        let c = f.append_inst(
            b0,
            InstKind::ICmp(pred, Operand::Value(a), Operand::ConstI32(0)),
            Type::Bool,
            None,
        );
        f.set_term(
            b0,
            Terminator::CondBr {
                cond: Operand::Value(c),
                then_blk: b1,
                else_blk: b2,
            },
        );
        f.set_term(b1, Terminator::Ret(Some(Operand::ConstI64(1))));
        f.set_term(b2, Terminator::Ret(Some(Operand::ConstI64(2))));
        f
    }

    fn words(f: &Function) -> Vec<u32> {
        let bytes = compile_function(f).bytes;
        let words = bytes.as_chunks::<4>().0.iter();
        words.map(|c| u32::from_le_bytes(*c)).collect()
    }

    fn is_bit_test(w: u32) -> bool {
        w & 0x7E00_0000 == 0x3600_0000
    }

    fn is_and(w: u32) -> bool {
        // AND (shifted register) or AND (immediate), either width
        w & 0x7F20_0000 == 0x0A00_0000 || w & 0x7F80_0000 == 0x1200_0000
    }

    #[test]
    fn two_bit_guard_branches_on_each_bit() {
        for pred in [IPred::Eq, IPred::Ne] {
            let w = words(&guard_func(pred, 0x8010_0000u32 as i32));
            assert!(!w.iter().any(|w| is_and(*w)), "{w:08x?}");
            let mut bits: Vec<u32> = (w.iter().filter(|w| is_bit_test(**w)))
                .map(|w| (w >> 19) & 0x1F)
                .collect();
            bits.sort();
            assert_eq!(bits, [20, 31], "{w:08x?}");
        }
    }

    #[test]
    fn one_bit_is_one_bit_test() {
        for pred in [IPred::Eq, IPred::Ne] {
            let w = words(&guard_func(pred, 1 << 21));
            assert!(!w.iter().any(|w| is_and(*w)), "{w:08x?}");
            assert_eq!(w.iter().filter(|w| is_bit_test(**w)).count(), 1, "{w:08x?}");
        }
    }

    #[test]
    fn encodable_two_bit_mask_keeps_the_and() {
        let w = words(&guard_func(IPred::Ne, 0b11 << 4));
        assert!(w.iter().any(|w| is_and(*w)), "{w:08x?}");
        assert!(!w.iter().any(|w| is_bit_test(*w)), "{w:08x?}");
    }

    #[test]
    fn out_of_reach_bit_test_falls_back_to_the_and() {
        for pred in [IPred::Eq, IPred::Ne] {
            let mut f = guard_func(pred, 0x8010_0000u32 as i32);
            let Terminator::CondBr { then_blk, .. } = f.blocks[0].term else {
                unreachable!("guard_func ends b0 with a CondBr");
            };
            // the then arm, laid out next, grows past the +-32 KiB reach,
            // so the branch to the else arm cannot be a TBZ / TBNZ
            let mut v = f.add_param(Type::I64, "n");
            for _ in 0..10_000 {
                let add = InstKind::BinOp(BinOp::Add, Operand::Value(v), Operand::ConstI64(1));
                v = f.append_inst(then_blk, add, Type::I64, None);
            }
            f.set_term(then_blk, Terminator::Ret(Some(Operand::Value(v))));
            let w = words(&f);
            assert!(w.len() * 4 > 32 * 1024, "{} bytes", w.len() * 4);
            assert!(!w.iter().any(|w| is_bit_test(*w)));
            assert!(w.iter().any(|w| is_and(*w)));
        }
    }
}
