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
        let cur_arr = self.f.append_inst(
            self.cur_block,
            InstKind::Load(arr_ty, base.clone(), offset),
            arr_ty,
            None,
        );
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
                let val_v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.any_unbox_value, vec![v_raw.clone()]),
                    Type::I64,
                    None,
                );
                if !transfers {
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Call(
                            self.intrinsics.any_payload_rc_inc,
                            vec![Operand::Value(tag_v), Operand::Value(val_v)],
                        ),
                    );
                }
                let new_arr = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(
                        self.intrinsics.arr_push_any,
                        vec![
                            Operand::Value(cur_arr),
                            Operand::Value(tag_v),
                            Operand::Value(val_v),
                        ],
                    ),
                    arr_ty,
                    None,
                );
                self.f.append_void(
                    self.cur_block,
                    InstKind::Store(Operand::Value(new_arr), base, offset),
                );
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
            InstKind::Call(
                self.intrinsics.arr_push_any,
                vec![Operand::Value(cur_arr), Operand::ConstI64(tag), push_val],
            ),
            arr_ty,
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::Value(new_arr), base, offset),
        );
        let new_len = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, Operand::Value(new_arr), ARR_LEN_OFF),
            Type::I64,
            None,
        );
        Operand::Value(new_len)
    }
}
