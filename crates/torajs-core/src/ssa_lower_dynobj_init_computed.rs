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

use crate::ast::PropKey;
use std::collections::HashSet;
use torajs_wtf8::Wtf8;

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type, ValueId};
use crate::ssa_lower::LowerCtx;

/// Which store kernel one literal field takes (r503, refining RFC
/// 20260825-inject-narrow-define 刀 3's per-literal `fresh` flag to a
/// per-field one).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SetLane {
    /// The literal has accessor members: the general `dynobj_set`
    /// (a later duplicate data key must dispatch the accessor entry
    /// it lands on).
    General,
    /// Accessor-free literal, and the key is provably not in the
    /// table yet — first of its static name, no computed key or
    /// spread evaluated before it: the insert-only
    /// `dynobj_set_fresh`, which carries no drop edge.
    FreshUnique,
    /// Accessor-free literal, key not provable: `dynobj_set_fresh_
    /// dup`, which keeps the duplicate-key found path (last write
    /// wins, the first value dropped).
    FreshDup,
}

impl LowerCtx<'_> {
    /// r503 — the per-field lane of one literal, in field order.
    /// Spread and `[[Prototype]]` members store nothing themselves
    /// (their lane is unread) but a spread, like a computed key,
    /// puts runtime keys in the table every later static key may
    /// collide with.
    pub(crate) fn objlit_set_lanes(&self, fields: &[(PropKey, ExprId)]) -> Vec<SetLane> {
        if !self.objlit_accessor_free(fields) {
            return vec![SetLane::General; fields.len()];
        }
        let mut seen: HashSet<&Wtf8> = HashSet::new();
        let mut runtime_keys = false;
        fields
            .iter()
            .map(|(fname, fval_eid)| {
                if self.ast.objlit_computed_keys.contains_key(fval_eid)
                    || crate::check_type_of_object_lit::spread_omit_set(fname).is_some()
                {
                    runtime_keys = true;
                    return SetLane::FreshDup;
                }
                if fname == "__proto__" && !self.ast.objlit_shorthand_proto_exprs.contains(fval_eid)
                {
                    return SetLane::FreshDup;
                }
                if runtime_keys || !seen.insert(fname.as_wtf8()) {
                    SetLane::FreshDup
                } else {
                    SetLane::FreshUnique
                }
            })
            .collect()
    }

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
        fresh: SetLane,
    ) {
        // RFC 20260825-inject-narrow-define 刀 3 / r503 — see
        // [`SetLane`].
        let kernel = match fresh {
            SetLane::General => self.intrinsics.dynobj_set,
            SetLane::FreshUnique => self.intrinsics.dynobj_set_fresh,
            SetLane::FreshDup => self.intrinsics.dynobj_set_fresh_dup,
        };
        self.f.append_void(
            self.cur_block,
            InstKind::Call(kernel, vec![Operand::Value(slot), key, tag, val]),
        );
    }

    /// 刀 3's per-literal judgment — true when no member of the
    /// literal installs an AccessorPair entry (shorthand
    /// `get`/`set` members or computed accessor faces), so no
    /// duplicate-key store can ever land on an accessor.
    pub(crate) fn objlit_accessor_free(&self, fields: &[(PropKey, ExprId)]) -> bool {
        fields.iter().all(|(fname, fval_eid)| {
            !self.ast.objlit_computed_accessors.contains_key(fval_eid)
                && !fname.starts_with("__getter_")
                && !fname.starts_with("__setter_")
        })
    }

    /// Emit the shared `dynobj_set` shape: intern the field name and
    /// call the set kernel `(slot, key, tag, val)`. Every field
    /// path (undefined, nested object, plain box, Any/Str runtime
    /// tag) ends here — chunk 819 consolidated the 5 repeated call
    /// sites; 刀 3 moved it next to its key-parameterized core.
    pub(crate) fn emit_dynobj_set(
        &mut self,
        slot: ValueId,
        fname: &Wtf8,
        tag: Operand,
        val: Operand,
        fresh: SetLane,
    ) {
        let key_str = self.intern_string_literal(fname);
        self.emit_dynobj_set_key(slot, Operand::Value(key_str), tag, val, fresh);
    }

    /// Route one field store to the interned-name or runtime-key set.
    pub(crate) fn emit_dynobj_set_for(
        &mut self,
        slot: ValueId,
        fname: &Wtf8,
        runtime_key: Option<ValueId>,
        tag: Operand,
        val: Operand,
        fresh: SetLane,
    ) {
        match runtime_key {
            Some(k) => self.emit_dynobj_set_key(slot, Operand::Value(k), tag, val, fresh),
            None => self.emit_dynobj_set(slot, fname, tag, val, fresh),
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
        fresh: SetLane,
    ) {
        // §7.1.19 step 2 — ToPropertyKey has two faces, and a Symbol
        // takes the one that does not coerce.
        if matches!(
            self.expr_types.get(&key_eid),
            Some(crate::check::Type::Symbol)
        ) {
            return self.emit_dynobj_computed_symbol_field(slot, key_eid, fval_eid, fresh);
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
        self.emit_dynobj_field_value(slot, Wtf8::new(""), fval_eid, Some(key_str), fresh);
        self.emit_drop_value(Operand::Value(key_str), Type::Str);
    }

    /// 565-03 — §10.2.9 SetFunctionName for a computed field whose
    /// value is an ANONYMOUS function definition (`{ [k]() {} }`,
    /// `{ [k]: () => {} }`, `{ [k]: function () {} }`): the name is
    /// the runtime key, which only this point knows. A value that
    /// already has a name of its own — a named function expression,
    /// an identifier reference — is left alone, exactly as §8.4.5
    /// leaves it.
    ///
    /// The class twin (564-01) carries the name on its reified face;
    /// an ordinary closure gets it as the own `name` property
    /// SetFunctionName is defined to create, because a per-instance
    /// word on the closure layout would be a word on every closure.
    pub(crate) fn emit_computed_field_fn_name(
        &mut self,
        fval_eid: ExprId,
        v_raw: &Operand,
        key: ValueId,
    ) {
        if !matches!(self.operand_ty(v_raw), Type::Closure(_)) {
            return;
        }
        let crate::ast::Expr::Closure { fn_name, .. } = self.ast.get_expr(fval_eid) else {
            return;
        };
        // The same two conditions Pass 2B's NamedEvaluation filter
        // uses: only a LIFTED body is an anonymous definition (a
        // `__forward_<target>` wrapper is an identifier reference to
        // a function that already has a name — `{ [k]: named }` must
        // stay `"named"`), and a named function expression's own
        // self-name wins over every syntactic position (§15.5.5).
        if !fn_name.starts_with("__closure_") || self.ast.closure_self_names.contains_key(fn_name) {
            return;
        }
        let cur_block = self.cur_block;
        self.f.append_void(
            cur_block,
            InstKind::Call(
                self.intrinsics.fn_computed_name_define,
                vec![v_raw.clone(), Operand::Value(key), Operand::ConstI64(0)],
            ),
        );
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
        fresh: SetLane,
    ) {
        let k_raw = self.lower_expr(key_eid);
        let k_ty = self.operand_ty(&k_raw);
        let key_transfers = self.expr_transfers_ownership(key_eid) && k_ty.is_refcounted();
        let key_keep = k_raw.clone();
        let Operand::Value(key_v) = k_raw else {
            panic!("ssa-lower: computed symbol key lowered to a non-value operand");
        };
        self.emit_dynobj_field_value(slot, Wtf8::new(""), fval_eid, Some(key_v), fresh);
        if key_transfers {
            self.emit_drop_value(key_keep, k_ty);
        }
    }
}
