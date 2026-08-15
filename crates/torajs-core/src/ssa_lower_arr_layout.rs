//! Array-layout helpers for `LowerCtx<'a>` extracted from
//! `ssa_lower.rs` chunk 371.
//!
//! T-13.5 deque head-x8 computation, slot address math (data-ptr
//! load + `head_x8 + idx*stride` — RFC 20260706-arr-grow-alias-
//! stability B1: slots live behind the cell's data pointer, so every
//! element access is a two-level load), and refcount walk loops
//! (`emit_arr_rc_drop_range` / `emit_arr_rc_inc_range`).

use crate::ast::{Expr, ExprId};
use crate::ssa::{BinOp as SsaBinOp, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{ARR_DATA_PTR_OFF, LowerCtx};

impl<'a> LowerCtx<'a> {
    /// §9.4.2.3 ArraySpeciesCreate constructor-face guard (RFC
    /// 20260713-array-proto-residual blade 3) — emitted at the head
    /// of every species-family method arm (concat / filter / flat /
    /// map / slice / splice; NOT the change-array-by-copy family,
    /// which always builds plain Arrays per §23.1.3.33+). The
    /// runtime records a TypeError for a present non-object
    /// non-undefined `constructor` expando; the throw check
    /// propagates it before the derive kernel runs. Fast path is a
    /// props-NULL load inside the helper.
    pub(crate) fn emit_arr_species_guard(&mut self, recv: Operand) {
        self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.arr_species_guard, vec![recv]),
            Type::I64,
            None,
        );
        self.emit_throw_check(None);
    }

    /// §23.1.3.{20,29} pop/shift length-write lock (RFC 20260713
    /// blade 4) — `Set(O, "length", …, true)` on a frozen array or a
    /// non-writable `length` throws TypeError, INCLUDING the empty
    /// receiver (step 3.b writes length 0), so this precedes the
    /// empty short-circuit. Inline hot path: one header load +
    /// shift/mask/branch (the flags word rides the same cache line
    /// as the len the op reads next); the cold block calls the
    /// runtime thrower and propagates.
    pub(crate) fn emit_arr_len_lock_guard(&mut self, arr: Operand) {
        const LOCK_BITS: i64 = 16 + 128; // FLAG_FROZEN | FLAG_ARR_LENGTH_RO
        let hdr = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, arr.clone(), 0),
            Type::I64,
            None,
        );
        let sh = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::LShr, Operand::Value(hdr), Operand::ConstI64(48)),
            Type::I64,
            None,
        );
        let masked = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(
                SsaBinOp::And,
                Operand::Value(sh),
                Operand::ConstI64(LOCK_BITS),
            ),
            Type::I64,
            None,
        );
        let locked = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Ne, Operand::Value(masked), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        let locked_blk = self.f.add_block();
        let ok_blk = self.f.add_block();
        let cb = self.cur_block;
        self.f.set_term(
            cb,
            Terminator::CondBr {
                cond: Operand::Value(locked),
                then_blk: locked_blk,
                else_blk: ok_blk,
            },
        );
        self.cur_block = locked_blk;
        self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.arr_len_write_guard, vec![arr]),
            Type::I64,
            None,
        );
        self.emit_throw_check(None);
        let after_throw = self.cur_block;
        self.f.set_term(after_throw, Terminator::Br(ok_blk));
        self.cur_block = ok_blk;
    }

    /// B1 — load the slots base pointer from the cell's data field.
    /// Emit once per access site; grow-capable calls in between
    /// invalidate it (grow swaps the buffer, not the cell).
    pub(crate) fn emit_arr_data_ptr(&mut self, arr: Operand) -> Operand {
        let d = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::Ptr, arr, ARR_DATA_PTR_OFF),
            Type::Ptr,
            None,
        );
        Operand::Value(d)
    }

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

    /// T-13.5 deque: return `(slots_base, byte_off)` for logical
    /// slot[idx] — the caller passes `slots_base` as the LoadDyn /
    /// StoreDyn base and `byte_off` as the offset (B1: the base is
    /// the cell's data pointer, not the cell). Use at element-walk
    /// sites that may operate on a shifted array (Index, sort,
    /// map/filter/reduce closures, JSON.stringify, console.log).
    /// `stride_log2` is 3 for regular Array<T> (8-byte slots) and 4
    /// for Array<Any> (16-byte tagged slots); head is always counted
    /// in 8-byte units (matching the C-side macro contract).
    ///
    /// 11-A1: `is_non_deque = true` ⇒ skip head load + lshr + shl +
    /// extra add chain. Caller proves safety via
    /// `arr_expr_is_non_deque` against `LowerCtx::deque_arrs`.
    pub(crate) fn emit_arr_slot_byte_offset(
        &mut self,
        arr: Operand,
        idx: Operand,
        stride_log2: i64,
        is_non_deque: bool,
    ) -> (Operand, Operand) {
        let data = self.emit_arr_data_ptr(arr.clone());
        if is_non_deque {
            // 11-A1 fast-path: head ≡ 0 by escape analysis.
            let scaled = self.f.append_inst(
                self.cur_block,
                InstKind::BinOp(SsaBinOp::Shl, idx, Operand::ConstI64(stride_log2)),
                Type::I64,
                None,
            );
            return (data, Operand::Value(scaled));
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
        let off = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Add, Operand::Value(scaled), head_scaled),
            Type::I64,
            None,
        );
        (data, Operand::Value(off))
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
        // T-13.5 deque: hoist head_x8 + data ptr out of the loop
        // (cur_block is the pre-loop block; neither changes during an
        // element-walk — drop calls never grow this array).
        let head_x8 = self.emit_arr_head_x8(arr.clone());
        let data = self.emit_arr_data_ptr(arr);
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
        // off = scaled + head_x8 (relative to the hoisted data ptr)
        let off = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Add, Operand::Value(scaled), head_x8.clone()),
            Type::I64,
            None,
        );
        let elem = self.f.append_inst(
            self.cur_block,
            InstKind::LoadDyn(elem_ty, data.clone(), Operand::Value(off)),
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
    /// and inc each one. Used right after every Array helper that
    /// memcpy-copies element pointers (slice / toReversed / with /
    /// concat / spread / etc.) when the element type is non-Copy —
    /// the derived array now shares ownership of each element with
    /// the source, so inc balances the future element-walk drop on
    /// either array.
    ///
    /// `elem_ty` picks the inc kernel (rotation 412 — the mirror of
    /// `emit_arr_rc_drop_range`'s type-awareness): an `Arr<Any>`
    /// slot holds a NaN-box ENCODING, not a header pointer, so it
    /// must ride the payload-gated `any_box_rc_inc` (immediates
    /// no-op). The old unconditional `__torajs_rc_inc` dereferenced
    /// a boxed bool's payload as an address — SIGSEGV at 0x7 the
    /// first time `Array.from(<any[]>)` ran (the ctor rest relay).
    ///
    /// `start` and `end` are i64 SSA operands (slot indices, not byte
    /// offsets). Generates an SSA `for (i = start; i < end; i++)` loop;
    /// LLVM mem2reg + loop opts collapse it to whatever the target ISA
    /// likes best.
    pub(crate) fn emit_arr_rc_inc_range(
        &mut self,
        arr: Operand,
        elem_ty: Type,
        start: Operand,
        end: Operand,
    ) {
        let i_slot = self.alloca_in_entry(Type::I64, Some("__inc_i"));
        self.f.append_void(
            self.cur_block,
            InstKind::Store(start, Operand::Value(i_slot), 0),
        );
        // T-13.5 deque: hoist head_x8 + data ptr out of the loop.
        let head_x8 = self.emit_arr_head_x8(arr.clone());
        let data = self.emit_arr_data_ptr(arr);
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
        // off = scaled + head_x8 (relative to the hoisted data ptr)
        let off = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Add, Operand::Value(scaled), head_x8.clone()),
            Type::I64,
            None,
        );
        let load_ty = if elem_ty == Type::Any {
            Type::Any
        } else {
            Type::Ptr
        };
        let elem = self.f.append_inst(
            self.cur_block,
            InstKind::LoadDyn(load_ty, data.clone(), Operand::Value(off)),
            load_ty,
            None,
        );
        let body_blk = self.cur_block;
        self.emit_owned_result_inc_in(body_blk, Operand::Value(elem), elem_ty);
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
