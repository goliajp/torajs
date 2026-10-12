//! Plain (unfused) Select lowering state: which `cmp v, #0` NZCV still
//! holds, so the next Select on the same condition value skips its own
//! compare; and the XZR source for a zero constant arm.
//!
//! The memo lives for one basic block (a block start can be reached
//! with any flags). It is checked against the words emitted since the
//! recorded compare rather than against instruction kinds, so a fused
//! compare, a call, an inline helper loop or a spill sequence that
//! happens to land between two Selects invalidates it on its own.
//!
//! One compare feeds at most `MAX_READERS` selects. On mandelbrot's
//! masked lanes (`cmp go, #0` then fcsel, fcsel, csel) letting all three
//! read one compare cost 2-4% more cycles in every one of 16 code-layout
//! phases although it retired 17% fewer instructions; two readers per
//! compare ran no slower than one in any phase.

use torajs_core::ssa::{Inst, InstKind, Operand, ValueId};

use super::operand::materialize_operand_gpr;
use super::write_u32;
use crate::enc::cmp_imm;
use crate::reg::Gpr;
use crate::regalloc::Assignment;

const MAX_READERS: u32 = 2;

#[derive(Default)]
pub(super) struct CondFlags {
    /// the condition value, the byte offset right after its `cmp #0`,
    /// and how many selects have read that compare
    held: Option<(ValueId, usize, u32)>,
}

impl CondFlags {
    /// Emit `inst` if it is a Select; false leaves it to the caller.
    pub(super) fn emit_select(
        &mut self,
        bytes: &mut Vec<u8>,
        inst: &Inst,
        alloc: &Assignment,
    ) -> bool {
        let is_select = matches!(inst.kind, InstKind::Select(..));
        if is_select {
            super::cmp::emit_select(bytes, inst, alloc, self);
        }
        is_select
    }

    /// `cmp cond, #0`, skipped when NZCV already holds it.
    pub(super) fn compare_zero(
        &mut self,
        bytes: &mut Vec<u8>,
        cond: &Operand,
        scratch: Gpr,
        alloc: &Assignment,
    ) {
        if let Operand::Value(v) = cond
            && self.holds(bytes, *v)
            && let Some((_, _, readers)) = &mut self.held
        {
            *readers += 1;
            return;
        }
        let rc = materialize_operand_gpr(bytes, cond, scratch, alloc);
        write_u32(bytes, cmp_imm(rc, 0));
        self.held = match cond {
            Operand::Value(v) => Some((*v, bytes.len(), 1)),
            _ => None,
        };
    }

    /// True when NZCV still holds `cmp v, #0` and may take another
    /// reader: the last compare recorded was on `v`, has fewer than
    /// `MAX_READERS` readers, and every word emitted since leaves NZCV
    /// alone.
    fn holds(&self, bytes: &[u8], v: ValueId) -> bool {
        let Some((held, end, readers)) = self.held else {
            return false;
        };
        held == v
            && readers < MAX_READERS
            && bytes[end..]
                .as_chunks::<4>()
                .0
                .iter()
                .all(|w| keeps_nzcv(u32::from_le_bytes(*w)))
    }
}

/// The encodings a Select's operand reloads, constant materialization,
/// result spill and value copies produce, none of which writes NZCV.
/// Anything else (compares, flag-setting arithmetic, branches, calls)
/// is treated as a writer.
fn keeps_nzcv(w: u32) -> bool {
    const KEEPERS: [(u32, u32); 9] = [
        (0x3FE0_0000, 0x1A80_0000), // CSEL / CSINC / CSINV / CSNEG
        (0xFF20_0C00, 0x1E20_0C00), // FCSEL
        (0x1F80_0000, 0x1280_0000), // MOVN / MOVZ / MOVK
        (0x7F20_0000, 0x2A00_0000), // ORR (shifted register), MOV Xd, Xm
        (0x7F80_0000, 0x3200_0000), // ORR (immediate)
        (0x7F80_0000, 0x1100_0000), // ADD (immediate), the far-slot base
        (0xFFFE_FC00, 0x9E66_0000), // FMOV Xd, Dn / FMOV Dd, Xn
        (0xBFE0_FC00, 0x0EA0_1C00), // ORR Vd, Vn, Vn (MOV Dd, Dn)
        (0xFB80_0000, 0xF900_0000), // LDR / STR Xt|Dt, [Xn, #imm12]
    ];
    KEEPERS.iter().any(|&(mask, val)| w & mask == val)
}

/// A GPR Select arm: XZR for a zero constant (CSEL reads register 31
/// as zero), else the operand materialized into `scratch`.
pub(super) fn select_arm_gpr(
    bytes: &mut Vec<u8>,
    op: &Operand,
    scratch: Gpr,
    alloc: &Assignment,
) -> Gpr {
    match op {
        Operand::ConstI64(0)
        | Operand::ConstI32(0)
        | Operand::ConstBool(false)
        | Operand::ConstPtrNull => Gpr::XZR,
        _ => materialize_operand_gpr(bytes, op, scratch, alloc),
    }
}

#[cfg(test)]
mod tests {
    use super::super::compile_function;
    use super::*;
    use crate::enc::{self, cond};
    use torajs_core::ssa::{BinOp, Block, BlockId, Function, IPred, Terminator, Type, ValueInfo};

    const CMP_ZERO_MASK: u32 = 0xFFFF_FC1F;
    const CMP_ZERO: u32 = 0xF100_001F;

    fn mk(result: u32, kind: InstKind) -> Inst {
        Inst {
            result: Some(ValueId(result)),
            kind,
            origin: None,
        }
    }

    fn val(v: u32) -> Operand {
        Operand::Value(ValueId(v))
    }

    /// `f(c: bool, a: i64, b: i64)`: `s1 = c ? a : 0`, then `between`
    /// (each defining the next value id), then `s2 = c ? b : s1`, and
    /// `ret s2`, or `ret s2 + <last between value>`.
    fn two_selects(between: Vec<InstKind>) -> Function {
        let mut insts = vec![mk(
            3,
            InstKind::Select(Type::I64, val(0), val(1), Operand::ConstI64(0)),
        )];
        let mut values = vec![
            ValueInfo {
                ty: Type::Bool,
                name: None,
            },
            ValueInfo {
                ty: Type::I64,
                name: None,
            },
            ValueInfo {
                ty: Type::I64,
                name: None,
            },
            ValueInfo {
                ty: Type::I64,
                name: None,
            },
        ];
        let mut extra = None;
        for kind in between {
            let id = values.len() as u32;
            let ty = if matches!(kind, InstKind::ICmp(..)) {
                Type::Bool
            } else {
                Type::I64
            };
            values.push(ValueInfo { ty, name: None });
            insts.push(mk(id, kind));
            extra = Some(val(id));
        }
        let s2 = values.len() as u32;
        values.push(ValueInfo {
            ty: Type::I64,
            name: None,
        });
        insts.push(mk(s2, InstKind::Select(Type::I64, val(0), val(2), val(3))));
        let mut ret = s2;
        if let Some(extra) = extra {
            ret = values.len() as u32;
            values.push(ValueInfo {
                ty: Type::I64,
                name: None,
            });
            insts.push(mk(ret, InstKind::BinOp(BinOp::Add, val(s2), extra)));
        }
        Function {
            name: "two_selects".into(),
            params: vec![ValueId(0), ValueId(1), ValueId(2)],
            ret: Type::I64,
            values,
            blocks: vec![Block {
                id: BlockId(0),
                insts,
                term: Terminator::Ret(Some(val(ret))),
            }],
            current_origin: None,
        }
    }

    fn words(func: &Function) -> Vec<u32> {
        compile_function(func)
            .bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| u32::from_le_bytes(*c))
            .collect()
    }

    fn cmp_zero_count(words: &[u32]) -> usize {
        words
            .iter()
            .filter(|&&w| w & CMP_ZERO_MASK == CMP_ZERO)
            .count()
    }

    fn csels(words: &[u32]) -> Vec<u32> {
        words
            .iter()
            .copied()
            .filter(|&w| w & 0xFFE0_0C00 == 0x9A80_0000)
            .collect()
    }

    #[test]
    fn zero_arm_reads_xzr() {
        let w = words(&two_selects(Vec::new()));
        let first = csels(&w)[0];
        assert_eq!((first >> 16) & 31, 31, "else arm must be XZR: {first:08X}");
        assert_eq!((first >> 12) & 15, cond::NE as u32);
        assert!(
            !w.iter()
                .any(|&x| x & 0xFFFF_FFE0 == enc::movz_imm(Gpr::X0, 0, 0)),
            "no MOVZ #0 for the zero arm: {w:08X?}"
        );
    }

    #[test]
    fn adjacent_selects_on_one_cond_share_its_compare() {
        let w = words(&two_selects(Vec::new()));
        assert_eq!(csels(&w).len(), 2);
        assert_eq!(cmp_zero_count(&w), 1, "{w:08X?}");
    }

    #[test]
    fn a_third_select_takes_a_fresh_compare() {
        let mid = InstKind::Select(Type::I64, val(0), val(1), val(2));
        let w = words(&two_selects(vec![mid]));
        assert_eq!(csels(&w).len(), 3);
        assert_eq!(cmp_zero_count(&w), 2, "{w:08X?}");
    }

    #[test]
    fn a_move_between_keeps_the_shared_compare() {
        let w = words(&two_selects(vec![InstKind::Copy(Type::I64, val(1))]));
        assert_eq!(cmp_zero_count(&w), 1, "{w:08X?}");
    }

    #[test]
    fn a_flag_writer_between_forces_a_new_compare() {
        let icmp = InstKind::ICmp(IPred::Slt, val(1), val(2));
        let zext = InstKind::ZExtBoolToI64(val(4));
        let w = words(&two_selects(vec![icmp, zext]));
        assert_eq!(cmp_zero_count(&w), 2, "{w:08X?}");
    }

    #[test]
    fn a_flag_setting_word_is_not_a_keeper() {
        assert!(!keeps_nzcv(enc::cmp_imm(Gpr::X1, 0)));
        assert!(!keeps_nzcv(enc::cmp_reg(Gpr::X1, Gpr::X2)));
        assert!(keeps_nzcv(enc::mov_x_reg(Gpr::X1, Gpr::X2)));
        assert!(keeps_nzcv(enc::csel_cond(
            Gpr::X1,
            Gpr::X2,
            Gpr::XZR,
            cond::NE
        )));
        assert!(keeps_nzcv(enc::movz_imm(Gpr::X1, 7, 0)));
        assert!(keeps_nzcv(enc::ldr_x_imm12(Gpr::X1, Gpr::SP, 16)));
        assert!(keeps_nzcv(enc::str_x_imm12(Gpr::X1, Gpr::SP, 16)));
    }
}
