//! `Array<Any>.fill(v, start?, end?)` lowering — NaN-box-encode `v`
//! into a (tag, value) pair, emit `__torajs_arr_fill_any` over the
//! clamped range. Lifted out of `ssa_lower_str.rs` so the Any-elem
//! arm doesn't grow the str-method dispatcher's known-debt and so
//! the (tag, value) encoding shape stays close to the sibling
//! `emit_arr_any_push_at_slot` helper.
//!
//! Pre-fix `arr.fill(v)` on Array<Any> typed-checked against
//! `v: Any` ([S127-4-style typecheck-only escape unblocks the
//! callable surface, but ssa-lower still wrote raw bits through
//! `__torajs_arr_fill` + the non-Copy per-slot loop with
//! `StoreDyn(value, off)` — `value` was a raw int, not a
//! NaN-boxed AnyValue], the slot decoded as garbage on the next
//! read and SIGSEGV'd in the heap-walker drop). The new runtime
//! helper `__torajs_arr_fill_any(arr, tag, value, start, end)`
//! does the box pack + per-slot drop + rc_inc on the runtime side
//! so the SSA emit stays a single Call.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};

impl<'a> LowerCtx<'a> {
    /// `xs.fill(v)` / `xs.fill(v, start)` / `xs.fill(v, start, end)`
    /// for `Array<Any>`. `recv_op` is the receiver ptr; `args` carry
    /// `[value, start?, end?]`. Returns the same receiver — fill is
    /// in-place and never reallocs.
    pub(crate) fn emit_arr_any_fill_at(
        &mut self,
        recv_op: Operand,
        args: &[ExprId],
        recv_ty: Type,
    ) -> Operand {
        let v_raw = self.lower_expr(args[0]);
        self.consume_if_ident(args[0]);
        let v_ty = self.operand_ty(&v_raw);
        let (tag_op, val_op): (Operand, Operand) = match v_ty {
            Type::I64 | Type::I32 => (Operand::ConstI64(2), v_raw),
            Type::F64 => {
                let bits = self.f.append_inst(
                    self.cur_block,
                    InstKind::BitCastF64ToI64(v_raw),
                    Type::I64,
                    None,
                );
                (Operand::ConstI64(3), Operand::Value(bits))
            }
            Type::Bool => {
                let zext = self.f.append_inst(
                    self.cur_block,
                    InstKind::ZExtBoolToI64(v_raw),
                    Type::I64,
                    None,
                );
                (Operand::ConstI64(1), Operand::Value(zext))
            }
            _ if v_ty.is_refcounted() => {
                // ANY_HEAP value — the runtime helper rc_inc's per
                // replaced slot, so the caller-side rc balance is
                // the value's pre-existing ref (which `consume_if
                // _ident` already shifted). No extra inc here.
                (Operand::ConstI64(4), v_raw)
            }
            Type::Ptr => {
                if matches!(v_raw, Operand::ConstPtrNull) {
                    (Operand::ConstI64(0), Operand::ConstI64(0))
                } else {
                    (Operand::ConstI64(4), v_raw)
                }
            }
            Type::Any => {
                // Already a NaN-boxed AnyValue — split into (tag,
                // value) so the helper can re-pack uniformly.
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
                (Operand::Value(tag_v), Operand::Value(val_v))
            }
            _ => panic!("ssa-lower: Array<Any>.fill unsupported value type {v_ty:?}"),
        };
        let len_for_norm = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, recv_op.clone(), ARR_LEN_OFF),
            Type::I64,
            None,
        );
        let start = if args.len() >= 2 {
            let raw = self.lower_expr(args[1]);
            self.relative_to_len(raw, Operand::Value(len_for_norm))
        } else {
            Operand::ConstI64(0)
        };
        let end = if args.len() >= 3 {
            let raw = self.lower_expr(args[2]);
            self.relative_to_len(raw, Operand::Value(len_for_norm))
        } else {
            Operand::Value(len_for_norm)
        };
        // S310 — lower-and-drop trailing args past the 3 useful
        // (value, start, end) slots per ES §23.1.3.7 trailing-arg
        // ignore. check.rs S246/S310 widen + ssa_lower_str S298
        // skip(3) loop already handle the typed-tier path; mirror
        // the Any-elem path for parity.
        for &a in args.iter().skip(3) {
            let _ = self.lower_expr(a);
        }
        let v = self.f.append_inst(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.arr_fill_any,
                vec![recv_op, tag_op, val_op, start, end],
            ),
            recv_ty,
            None,
        );
        Operand::Value(v)
    }
}
