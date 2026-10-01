//! Object cells through the allocator's thread cache, inline:
//! `Call(obj_alloc, [n])` and `Call(obj_drop_sized, [p, n])` with a
//! constant `n` that has a size class.

use torajs_core::ssa::{FuncId, Inst, InstKind, Module, Operand};
use torajs_mmalloc::Allocator;
use torajs_mmalloc::size_class::SIZE_CLASSES;
use torajs_mmalloc::tlab::{
    TLAB_CACHE_DEPTH, TLAB_CLASS_SLOTS_STRIDE, TLAB_DEPTH_OFFSET, TLAB_SLOTS_OFFSET,
};

use super::rc_dec::{Fix, patch, place, store};
use super::refs::emit_adrp_add_pair;
use super::{
    OP_SCRATCH_LHS, OP_SCRATCH_RESULT_GPR, OP_SCRATCH_RHS, OP_SCRATCH_TMP, materialize_const_i64,
    materialize_operand_gpr, write_def_spill_gpr, write_u32,
};
use crate::enc::{
    add_imm, add_w_imm, bl_imm26, ldr_x_reg_lsl3, ldrb_w_imm12, mov_x_reg, str_x_reg_lsl3,
    strb_w_imm12, sub_w_imm,
};
use crate::reg::{Gpr, Reg};
use crate::regalloc::Assignment;
use crate::reloc::{CallTarget, Reloc, RelocKind};

/// `torajs_mmalloc::core::__torajs_core_tlab` by its Mach-O name: per
/// size class, a stack of free slots every sized allocation pops
/// first and every sized free pushes first.
const TLAB_SYM: &str = "___torajs_core_tlab";
const ALLOC_NAME: &str = "__torajs_obj_alloc";
const FREE_NAME: &str = "__torajs_obj_drop_sized";

// a row never holds more than TLAB_CACHE_DEPTH slots, so its count
// has this bit set exactly when it is full
const FULL_BIT: u32 = TLAB_CACHE_DEPTH.trailing_zeros();
const _: () = assert!(TLAB_CACHE_DEPTH.is_power_of_two());
const _: () = assert!(TLAB_SLOTS_OFFSET == 0);
// every row base and every count byte is one imm12 from the cache
const _: () = assert!(TLAB_DEPTH_OFFSET + SIZE_CLASSES.len() <= 4096);
const _: () = assert!((SIZE_CLASSES.len() - 1) * TLAB_CLASS_SLOTS_STRIDE < 4096);

/// The module's two object-cell intrinsics.
#[derive(Debug, Clone, Copy)]
pub struct CellOps {
    alloc: FuncId,
    free: FuncId,
}

impl CellOps {
    /// `None` leaves every cell call a plain call: in a census
    /// compiler, whose allocator counters the inline arms would
    /// bypass, and under `TORAJS_CELL_INLINE_OFF=1` (bisect gate).
    pub fn of_module(module: &Module) -> Option<CellOps> {
        if cfg!(torajs_census)
            || std::env::var_os("TORAJS_CELL_INLINE_OFF").is_some_and(|v| v == "1")
        {
            return None;
        }
        let find = |name: &str| {
            let at = module.funcs.iter().position(|f| f.name == name)?;
            Some(FuncId(at as u32))
        };
        Some(CellOps {
            alloc: find(ALLOC_NAME)?,
            free: find(FREE_NAME)?,
        })
    }

    /// The size class of the cell `kind` allocates or frees, when it
    /// is a call [`try_emit`] opens inline.
    pub(crate) fn inline_class(self, kind: &InstKind) -> Option<u32> {
        let InstKind::Call(fid, args) = kind else {
            return None;
        };
        let n = match args.as_slice() {
            [Operand::ConstI64(n)] if *fid == self.alloc => *n,
            [Operand::Value(_), Operand::ConstI64(n)] if *fid == self.free => *n,
            _ => return None,
        };
        let n = usize::try_from(n).ok().filter(|&n| n != 0)?;
        Allocator::bucket_for(n).map(|c| c as u32)
    }
}

/// The cell call at `inst`, opened inline; `false` when it is not one.
///
/// ```text
/// alloc:                              free:
///     adrp+add x9, tlab                   adrp+add x9, tlab
///     ldrb  w10, [x9, #count[c]]          ldrb  w10, [x9, #count[c]]
///     cbz   x10, slow                     tbnz  w10, #FULL_BIT, slow
///     sub   w10, w10, #1                  add   x11, x9, #row[c]
///     add   x11, x9, #row[c]              str   xP, [x11, x10, lsl #3]
///     ldr   xD, [x11, x10, lsl #3]        add   w10, w10, #1
///     strb  w10, [x9, #count[c]]          strb  w10, [x9, #count[c]]
///     b     done                          b     done
/// slow:                               slow:
///     <store the call saves>              <store the call saves>
///     mov x0, #n ; bl obj_alloc           mov x0, xP ; mov x1, #n
///     mov xD, x0                          bl obj_drop_sized
///     <reload them>                       <reload them>
/// done:                               done:
/// ```
///
/// The slow arm is the call this replaces — an empty row refills and
/// a full one spills inside the allocator — so only it pays `saves`,
/// the allocator's call saves for this site.
pub(super) fn try_emit(
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Reloc>,
    inst: &Inst,
    alloc: &Assignment,
    saves: &[(Reg, u32)],
    cells: CellOps,
) -> bool {
    let Some(class) = cells.inline_class(&inst.kind) else {
        return false;
    };
    let InstKind::Call(fid, args) = &inst.kind else {
        return false;
    };
    let (tlab, count, row) = (OP_SCRATCH_LHS, OP_SCRATCH_RHS, OP_SCRATCH_TMP);
    let count_off = TLAB_DEPTH_OFFSET as u32 + class;
    let row_off = (class * TLAB_CLASS_SLOTS_STRIDE as u32) as u16;
    match args.as_slice() {
        [Operand::ConstI64(n)] => {
            let result = inst.result.expect("obj_alloc must have a result");
            let (dst, spill_off) = alloc.def_gpr(result, OP_SCRATCH_RESULT_GPR);
            emit_adrp_add_pair(bytes, relocs, tlab, TLAB_SYM.to_string());
            write_u32(bytes, ldrb_w_imm12(count, tlab, count_off));
            let to_slow = place(bytes, Fix::Cbz(count));
            write_u32(bytes, sub_w_imm(count, count, 1));
            write_u32(bytes, add_imm(row, tlab, row_off));
            write_u32(bytes, ldr_x_reg_lsl3(dst, row, count));
            write_u32(bytes, strb_w_imm12(count, tlab, count_off));
            let to_done = place(bytes, Fix::B);
            let slow_at = bytes.len();
            patch(bytes, to_slow, slow_at);
            emit_saves(bytes, saves, true);
            materialize_const_i64(bytes, Gpr::X0, *n);
            emit_bl(bytes, relocs, *fid);
            // before the reload: a saved register may be x0 itself
            if dst != Gpr::X0 {
                write_u32(bytes, mov_x_reg(dst, Gpr::X0));
            }
            emit_saves(bytes, saves, false);
            let done_at = bytes.len();
            patch(bytes, to_done, done_at);
            write_def_spill_gpr(bytes, spill_off, dst);
        }
        [p, Operand::ConstI64(n)] => {
            let ptr = materialize_operand_gpr(bytes, p, OP_SCRATCH_RESULT_GPR, alloc);
            emit_adrp_add_pair(bytes, relocs, tlab, TLAB_SYM.to_string());
            write_u32(bytes, ldrb_w_imm12(count, tlab, count_off));
            let to_slow = place(bytes, Fix::Tbnz(count, FULL_BIT));
            write_u32(bytes, add_imm(row, tlab, row_off));
            write_u32(bytes, str_x_reg_lsl3(ptr, row, count));
            write_u32(bytes, add_w_imm(count, count, 1));
            write_u32(bytes, strb_w_imm12(count, tlab, count_off));
            let to_done = place(bytes, Fix::B);
            let slow_at = bytes.len();
            patch(bytes, to_slow, slow_at);
            emit_saves(bytes, saves, true);
            if ptr != Gpr::X0 {
                write_u32(bytes, mov_x_reg(Gpr::X0, ptr));
            }
            materialize_const_i64(bytes, Gpr::X1, *n);
            emit_bl(bytes, relocs, *fid);
            emit_saves(bytes, saves, false);
            let done_at = bytes.len();
            patch(bytes, to_done, done_at);
        }
        _ => unreachable!("inline_class admitted a call of another shape"),
    }
    true
}

fn emit_saves(bytes: &mut Vec<u8>, saves: &[(Reg, u32)], save: bool) {
    for &(reg, off) in saves {
        store(bytes, reg, off, save);
    }
}

fn emit_bl(bytes: &mut Vec<u8>, relocs: &mut Vec<Reloc>, fid: FuncId) {
    relocs.push(Reloc {
        byte_offset: bytes.len() as u32,
        kind: RelocKind::CallSite {
            target: CallTarget::Func(fid),
        },
    });
    write_u32(bytes, bl_imm26(0));
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use torajs_core::ssa::ValueId;

    use super::*;
    use crate::compile::slot;
    use crate::enc::{adrp, b_imm26, cbz_x, movz_imm, tbnz};

    const CELLS: CellOps = CellOps {
        alloc: FuncId(3),
        free: FuncId(4),
    };
    // 48 B: the 64 class
    const CLASS: u32 = 2;
    const COUNT_OFF: u32 = TLAB_DEPTH_OFFSET as u32 + CLASS;
    const ROW_OFF: u16 = (CLASS * TLAB_CLASS_SLOTS_STRIDE as u32) as u16;

    fn words(bytes: &[u8]) -> Vec<u32> {
        bytes
            .chunks(4)
            .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
            .collect()
    }

    fn call(fid: FuncId, args: Vec<Operand>, result: Option<u32>) -> Inst {
        Inst {
            result: result.map(ValueId),
            kind: InstKind::Call(fid, args),
            origin: None,
        }
    }

    #[test]
    fn only_constant_classed_cell_calls_open_inline() {
        let class = |inst: &Inst| CELLS.inline_class(&inst.kind);
        let p = Operand::Value(ValueId(0));
        assert_eq!(
            class(&call(CELLS.alloc, vec![Operand::ConstI64(48)], Some(1))),
            Some(2)
        );
        assert_eq!(
            class(&call(CELLS.alloc, vec![Operand::ConstI64(32)], Some(1))),
            Some(1)
        );
        assert_eq!(
            class(&call(
                CELLS.free,
                vec![p.clone(), Operand::ConstI64(64)],
                None
            )),
            Some(2)
        );
        // no size class: zero, and past the largest
        assert_eq!(
            class(&call(CELLS.alloc, vec![Operand::ConstI64(0)], Some(1))),
            None
        );
        assert_eq!(
            class(&call(CELLS.alloc, vec![Operand::ConstI64(4097)], Some(1))),
            None
        );
        // a size only known at run time
        assert_eq!(class(&call(CELLS.alloc, vec![p.clone()], Some(1))), None);
        // another callee
        assert_eq!(
            class(&call(FuncId(9), vec![Operand::ConstI64(48)], Some(1))),
            None
        );
    }

    #[test]
    fn alloc_pops_the_row_and_calls_only_when_it_is_empty() {
        let alloc =
            Assignment::from_parts([(1, Reg::Gpr(Gpr::X5))].into(), HashMap::new(), 0, 32, true);
        let inst = call(CELLS.alloc, vec![Operand::ConstI64(48)], Some(1));
        let (mut bytes, mut relocs) = (Vec::new(), Vec::new());
        let saves = [(Reg::Gpr(Gpr::X6), 24)];
        assert!(try_emit(
            &mut bytes,
            &mut relocs,
            &inst,
            &alloc,
            &saves,
            CELLS
        ));

        let (tlab, count, row) = (Gpr::X9, Gpr::X10, Gpr::X11);
        let mut expect: Vec<u8> = Vec::new();
        let mut w = |word: u32| expect.extend_from_slice(&word.to_le_bytes());
        w(adrp(tlab, 0));
        w(add_imm(tlab, tlab, 0));
        w(ldrb_w_imm12(count, tlab, COUNT_OFF));
        w(cbz_x(count, 6 * 4));
        w(sub_w_imm(count, count, 1));
        w(add_imm(row, tlab, ROW_OFF));
        w(ldr_x_reg_lsl3(Gpr::X5, row, count));
        w(strb_w_imm12(count, tlab, COUNT_OFF));
        w(b_imm26(6 * 4));
        slot::sp_str_x(&mut expect, Gpr::X6, 24);
        let mut w = |word: u32| expect.extend_from_slice(&word.to_le_bytes());
        w(movz_imm(Gpr::X0, 48, 0));
        w(bl_imm26(0));
        w(mov_x_reg(Gpr::X5, Gpr::X0));
        slot::sp_ldr_x(&mut expect, Gpr::X6, 24);
        assert_eq!(words(&bytes), words(&expect));

        // tlab page + pageoff, then the slow arm's call
        assert_eq!(relocs.len(), 3);
        assert_eq!(relocs[2].byte_offset, 11 * 4);
        assert!(matches!(
            &relocs[2].kind,
            RelocKind::CallSite { target: CallTarget::Func(f) } if *f == CELLS.alloc
        ));
    }

    #[test]
    fn free_pushes_the_row_and_calls_only_when_it_is_full() {
        let alloc = Assignment::from_parts(
            [(0, Reg::Gpr(Gpr::X19))].into(),
            HashMap::new(),
            0,
            32,
            true,
        );
        let p = Operand::Value(ValueId(0));
        let inst = call(CELLS.free, vec![p, Operand::ConstI64(48)], None);
        let (mut bytes, mut relocs) = (Vec::new(), Vec::new());
        assert!(try_emit(&mut bytes, &mut relocs, &inst, &alloc, &[], CELLS));

        let (tlab, count, row) = (Gpr::X9, Gpr::X10, Gpr::X11);
        let expect = [
            adrp(tlab, 0),
            add_imm(tlab, tlab, 0),
            ldrb_w_imm12(count, tlab, COUNT_OFF),
            tbnz(count, 4, 6 * 4),
            add_imm(row, tlab, ROW_OFF),
            str_x_reg_lsl3(Gpr::X19, row, count),
            add_w_imm(count, count, 1),
            strb_w_imm12(count, tlab, COUNT_OFF),
            b_imm26(4 * 4),
            mov_x_reg(Gpr::X0, Gpr::X19),
            movz_imm(Gpr::X1, 48, 0),
            bl_imm26(0),
        ];
        assert_eq!(words(&bytes), expect);
        assert_eq!(relocs[2].byte_offset, 11 * 4);
    }
}
