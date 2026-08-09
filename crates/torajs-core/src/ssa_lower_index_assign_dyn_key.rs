//! Dynamic (non-literal) property-key `o[k] = v` lowering on an `any`
//! receiver — the string-key and §6.1.7 symbol-key pair, split out of
//! [`crate::ssa_lower_index_assign`] to keep that file under the
//! size cap.
//!
//! Both hand the key to the key-parameterized member-set core rather
//! than an interned literal, and both follow the chunk-567 ledger: the
//! core READS the key (interning it into the bucket, not adopting it),
//! so a borrow-shaped key keeps its stake and its scope drop while an
//! owned temp is released after the call. They differ only in the
//! coerce — §7.1.19 sends a string key through ToString-shaped
//! materialization (a Substr view becomes an owned Str), while step 2
//! hands a Symbol to the store untouched.

use crate::ast::{Expr, ExprId};
use crate::ssa::{Operand, Type};
use crate::ssa_lower::LowerCtx;

impl<'a> LowerCtx<'a> {
    /// L3b #3 (chunk 527) — an Ident receiver rides its variable slot
    /// along so a dynobj store that resizes writes the fresh cell
    /// back (same two shapes as the member-set gate); other receivers
    /// pass NULL. Shared by the numeric and any-key set lanes.
    pub(crate) fn resolve_any_recv_slot(&mut self, obj: ExprId) -> Operand {
        if let Expr::Ident(n) = self.ast.get_expr(obj) {
            if let Some(info) = self.locals.get(n) {
                Operand::Value(info.slot)
            } else if self.globals.contains_key(n) {
                let name = n.clone();
                let gref = self.f.append_inst(
                    self.cur_block,
                    crate::ssa::InstKind::GlobalRef(name),
                    Type::Ptr,
                    None,
                );
                Operand::Value(gref)
            } else {
                Operand::ConstPtrNull
            }
        } else {
            Operand::ConstPtrNull
        }
    }

    /// Dynamic string key on an `any` receiver — store through the
    /// key-parameterized member-set core with the runtime Str cell
    /// as the key (a Substr view materializes to an owned temp
    /// released after the call).
    pub(crate) fn lower_any_index_assign_str_key(
        &mut self,
        eid: ExprId,
        obj: ExprId,
        obj_val: Operand,
        index: ExprId,
        value: ExprId,
    ) -> Operand {
        let obj_ident = if let Expr::Ident(n) = self.ast.get_expr(obj) {
            Some(n.clone())
        } else {
            None
        };
        let k_raw = self.lower_expr(index);
        let k_ty = self.operand_ty(&k_raw);
        // Chunk 567 — the key is READ by the set core (interned into
        // the bucket, not adopted): no consume, a borrow-shape key
        // keeps its stake; an owned-temp key (BinOp mint / Substr
        // view) releases after the call — was a 32B/iter leak.
        let key_transfers = self.expr_transfers_ownership(index) && k_ty.is_refcounted();
        let key_raw_keep = k_raw.clone();
        let key_owned = k_ty == Type::Substr;
        let key_op = self.coerce_to_str(k_raw, k_ty);
        let Operand::Value(key_v) = key_op else {
            panic!("ssa-lower: string index key lowered to a non-value operand");
        };
        let v_raw = self.lower_expr(value);
        // Chunk 567 — SHARE, no consume.
        let v_ty = self.operand_ty(&v_raw);
        let (tag_op, value_op) = self.pack_any_slot_value_shared(value, &v_raw, v_ty);
        let recv_owned = self.expr_transfers_ownership(obj);
        crate::ssa_lower_assign_member_any::emit_any_member_set_dyn(
            self, obj_val, key_v, -1, tag_op, value_op, &obj_ident, recv_owned,
        );
        if key_owned {
            // Substr view materialized to a fresh owned Str.
            self.emit_drop_value(key_op, Type::Str);
        }
        if key_transfers {
            // An owned-temp key (BinOp mint / fresh view) releases
            // its own reference too — substr_to_owned only reads.
            self.emit_drop_value(key_raw_keep, k_ty);
        }
        crate::ssa_lower_index_assign::mint_index_assign_value(self, eid, &v_raw);
        v_raw
    }

    /// `any`-typed key on an `any` receiver (cluster #1 blade 4) — no
    /// static lane can be picked, so the keyed set kernel does the
    /// §7.1.19 ToPropertyKey dispatch at runtime. Key is READ by the
    /// kernel (chunk-567 ledger, same as the str/symbol twins); the
    /// (tag, value) pair transfers into the store. Ident receivers
    /// ride their variable slot along for the dynobj-resize
    /// write-back, mirroring the numeric lane.
    pub(crate) fn lower_any_index_assign_any_key(
        &mut self,
        eid: ExprId,
        obj: ExprId,
        obj_val: Operand,
        index: ExprId,
        value: ExprId,
    ) -> Operand {
        let k_raw = crate::ssa_lower_index_any_key::lower_any_key(self, index);
        let key_transfers = self.expr_transfers_ownership(index);
        let v_raw = self.lower_expr(value);
        let v_ty = self.operand_ty(&v_raw);
        let (tag_op, value_op) = self.pack_any_slot_value_shared(value, &v_raw, v_ty);
        let recv_slot = self.resolve_any_recv_slot(obj);
        let cur_block = self.cur_block;
        self.f.append_void(
            cur_block,
            crate::ssa::InstKind::Call(
                self.intrinsics.any_index_set_keyed,
                vec![obj_val, k_raw.clone(), tag_op, value_op, recv_slot],
            ),
        );
        self.emit_throw_check(None);
        if key_transfers {
            self.emit_drop_value(k_raw, Type::Any);
        }
        crate::ssa_lower_index_assign::mint_index_assign_value(self, eid, &v_raw);
        v_raw
    }

    /// Symbol key on an `any` receiver — §7.1.19 step 2 hands the key
    /// to the store uncoerced, so the set core receives the Symbol
    /// cell itself and keys off its `type_tag`. Twin of
    /// [`Self::lower_any_index_assign_str_key`] minus the coerce:
    /// there is no view / wrapper form of a Symbol to materialize.
    pub(crate) fn lower_any_index_assign_symbol_key(
        &mut self,
        eid: ExprId,
        obj: ExprId,
        obj_val: Operand,
        index: ExprId,
        value: ExprId,
    ) -> Operand {
        let obj_ident = if let Expr::Ident(n) = self.ast.get_expr(obj) {
            Some(n.clone())
        } else {
            None
        };
        let k_raw = self.lower_expr(index);
        let k_ty = self.operand_ty(&k_raw);
        // Chunk 567 — the key is READ by the set core (interned into
        // the bucket, not adopted): a borrow-shape key keeps its
        // stake, an owned temp releases after the call.
        let key_transfers = self.expr_transfers_ownership(index) && k_ty.is_refcounted();
        let key_raw_keep = k_raw.clone();
        let Operand::Value(key_v) = k_raw else {
            panic!("ssa-lower: symbol index key lowered to a non-value operand");
        };
        let v_raw = self.lower_expr(value);
        let v_ty = self.operand_ty(&v_raw);
        let (tag_op, value_op) = self.pack_any_slot_value_shared(value, &v_raw, v_ty);
        let recv_owned = self.expr_transfers_ownership(obj);
        crate::ssa_lower_assign_member_any::emit_any_member_set_dyn(
            self, obj_val, key_v, -1, tag_op, value_op, &obj_ident, recv_owned,
        );
        if key_transfers {
            self.emit_drop_value(key_raw_keep, k_ty);
        }
        crate::ssa_lower_index_assign::mint_index_assign_value(self, eid, &v_raw);
        v_raw
    }
}
