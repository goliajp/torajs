//! Computed-key field family of the dynobj-init lane (RFC
//! 20260725-objlit-computed-key 刀 3), split from
//! `ssa_lower_dynobj_init.rs` at the 500-line boundary.
//!
//! `{ [expr]: v }` fields evaluate their key at runtime and store
//! through the key-parameterized `dynobj_set` core. §7.1.19
//! ToPropertyKey picks which key the core receives:
//!
//! - **string face** — the key expr boxes to Any and coerces through
//!   the implicit ToString kernel; the value stores under that Str.
//! - **symbol face** (RFC 20260725-getiterator-getmethod 刀 1) — a
//!   Symbol key is handed over uncoerced, exactly as `o[sym] = v`
//!   hands it over. §6.1.7's two key domains are both 8-aligned heap
//!   cells and the dict keys off the cell's own `type_tag`, so the
//!   store core needs no second entry point — only the coerce differs.

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
        // §7.1.19 step 2 — ToPropertyKey has two faces, and a Symbol
        // takes the one that does not coerce.
        if matches!(
            self.expr_types.get(&key_eid),
            Some(crate::check::Type::Symbol)
        ) {
            return self.emit_dynobj_computed_symbol_field(slot, key_eid, fval_eid);
        }
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

    /// The Symbol half of [`Self::emit_dynobj_computed_field`] — the
    /// key cell IS the key, so there is no coerce and no ToString
    /// temp to release. Twin of the `o[sym] = v` lane
    /// (`lower_any_index_assign_symbol_key`), and it shares that
    /// lane's chunk-567 ledger: the set core READS the key (interning
    /// it into the bucket, not adopting it), so only an owned temp
    /// needs releasing — and it releases AFTER the store, since the
    /// store is what reads it.
    fn emit_dynobj_computed_symbol_field(
        &mut self,
        slot: ValueId,
        key_eid: ExprId,
        fval_eid: ExprId,
    ) {
        let k_raw = self.lower_expr(key_eid);
        let k_ty = self.operand_ty(&k_raw);
        let key_transfers = self.expr_transfers_ownership(key_eid) && k_ty.is_refcounted();
        let key_keep = k_raw.clone();
        let Operand::Value(key_v) = k_raw else {
            panic!("ssa-lower: computed symbol key lowered to a non-value operand");
        };
        self.emit_dynobj_field_value(slot, "", fval_eid, Some(key_v));
        if key_transfers {
            self.emit_drop_value(key_keep, k_ty);
        }
    }
}
