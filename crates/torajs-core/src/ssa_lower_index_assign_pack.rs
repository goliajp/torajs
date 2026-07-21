//! Any-slot `(tag, value)` pair packing — split out of
//! `ssa_lower_index_assign.rs` (file-size hard limit; the RFC
//! 20260721-typed-grow-on-write OOB arm pushed it over 500).
//!
//! The pack family turns a lowered value operand into the raw
//! NaN-box pair the Array<Any> / dynobj slot runtimes store
//! directly. Ownership discipline (chunk 567/610): a borrow-shape
//! rhs takes +1 so the slot owns its stake; owned temps transfer.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

impl<'a> LowerCtx<'a> {
    /// Share-aware wrapper around [`Self::pack_any_slot_value`]
    /// (chunk 567): the Any-slot runtimes store the pair raw (the
    /// slot takes ownership of the passed reference), so a
    /// borrow-shape rhs takes +1 here — refcounted values via
    /// rc_inc, boxed Any through the owned unbox (chunk 610 — fuses
    /// the old separate payload inc, which double-counted a
    /// ShortStr's materialized rc=1 Str and leaked) — keeping the
    /// source binding's stake; owned temps transfer their fresh
    /// reference (a ShortStr materialization IS that fresh ref).
    pub(crate) fn pack_any_slot_value_shared(
        &mut self,
        value: ExprId,
        v_raw: &Operand,
        v_ty: Type,
    ) -> (Operand, Operand) {
        let transfers = self.expr_transfers_ownership(value);
        let (tag_op, value_op) = self.pack_any_slot_value(v_raw, v_ty, !transfers);
        if !transfers && !matches!(v_ty, Type::Any) && v_ty.is_refcounted() {
            self.emit_rc_inc(v_raw.clone());
        }
        (tag_op, value_op)
    }

    /// Pack the value into a (tag, value) operand pair using the same
    /// scheme as box_to_any but without the heap allocation.
    /// `any_owned` selects the owned unbox for an Any input (the
    /// pair carries the slot's +1; see pack_any_slot_value_shared).
    pub(crate) fn pack_any_slot_value(
        &mut self,
        v_raw: &Operand,
        v_ty: Type,
        any_owned: bool,
    ) -> (Operand, Operand) {
        match v_ty {
            Type::I64 | Type::I32 => (Operand::ConstI64(2), v_raw.clone()),
            Type::F64 => {
                let bits = self.f.append_inst(
                    self.cur_block,
                    InstKind::BitCastF64ToI64(v_raw.clone()),
                    Type::I64,
                    None,
                );
                (Operand::ConstI64(3), Operand::Value(bits))
            }
            Type::Bool => {
                let zext = self.f.append_inst(
                    self.cur_block,
                    InstKind::ZExtBoolToI64(v_raw.clone()),
                    Type::I64,
                    None,
                );
                (Operand::ConstI64(1), Operand::Value(zext))
            }
            Type::Any => {
                // Already boxed — extract tag/value via the
                // NaN-box decoders (Step 7e-A). Owned variant
                // hands the pair the slot's +1 (chunk 610).
                let tag_v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.any_unbox_tag, vec![v_raw.clone()]),
                    Type::I64,
                    None,
                );
                let unbox_fid = if any_owned {
                    self.intrinsics.any_unbox_value_owned
                } else {
                    self.intrinsics.any_unbox_value
                };
                let val_v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(unbox_fid, vec![v_raw.clone()]),
                    Type::I64,
                    None,
                );
                (Operand::Value(tag_v), Operand::Value(val_v))
            }
            _ if v_ty.is_refcounted() => (Operand::ConstI64(4), v_raw.clone()),
            Type::Ptr => {
                // Frontend `null` lowers to Type::Ptr
                // ConstPtrNull. Detect that constant shape and
                // emit ANY_NULL (tag=0); any other Ptr value is a
                // generic heap pointer (ANY_HEAP).
                if matches!(v_raw, Operand::ConstPtrNull) {
                    (Operand::ConstI64(0), Operand::ConstI64(0))
                } else {
                    (Operand::ConstI64(4), v_raw.clone())
                }
            }
            _ => panic!("ssa-lower: Array<Any>[i] = unsupported value type {v_ty:?}"),
        }
    }
}
