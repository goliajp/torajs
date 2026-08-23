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

    /// Logical-assignment fingerprint — `o[k] &&= v` desugars to
    /// `o[k] && (o[k] = v)` with the obj/index sub-ExprIds SHARED
    /// between the guard read and the assign target (the parser's
    /// deliberate clone fingerprint). §13.15.2 evaluates the
    /// Reference once, so the key expression runs ONCE and — for a
    /// coercing key — ToPropertyKey runs once (§6.2.5 GetValue writes
    /// it back). When the fingerprint matches, evaluate (and for the
    /// keyed lanes coerce) the key up front and pin it in
    /// `compound_key_memo`; every index lane consults the memo. The
    /// caller clears the memo and, when this returns an owned cell
    /// (`true`), releases it after the join.
    ///
    /// String/Symbol-typed keys stay un-pinned for now — they ride
    /// dedicated lanes without a memo consult (recorded boundary; a
    /// bare Ident key there is side-effect-free anyway).
    pub(crate) fn pin_logical_assign_key(&mut self, left: ExprId, right: ExprId) -> Option<bool> {
        let Expr::Index { obj: o1, index: i1 } = self.ast.get_expr(left) else {
            return None;
        };
        let (o1, i1) = (*o1, *i1);
        let Expr::Assign { target, .. } = self.ast.get_expr(right) else {
            return None;
        };
        let Expr::Index { obj: o2, index: i2 } = self.ast.get_expr(*target) else {
            return None;
        };
        if *o2 != o1 || *i2 != i1 {
            return None;
        }
        match self.expr_types.get(&i1) {
            Some(
                crate::check::Type::Any
                | crate::check::Type::Undefined
                | crate::check::Type::Struct(_),
            ) => {
                let k_raw = crate::ssa_lower_index_any_key::lower_any_key(self, i1);
                let transfers = self.expr_transfers_ownership(i1);
                let cur_block = self.cur_block;
                let p = self.f.append_inst(
                    cur_block,
                    crate::ssa::InstKind::Call(
                        self.intrinsics.anyv_to_property_key,
                        vec![k_raw.clone()],
                    ),
                    Type::Ptr,
                    None,
                );
                self.emit_throw_check(None);
                if transfers {
                    self.emit_drop_value(k_raw, Type::Any);
                }
                let cur_block = self.cur_block;
                let boxed = self.f.append_inst(
                    cur_block,
                    crate::ssa::InstKind::Call(
                        self.intrinsics.any_box,
                        vec![Operand::ConstI64(4), Operand::Value(p)],
                    ),
                    Type::Any,
                    None,
                );
                self.compound_key_memo = Some((i1, Operand::Value(boxed)));
                Some(true)
            }
            Some(crate::check::Type::Number) => {
                let v = self.lower_expr(i1);
                self.compound_key_memo = Some((i1, v));
                Some(false)
            }
            _ => None,
        }
    }

    /// Release the cell [`Self::pin_logical_assign_key`] pinned (when
    /// it answered `true`) and clear the memo. Runs after the join so
    /// both the taken and short-circuited paths see the release.
    pub(crate) fn unpin_logical_assign_key(&mut self, owned: bool) {
        if let Some((_, op)) = self.compound_key_memo.take()
            && owned
        {
            self.emit_drop_value(op, Type::Any);
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
        // Stake before the store (consuming receivers release the
        // transferred pair inside the kernel).
        crate::ssa_lower_index_assign::mint_index_assign_value(self, eid, &v_raw);
        crate::ssa_lower_assign_member_any::emit_any_member_set_dyn(
            self,
            self.intrinsics.any_member_set,
            obj_val,
            key_v,
            -1,
            tag_op,
            value_op,
            &obj_ident,
            recv_owned,
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
        // Cluster #4 logical-assign fingerprint — the guard layer
        // pinned this key (evaluated + coerced once); reuse it, no
        // re-lower, no drop (the pinning layer owns the cell).
        if let Some((mid, mop)) = &self.compound_key_memo
            && *mid == index
        {
            let k = mop.clone();
            let v_raw = self.lower_expr(value);
            let v_ty = self.operand_ty(&v_raw);
            let (tag_op, value_op) = self.pack_any_slot_value_shared(value, &v_raw, v_ty);
            let recv_slot = self.resolve_any_recv_slot(obj);
            // Stake before the store (consuming receivers release
            // the transferred pair inside the kernel).
            crate::ssa_lower_index_assign::mint_index_assign_value(self, eid, &v_raw);
            let cur_block = self.cur_block;
            self.f.append_void(
                cur_block,
                crate::ssa::InstKind::Call(
                    self.intrinsics.any_index_set_keyed,
                    vec![obj_val, k, tag_op, value_op, recv_slot],
                ),
            );
            self.emit_throw_check(None);
            return v_raw;
        }
        let k_raw = crate::ssa_lower_index_any_key::lower_any_key(self, index);
        let key_transfers = self.expr_transfers_ownership(index);
        // Cluster #4 T4 — the compound desugar (`o[k] op= v` →
        // `o[k] = o[k] op v`) shares the obj/index sub-ExprIds
        // between target and embedded read (the parser's deliberate
        // fingerprint). §6.2.5 GetValue writes ToPropertyKey's answer
        // back into the Reference Record, so a coercing key (object
        // with its own toString) runs ONCE. Mirror: coerce here, pin
        // the coerced cell in `compound_key_memo`, and both the
        // embedded read and the store below reuse it.
        let compound_shared = if let Expr::BinOp { left, .. } = self.ast.get_expr(value) {
            let left = *left;
            matches!(self.ast.get_expr(left),
                Expr::Index { obj: o2, index: i2 } if *o2 == obj && *i2 == index)
        } else {
            false
        };
        let k_final = if compound_shared {
            let cur_block = self.cur_block;
            let p = self.f.append_inst(
                cur_block,
                crate::ssa::InstKind::Call(
                    self.intrinsics.anyv_to_property_key,
                    vec![k_raw.clone()],
                ),
                Type::Ptr,
                None,
            );
            // NULL answer = the key's ToString threw; propagate.
            self.emit_throw_check(None);
            if key_transfers {
                self.emit_drop_value(k_raw, Type::Any);
            }
            let cur_block = self.cur_block;
            // The coerced Str/Symbol cell boxes as a heap tag; the
            // kernels' cell arm takes it uncoerced.
            let boxed = self.f.append_inst(
                cur_block,
                crate::ssa::InstKind::Call(
                    self.intrinsics.any_box,
                    vec![Operand::ConstI64(4), Operand::Value(p)],
                ),
                Type::Any,
                None,
            );
            self.compound_key_memo = Some((index, Operand::Value(boxed)));
            Operand::Value(boxed)
        } else {
            k_raw
        };
        let v_raw = self.lower_expr(value);
        if compound_shared {
            self.compound_key_memo = None;
        }
        let v_ty = self.operand_ty(&v_raw);
        let (tag_op, value_op) = self.pack_any_slot_value_shared(value, &v_raw, v_ty);
        let recv_slot = self.resolve_any_recv_slot(obj);
        // Stake before the store (consuming receivers release the
        // transferred pair inside the kernel).
        crate::ssa_lower_index_assign::mint_index_assign_value(self, eid, &v_raw);
        let cur_block = self.cur_block;
        self.f.append_void(
            cur_block,
            crate::ssa::InstKind::Call(
                self.intrinsics.any_index_set_keyed,
                vec![obj_val, k_final.clone(), tag_op, value_op, recv_slot],
            ),
        );
        self.emit_throw_check(None);
        // The coerced cell carries the to_property_key +1 (its box is
        // a pure encode) — release it; the uncoerced path keeps the
        // original owned-temp contract.
        if compound_shared || key_transfers {
            self.emit_drop_value(k_final, Type::Any);
        }
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
        // Stake before the store (consuming receivers release the
        // transferred pair inside the kernel).
        crate::ssa_lower_index_assign::mint_index_assign_value(self, eid, &v_raw);
        crate::ssa_lower_assign_member_any::emit_any_member_set_dyn(
            self,
            self.intrinsics.any_member_set,
            obj_val,
            key_v,
            -1,
            tag_op,
            value_op,
            &obj_ident,
            recv_owned,
        );
        if key_transfers {
            self.emit_drop_value(key_raw_keep, k_ty);
        }
        v_raw
    }
}
