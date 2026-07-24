//! Computed-key field family of the dynobj-init lane (RFC
//! 20260725-objlit-computed-key 刀 3), split from
//! `ssa_lower_dynobj_init.rs` at the 500-line boundary.
//!
//! `{ [expr]: v }` fields evaluate their key at runtime: the key expr
//! boxes to Any and coerces through the implicit ToString kernel
//! (§7.1.19 ToPropertyKey string face — a Symbol key throws; the
//! symbol-key substrate is a recorded boundary), then the value
//! stores under the runtime Str through the key-parameterized
//! `dynobj_set` core.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type, ValueId};
use crate::ssa_lower::LowerCtx;

impl LowerCtx<'_> {
    /// Key-parameterized core of `emit_dynobj_set` — the computed-key
    /// lane passes a runtime Str instead of an interned literal. The
    /// kernel rc-bumps the key on fresh insert, so the caller keeps
    /// (and must release) its own stake.
    pub(crate) fn emit_dynobj_set_key(
        &mut self,
        slot: ValueId,
        key: Operand,
        tag: Operand,
        val: Operand,
    ) {
        self.f.append_void(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.dynobj_set,
                vec![Operand::Value(slot), key, tag, val],
            ),
        );
    }

    /// Route one field store to the interned-name or runtime-key set.
    pub(crate) fn emit_dynobj_set_for(
        &mut self,
        slot: ValueId,
        fname: &str,
        runtime_key: Option<ValueId>,
        tag: Operand,
        val: Operand,
    ) {
        match runtime_key {
            Some(k) => self.emit_dynobj_set_key(slot, Operand::Value(k), tag, val),
            None => self.emit_dynobj_set(slot, fname, tag, val),
        }
    }

    /// One computed-key field: evaluate the key expr, coerce through
    /// the implicit ToString kernel, then store the value under the
    /// runtime key. Field order preserves the spec's evaluation
    /// order: key before value, both in literal position.
    pub(crate) fn emit_dynobj_computed_field(
        &mut self,
        slot: ValueId,
        key_eid: ExprId,
        fval_eid: ExprId,
    ) {
        let k_raw = self.lower_expr(key_eid);
        let k_ty = self.operand_ty(&k_raw);
        let k_boxed = if matches!(k_ty, Type::Any) {
            k_raw.clone()
        } else {
            self.box_to_any_from_expr(key_eid, k_raw.clone())
        };
        let key_str = self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.any_to_str_box, vec![k_boxed]),
            Type::Str,
            None,
        );
        self.release_owned_temp(key_eid, &k_raw);
        self.emit_throw_check(None);
        self.emit_dynobj_field_value(slot, "", fval_eid, Some(key_str));
        self.emit_drop_value(Operand::Value(key_str), Type::Str);
    }
}
