//! `Array<Any>.push(v)` lowering — emit the tagged-slot push at a
//! given slot ptr operand, shared by the local-Ident dispatch arm
//! (alloca slot) and the K.8 const-global Ident arm (GlobalRef
//! slot). Pre-fix K.8 routed Array<Any>.push through typed
//! `arr_push` + a stray `rc_inc(ConstI64(1))` (Type::Any reads as
//! refcounted), wrote raw int into the 16-byte slot allocated by
//! K.6 (`arr_alloc_any(0)`), and SIGSEGV'd on the next decode.
//! Lifted out of `ssa_lower.rs` so both dispatch sites share one
//! implementation and the file-size debt rule is honoured.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};

impl<'a> LowerCtx<'a> {
    /// `xs.push(v)` for `Array<Any>` — load cur arr from `slot_op`,
    /// NaN-box-encode `v` into a (tag,value) pair, append via
    /// `__torajs_arr_push_any` (16-byte tagged-slot stride), store
    /// the realloc'd ptr back into `slot_op`, return the new length
    /// per spec §22.1.3.20. `slot_op` is either an alloca ValueId
    /// (local-receiver path) or a GlobalRef ValueId (top-level
    /// const-global-receiver path); both shapes carry the array ptr
    /// at offset 0.
    pub(crate) fn emit_arr_any_push_at_slot(
        &mut self,
        slot_op: Operand,
        arg_id: ExprId,
        arr_ty: Type,
    ) -> Operand {
        let cur_arr = self.f.append_inst(
            self.cur_block,
            InstKind::Load(arr_ty, slot_op.clone(), 0),
            arr_ty,
            None,
        );
        let v_raw = self.lower_expr(arg_id);
        self.consume_if_ident(arg_id);
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
            _ if v_ty.is_refcounted() => {
                // ANY_HEAP slot — bump rc so the array's slot owns
                // a balanced ref.
                self.emit_rc_inc(v_raw.clone());
                (4, v_raw)
            }
            Type::Ptr => {
                if matches!(v_raw, Operand::ConstPtrNull) {
                    (0, Operand::ConstI64(0))
                } else {
                    (4, v_raw)
                }
            }
            Type::Any => {
                // Already a NaN-boxed AnyValue — decode the
                // (tag, value) pair via the unbox shims and call
                // arr_push_any directly.
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
                    InstKind::Store(Operand::Value(new_arr), slot_op, 0),
                );
                let new_len = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(Type::I64, Operand::Value(new_arr), ARR_LEN_OFF),
                    Type::I64,
                    None,
                );
                return Operand::Value(new_len);
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
            InstKind::Store(Operand::Value(new_arr), slot_op, 0),
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
