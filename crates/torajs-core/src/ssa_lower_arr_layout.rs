//! Array-layout helpers for `LowerCtx<'a>` extracted from
//! `ssa_lower.rs` chunk 371.
//!
//! T-13.5 deque head-x8 computation, slot byte-offset math
//! (`ARR_DATA_OFF + head_x8 + idx*stride`), and refcount walk loops
//! (`emit_arr_rc_drop_range` / `emit_arr_rc_inc_range`). Method bodies
//! are byte-for-byte preserved from the source; siblings and
//! `ssa_lower.rs` reach them through the impl block on the shared
//! `crate::ssa_lower::LowerCtx` type.

use crate::ast::{Expr, ExprId};
use crate::ssa::{BinOp as SsaBinOp, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{ARR_DATA_OFF, LowerCtx};

impl<'a> LowerCtx<'a> {
    /// T-13.5 deque: load `head * 8` from arr (the byte offset of
    /// logical[0] within the slot data section). Reads the packed
    /// u64 at offset 16 (low 32 = cap, high 32 = head, little-endian),
    /// extracts head via `LShr 32`, then `Shl 3` to scale to bytes.
    /// LICM hoists this out of any element-walk loop.
    pub(crate) fn emit_arr_head_x8(&mut self, arr: Operand) -> Operand {
        let packed = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, arr, 16),
            Type::I64,
            None,
        );
        let head = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(
                SsaBinOp::LShr,
                Operand::Value(packed),
                Operand::ConstI64(32),
            ),
            Type::I64,
            None,
        );
        let head_x8 = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Shl, Operand::Value(head), Operand::ConstI64(3)),
            Type::I64,
            None,
        );
        Operand::Value(head_x8)
    }

    /// T-13.5 deque: return byte offset of logical slot[idx] in arr,
    /// `24 + (idx + head) * 8`. Use at element-walk sites that may
    /// operate on a shifted array (Index, sort, map/filter/reduce
    /// closures, JSON.stringify, console.log). For literal-init paths
    /// where the array was just allocated and head=0, prefer
    /// `ARR_DATA_OFF + idx*8` directly to skip the head load.
    /// `stride_log2` is 3 for regular Array<T> (8-byte slots) and 4
    /// for Array<Any> (16-byte tagged slots); head is always counted
    /// in 8-byte units (matching the C-side macro contract).
    ///
    /// 11-A1: `is_non_deque = true` ⇒ skip head load + lshr + shl +
    /// extra add chain (5 → 2 arith ops). Caller proves safety via
    /// `arr_expr_is_non_deque` against `LowerCtx::deque_arrs`.
    pub(crate) fn emit_arr_slot_byte_offset(
        &mut self,
        arr: Operand,
        idx: Operand,
        stride_log2: i64,
        is_non_deque: bool,
    ) -> Operand {
        if is_non_deque {
            // 11-A1 fast-path: head ≡ 0 by escape analysis.
            let scaled = self.f.append_inst(
                self.cur_block,
                InstKind::BinOp(SsaBinOp::Shl, idx, Operand::ConstI64(stride_log2)),
                Type::I64,
                None,
            );
            let off = self.f.append_inst(
                self.cur_block,
                InstKind::BinOp(
                    SsaBinOp::Add,
                    Operand::Value(scaled),
                    Operand::ConstI64(ARR_DATA_OFF as i64),
                ),
                Type::I64,
                None,
            );
            return Operand::Value(off);
        }
        let head_x8 = self.emit_arr_head_x8(arr);
        let head_scaled = if stride_log2 == 3 {
            head_x8
        } else {
            // Array<Any>: head is in 8-byte units but slot stride is 16,
            // so the byte distance for `head` slots is head*16 = head_x8*2.
            let h2 = self.f.append_inst(
                self.cur_block,
                InstKind::BinOp(SsaBinOp::Shl, head_x8, Operand::ConstI64(stride_log2 - 3)),
                Type::I64,
                None,
            );
            Operand::Value(h2)
        };
        let scaled = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Shl, idx, Operand::ConstI64(stride_log2)),
            Type::I64,
            None,
        );
        let with_data = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(
                SsaBinOp::Add,
                Operand::Value(scaled),
                Operand::ConstI64(ARR_DATA_OFF as i64),
            ),
            Type::I64,
            None,
        );
        let off = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Add, Operand::Value(with_data), head_scaled),
            Type::I64,
            None,
        );
        Operand::Value(off)
    }

    /// 11-A1 — peek an array-receiving expr's binding name; returns
    /// true only for Ident receivers whose name is NOT in
    /// `deque_arrs` (conservative `false` for any non-Ident shape).
    pub(crate) fn arr_expr_is_non_deque(&self, eid: ExprId) -> bool {
        if let Expr::Ident(name) = self.ast.get_expr(eid) {
            !self.deque_arrs.contains(name)
        } else {
            false
        }
    }

    /// Walk slots [start, end) and call `emit_drop_value` on each
    /// element. Used by `arr.fill` / `arr.copyWithin` non-Copy paths
    /// to release the values that the operation is about to overwrite.
    pub(crate) fn emit_arr_rc_drop_range(
        &mut self,
        arr: Operand,
        elem_ty: Type,
        start: Operand,
        end: Operand,
    ) {
        let i_slot = self.alloca_in_entry(Type::I64, Some("__drp_i"));
        self.f.append_void(
            self.cur_block,
            InstKind::Store(start, Operand::Value(i_slot), 0),
        );
        // T-13.5 deque: hoist head_x8 out of the loop (cur_block is the
        // pre-loop block; head doesn't change during element-walk).
        let head_x8 = self.emit_arr_head_x8(arr.clone());
        let header = self.f.add_block();
        let body = self.f.add_block();
        let after = self.f.add_block();
        self.f.set_term(self.cur_block, Terminator::Br(header));
        self.cur_block = header;
        let i_now = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
            Type::I64,
            None,
        );
        let cond = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Slt, Operand::Value(i_now), end),
            Type::Bool,
            None,
        );
        self.f.set_term(
            self.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(cond),
                then_blk: body,
                else_blk: after,
            },
        );
        self.cur_block = body;
        let scaled = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Shl, Operand::Value(i_now), Operand::ConstI64(3)),
            Type::I64,
            None,
        );
        // T-13.5: off = scaled + ARR_DATA_OFF + head_x8
        let off_no_head = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(
                SsaBinOp::Add,
                Operand::Value(scaled),
                Operand::ConstI64(ARR_DATA_OFF as i64),
            ),
            Type::I64,
            None,
        );
        let off = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Add, Operand::Value(off_no_head), head_x8.clone()),
            Type::I64,
            None,
        );
        let elem = self.f.append_inst(
            self.cur_block,
            InstKind::LoadDyn(elem_ty, arr, Operand::Value(off)),
            elem_ty,
            None,
        );
        self.emit_drop_value(Operand::Value(elem), elem_ty);
        let i_next = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Add, Operand::Value(i_now), Operand::ConstI64(1)),
            Type::I64,
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::Value(i_next), Operand::Value(i_slot), 0),
        );
        self.f.set_term(self.cur_block, Terminator::Br(header));
        self.cur_block = after;
    }

    /// Phase B refcount: walk an array's element slots in [start, end)
    /// and call `__torajs_rc_inc` on each pointer. Used right after
    /// every Array helper that memcpy-copies element pointers (slice /
    /// toReversed / with / concat / spread / etc.) when the element
    /// type is non-Copy — the derived array now shares ownership of
    /// each element with the source, so inc balances the future
    /// element-walk drop on either array.
    ///
    /// `start` and `end` are i64 SSA operands (slot indices, not byte
    /// offsets). Generates an SSA `for (i = start; i < end; i++)` loop;
    /// LLVM mem2reg + loop opts collapse it to whatever the target ISA
    /// likes best.
    pub(crate) fn emit_arr_rc_inc_range(&mut self, arr: Operand, start: Operand, end: Operand) {
        let i_slot = self.alloca_in_entry(Type::I64, Some("__inc_i"));
        self.f.append_void(
            self.cur_block,
            InstKind::Store(start, Operand::Value(i_slot), 0),
        );
        // T-13.5 deque: hoist head_x8 out of the loop.
        let head_x8 = self.emit_arr_head_x8(arr.clone());
        let header = self.f.add_block();
        let body = self.f.add_block();
        let after = self.f.add_block();
        self.f.set_term(self.cur_block, Terminator::Br(header));
        // header: i < end ?
        self.cur_block = header;
        let i_now = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
            Type::I64,
            None,
        );
        let cond = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Slt, Operand::Value(i_now), end),
            Type::Bool,
            None,
        );
        self.f.set_term(
            self.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(cond),
                then_blk: body,
                else_blk: after,
            },
        );
        // body: rc_inc(arr[i]); i++
        self.cur_block = body;
        let scaled = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Shl, Operand::Value(i_now), Operand::ConstI64(3)),
            Type::I64,
            None,
        );
        // T-13.5: off = scaled + ARR_DATA_OFF + head_x8
        let off_no_head = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(
                SsaBinOp::Add,
                Operand::Value(scaled),
                Operand::ConstI64(ARR_DATA_OFF as i64),
            ),
            Type::I64,
            None,
        );
        let off = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Add, Operand::Value(off_no_head), head_x8.clone()),
            Type::I64,
            None,
        );
        let elem = self.f.append_inst(
            self.cur_block,
            InstKind::LoadDyn(Type::Ptr, arr, Operand::Value(off)),
            Type::Ptr,
            None,
        );
        self.emit_rc_inc(Operand::Value(elem));
        let i_next = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Add, Operand::Value(i_now), Operand::ConstI64(1)),
            Type::I64,
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::Value(i_next), Operand::Value(i_slot), 0),
        );
        self.f.set_term(self.cur_block, Terminator::Br(header));
        self.cur_block = after;
    }
}
