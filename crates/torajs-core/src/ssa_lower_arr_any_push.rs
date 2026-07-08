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
        // binding, never a move: no consume. Borrow-shape args take
        // +1 for the slot; owned temps (Call / BinOp / view mints)
        // transfer their fresh reference into the slot instead.
        let transfers = self.expr_transfers_ownership(arg_id);
        let v_ty = self.operand_ty(&v_raw);
        let (tag, push_val): (i64, Operand) = match v_ty {
            Type::I64 | Type::I32 => (2, v_raw),
            Type::F64 => {
                let bits = self.f.append_inst(
                    self.cur_block,
                    InstKind::BitCastF64ToI64(v_raw),
                    Type::I64,
                    None,
                );
                (3, Operand::Value(bits))
            }
            Type::Bool => {
                let zext = self.f.append_inst(
                    self.cur_block,
                    InstKind::ZExtBoolToI64(v_raw),
                    Type::I64,
                    None,
                );
                (1, Operand::Value(zext))
            }
            // Type::Any must be handled BEFORE the is_refcounted
            // catch-all (Type::Any is itself refcounted — the
            // catch-all's raw header inc only worked by grace of
            // rc_inc's nan_box_is_cell_like guard, and stamped
            // tag=4 over immediates). Decode the (tag, value) pair
            // so the slot records the real runtime tag.
            Type::Any => {
                let tag_v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.any_unbox_tag, vec![v_raw.clone()]),
                    Type::I64,
                    None,
                );
                // Chunk 610 — borrow-shape args take the slot's +1
                // through the owned unbox (fuses the old separate
                // payload_rc_inc, which double-counted a ShortStr's
                // materialized rc=1 Str and leaked); an owned temp
                // transfers its fresh reference via the plain unbox
                // (a ShortStr materialization IS that fresh ref).
                let unbox_fid = if transfers {
                    self.intrinsics.any_unbox_value
                } else {
                    self.intrinsics.any_unbox_value_owned
                };
                let val_v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(unbox_fid, vec![v_raw.clone()]),
                    Type::I64,
                    None,
                );
                let new_arr = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(
                        grow_fid,
                        vec![cur_arr, Operand::Value(tag_v), Operand::Value(val_v)],
                    ),
                    arr_ty,
                    None,
                );
                // Chunk 628 — a typed block behind the Arr<Any> view
                // records a pending TypeError on kind mismatch;
                // propagate it (pre-fix the throw sat silently).
                self.emit_throw_check(None);
                // B1 — cell fixed across grow; slot write-back retired.
                let new_len = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(Type::I64, Operand::Value(new_arr), ARR_LEN_OFF),
                    Type::I64,
                    None,
                );
                return Operand::Value(new_len);
            }
            _ if v_ty.is_refcounted() => {
                // ANY_HEAP slot — a borrow-shape arg takes +1 so
                // the slot owns a balanced ref while the source
                // binding keeps its stake; an owned temp transfers
                // its fresh reference (chunk 565).
                if !transfers {
                    self.emit_rc_inc(v_raw.clone());
                }
                (4, v_raw)
            }
            Type::Ptr => {
                if matches!(v_raw, Operand::ConstPtrNull) {
                    (0, Operand::ConstI64(0))
                } else {
                    (4, v_raw)
                }
            }
            _ => panic!("ssa-lower: Array<Any>.push unsupported value type {v_ty:?}"),
        };
        let new_arr = self.f.append_inst(
            self.cur_block,
            InstKind::Call(grow_fid, vec![cur_arr, Operand::ConstI64(tag), push_val]),
            arr_ty,
            None,
        );
        // Chunk 628 — kind-mismatch pending TypeError propagation
        // (see the Any arm above).
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
