//! Object cells through the allocator's thread cache, inline:
//! `Call(obj_alloc, [n])` and `Call(obj_drop_sized, [p, n])` with a
//! constant `n` that has a size class.

use torajs_core::ssa::{FuncId, Inst, InstKind, Module, Operand};
use torajs_mmalloc::size_class::{N_CLASSES, bucket_for};
use torajs_mmalloc::tlab::{
    FREE_LINK_OFF, SPILL_AT, TLAB_ROW_LEN_OFF, TLAB_ROW_STRIDE, TLAB_ROWS_OFFSET,
};

use super::rc_dec::{Fix, patch, place, store};
use super::refs::emit_adrp_add_pair;
use super::{
    OP_SCRATCH_LHS, OP_SCRATCH_RESULT_GPR, OP_SCRATCH_RHS, OP_SCRATCH_TMP, materialize_const_i64,
    materialize_operand_gpr, write_def_spill_gpr, write_u32,
};
use crate::enc::{
    add_imm, bl_imm26, ldp_x_off, ldr_x_imm12, mov_x_reg, stp_x_off, str_x_imm12, sub_imm,
};
use crate::reg::{Gpr, Reg};
use crate::regalloc::Assignment;
use crate::reloc::{CallTarget, Reloc, RelocKind};

/// `torajs_mmalloc::core::__torajs_core_tlab` by its Mach-O name: per
/// size class, a `{head, len}` stack of free blocks linked through
/// the blocks, which every sized allocation pops first and every
/// sized free pushes first.
pub(super) const TLAB_SYM: &str = "___torajs_core_tlab";
const ALLOC_NAME: &str = "__torajs_obj_alloc";
const FREE_NAME: &str = "__torajs_obj_drop_sized";

// a row never holds more than SPILL_AT blocks, so its length has
// this bit set exactly when it is full
const FULL_BIT: u32 = SPILL_AT.trailing_zeros();
const _: () = assert!(SPILL_AT.is_power_of_two());
const _: () = assert!(TLAB_ROWS_OFFSET == 0);
const _: () = assert!(TLAB_ROW_STRIDE == 16 && TLAB_ROW_LEN_OFF == 8);
// every row is one ldp / stp signed offset from the cache
const _: () = assert!((N_CLASSES - 1) * TLAB_ROW_STRIDE <= 504);
// the link word is one scaled ldr / str offset from the block
const _: () = assert!(FREE_LINK_OFF.is_multiple_of(8) && FREE_LINK_OFF < 8 * 4096);

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
        bucket_for(n).map(|c| c as u32)
    }
}

/// The cell call at `inst`, opened inline; `false` when it is not one.
///
/// ```text
/// alloc:                              free:
///     adrp+add x9, tlab                   adrp+add x9, tlab
///     ldp   xD, x10, [x9, #row[c]]        ldp   x10, x11, [x9, #row[c]]
///     cbz   xD, slow                      tbnz  x11, #FULL_BIT, slow
///     ldr   x11, [xD, #link]              str   x10, [xP, #link]
///     sub   x10, x10, #1                  add   x11, x11, #1
///     stp   x11, x10, [x9, #row[c]]       stp   xP, x11, [x9, #row[c]]
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
    let (tlab, len, link) = (OP_SCRATCH_LHS, OP_SCRATCH_RHS, OP_SCRATCH_TMP);
    let row_off = (class as usize * TLAB_ROW_STRIDE) as i32;
    match args.as_slice() {
        [Operand::ConstI64(n)] => {
            let result = inst.result.expect("obj_alloc must have a result");
            let (dst, spill_off) = alloc.def_gpr(result, OP_SCRATCH_RESULT_GPR);
            emit_adrp_add_pair(bytes, relocs, tlab, TLAB_SYM.to_string());
            write_u32(bytes, ldp_x_off(dst, len, tlab, row_off));
            let to_slow = place(bytes, Fix::Cbz(dst));
            write_u32(bytes, ldr_x_imm12(link, dst, FREE_LINK_OFF as u32));
            write_u32(bytes, sub_imm(len, len, 1));
            write_u32(bytes, stp_x_off(link, len, tlab, row_off));
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
            let head = len;
            let len = link;
            emit_adrp_add_pair(bytes, relocs, tlab, TLAB_SYM.to_string());
            write_u32(bytes, ldp_x_off(head, len, tlab, row_off));
            let to_slow = place(bytes, Fix::Tbnz(len, FULL_BIT));
            write_u32(bytes, str_x_imm12(head, ptr, FREE_LINK_OFF as u32));
            write_u32(bytes, add_imm(len, len, 1));
            write_u32(bytes, stp_x_off(ptr, len, tlab, row_off));
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
    use torajs_core::ssa::id_hash::HashMap;

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
    const ROW_OFF: i32 = (CLASS as usize * TLAB_ROW_STRIDE) as i32;
    const LINK: u32 = FREE_LINK_OFF as u32;

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
            class(&call(CELLS.free, vec![p, Operand::ConstI64(64)], None)),
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
        assert_eq!(class(&call(CELLS.alloc, vec![p], Some(1))), None);
        // another callee
        assert_eq!(
            class(&call(FuncId(9), vec![Operand::ConstI64(48)], Some(1))),
            None
        );
    }

    #[test]
    fn alloc_pops_the_row_and_calls_only_when_it_is_empty() {
        let alloc = Assignment::from_parts(
            HashMap::from_iter([(1, Reg::Gpr(Gpr::X5))]),
            HashMap::default(),
            0,
            32,
            true,
        );
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

        let (tlab, len, link) = (Gpr::X9, Gpr::X10, Gpr::X11);
        let mut expect: Vec<u8> = Vec::new();
        let mut w = |word: u32| expect.extend_from_slice(&word.to_le_bytes());
        w(adrp(tlab, 0));
        w(add_imm(tlab, tlab, 0));
        w(ldp_x_off(Gpr::X5, len, tlab, ROW_OFF));
        w(cbz_x(Gpr::X5, 5 * 4));
        w(ldr_x_imm12(link, Gpr::X5, LINK));
        w(sub_imm(len, len, 1));
        w(stp_x_off(link, len, tlab, ROW_OFF));
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
        assert_eq!(relocs[2].byte_offset, 10 * 4);
        assert!(matches!(
            &relocs[2].kind,
            RelocKind::CallSite { target: CallTarget::Func(f) } if *f == CELLS.alloc
        ));
    }

    #[test]
    fn free_pushes_the_row_and_calls_only_when_it_is_full() {
        let alloc = Assignment::from_parts(
            HashMap::from_iter([(0, Reg::Gpr(Gpr::X19))]),
            HashMap::default(),
            0,
            32,
            true,
        );
        let p = Operand::Value(ValueId(0));
        let inst = call(CELLS.free, vec![p, Operand::ConstI64(48)], None);
        let (mut bytes, mut relocs) = (Vec::new(), Vec::new());
        assert!(try_emit(&mut bytes, &mut relocs, &inst, &alloc, &[], CELLS));

        let (tlab, head, len) = (Gpr::X9, Gpr::X10, Gpr::X11);
        let expect = [
            adrp(tlab, 0),
            add_imm(tlab, tlab, 0),
            ldp_x_off(head, len, tlab, ROW_OFF),
            tbnz(len, 8, 5 * 4),
            str_x_imm12(head, Gpr::X19, LINK),
            add_imm(len, len, 1),
            stp_x_off(Gpr::X19, len, tlab, ROW_OFF),
            b_imm26(4 * 4),
            mov_x_reg(Gpr::X0, Gpr::X19),
            movz_imm(Gpr::X1, 48, 0),
            bl_imm26(0),
        ];
        assert_eq!(words(&bytes), expect);
        assert_eq!(relocs[2].byte_offset, 10 * 4);
    }

    /// The template's row test is one bit, so the allocator's spill
    /// mark must be the power of two the bit names.
    #[test]
    fn full_bit_names_spill_at() {
        assert_eq!(1usize << FULL_BIT, SPILL_AT);
    }
}
