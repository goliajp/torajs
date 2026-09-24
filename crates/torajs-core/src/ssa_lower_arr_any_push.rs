//! `Array<Any>.push(v)` lowering — emit the tagged-slot push for
//! an array pointer that lives at `base + offset`. Shared by the
//! local-Ident dispatch arm (alloca slot, offset=0), the K.8
//! const-global Ident arm (GlobalRef slot, offset=0), and the (b)
//! struct-field receiver arm (obj ptr, offset=field). Pre-fix the
//! K.8 and (b) sites routed Array<Any>.push through typed
//! `arr_push` + a stray `rc_inc(ConstI64(1))` (Type::Any reads as
//! refcounted), wrote raw int into the 16-byte slot allocated by
//! K.6 `arr_alloc_any(0)` / class field default, and SIGSEGV'd on
//! the next decode. Lifted out of `ssa_lower.rs` so all three
//! dispatch sites share one implementation and the file-size debt
//! rule is honoured.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};

impl<'a> LowerCtx<'a> {
    /// `xs.push(v)` for `Array<Any>` — load cur arr from `base +
    /// offset`, NaN-box-encode `v` into a (tag,value) pair, append
    /// via `__torajs_arr_push_any` (16-byte tagged-slot stride),
    /// store the realloc'd ptr back at the same site, return the
    /// new length per spec §22.1.3.20. `base + offset` covers:
    /// alloca slot (local), GlobalRef slot (const global), and
    /// struct field (obj + field offset).
    pub(crate) fn emit_arr_any_push_at_slot(
        &mut self,
        base: Operand,
        offset: u64,
        arg_id: ExprId,
        arr_ty: Type,
    ) -> Operand {
        let fid = self.intrinsics.arr_push_any;
        self.emit_arr_any_grow_at_slot(base, offset, arg_id, arr_ty, fid)
    }

    /// Chunk 697 — `xs[i].push(v)` for an already-loaded `Array<Any>`
    /// receiver value (index-read receivers have no slot to hand
    /// over; B1 fixed the cell across grow so the slot was only ever
    /// used for the initial load anyway — the unshift twin below).
    pub(crate) fn emit_arr_any_push_at_value(
        &mut self,
        cur_arr: Operand,
        arg_id: ExprId,
        arr_ty: Type,
    ) -> Operand {
        let fid = self.intrinsics.arr_push_any;
        self.emit_arr_any_grow_at_value(cur_arr, arg_id, arr_ty, fid)
    }

    /// Chunk 628 — `xs.unshift(v)` for an already-loaded `Array<Any>`
    /// receiver value (Member-expr receivers like `b.arr.unshift(v)`
    /// have no slot to hand over; B1 fixed the cell across grow so
    /// the slot was only ever used for the initial load anyway).
    pub(crate) fn emit_arr_any_unshift_at_value(
        &mut self,
        cur_arr: Operand,
        arg_id: ExprId,
        arr_ty: Type,
    ) -> Operand {
        let fid = self.intrinsics.arr_unshift_any;
        self.emit_arr_any_grow_at_value(cur_arr, arg_id, arr_ty, fid)
    }

    /// Shared pack + call + store-back core for the Array<Any> grow
    /// mutators (push / unshift) — `grow_fid` is the (Ptr, I64, I64)
    /// → Ptr adopt-contract runtime helper to invoke.
    fn emit_arr_any_grow_at_slot(
        &mut self,
        base: Operand,
        offset: u64,
        arg_id: ExprId,
        arr_ty: Type,
        grow_fid: crate::ssa::FuncId,
    ) -> Operand {
        let cur_arr = self.f.append_inst(
            self.cur_block,
            InstKind::Load(arr_ty, base.clone(), offset),
            arr_ty,
            None,
        );
        self.emit_arr_any_grow_at_value(Operand::Value(cur_arr), arg_id, arr_ty, grow_fid)
    }

    /// Value-receiver core — pack the arg into a (tag, value) pair
    /// and invoke the adopt-contract grow helper on `cur_arr`.
    fn emit_arr_any_grow_at_value(
        &mut self,
        cur_arr: Operand,
        arg_id: ExprId,
        arr_ty: Type,
        grow_fid: crate::ssa::FuncId,
    ) -> Operand {
        let v_raw = self.lower_expr(arg_id);
        // Chunk 565 — pushing a value is a SHARE of the source
        // binding, never a move: the index-assign packer already
        // gives a borrow-shape arg the slot's +1 and lets an owned
        // temp transfer. This was a third copy of that pack.
        let v_ty = self.operand_ty(&v_raw);
        let (tag, push_val) = self.pack_any_slot_value_shared(arg_id, &v_raw, v_ty);
        let new_arr = self.f.append_inst(
            self.cur_block,
            InstKind::Call(grow_fid, vec![cur_arr, tag, push_val]),
            arr_ty,
            None,
        );
        // Chunk 628 — a typed block behind the Arr<Any> view records
        // a pending TypeError on kind mismatch; propagate it.
        self.emit_throw_check(None);
        // B1 — cell fixed across grow; slot write-back retired.
        let new_len = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, Operand::Value(new_arr), ARR_LEN_OFF),
            Type::I64,
            None,
        );
        Operand::Value(new_len)
    }
}
