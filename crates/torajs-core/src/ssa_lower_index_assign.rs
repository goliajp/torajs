//! `arr[i] = value` lowering — split out of the ssa_lower Assign arm
//! (file-size known-debt: ssa_lower.rs only shrinks).
//!
//! bug-327 C3 — the pre-fix emit had no bounds handling on either
//! tier: the Array<Any> runtime helper wrote (and dropped the garbage
//! "old value" of) whatever sat at `base + i*8`, and the typed tier's
//! inline StoreDyn did the same — silent heap corruption for small
//! OOB indices, SIGSEGV past the mapped page.
//!
//! Post-fix shape:
//!  - **Array<Any> + write-back receiver** (Ident bound to a local or
//!    a const-global): routes through `__torajs_arr_set_any_grow`,
//!    which implements the ES OOB-write semantics (grow, undefined
//!    holes, len = i+1) and may realloc — the returned pointer is
//!    stored back, mirroring the arr_push contract.
//!  - **Array<Any>, no write-back slot** (`getArr()[i] = v`): the
//!    plain `__torajs_arr_set_any` entry, which now raises a
//!    catchable RangeError on OOB instead of corrupting.
//!  - **Typed tier**: the inline StoreDyn is guarded by an `i < len`
//!    branch; OOB calls `__torajs_arr_oob_write_reject` (RangeError).
//!    Typed grow-on-write semantics are a tracked roadmap item (RFC
//!    20260613-test262-bug327-root-causes) — the loud reject replaces
//!    the silent corruption, not the spec behavior.

use crate::ast::{Expr, ExprId};
use crate::ssa::{BlockId, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};

/// Receiver shapes that historically had a slot to store a
/// realloc'd pointer back into — B1 retired the store itself (the
/// cell is fixed across grow); the enum survives purely as the
/// "may grow" gate. Widening non-receiver shapes to grow too is a
/// semantic change parked in the RFC.
enum WriteBack {
    /// Ident bound to a mutable local.
    Local,
    /// Ident bound to a K.3 const-global array.
    Global,
}

impl<'a> LowerCtx<'a> {
    /// The `Expr::Index` arm of Assign lowering. `obj[index] = value`.
    pub(crate) fn lower_index_assign(
        &mut self,
        obj: ExprId,
        index: ExprId,
        value: ExprId,
    ) -> Operand {
        // M1.4 — `arr[i] = value`. 11-A1: peek receiver before
        // consuming `obj` for the head-elision flag.
        let is_non_deque = self.arr_expr_is_non_deque(obj);
        let arr_val = self.lower_expr(obj);
        let arr_ty = self.operand_ty(&arr_val);
        // Any-dynamic-access RFC (20260704) S3-set — `recv[i] = v`
        // where recv is an `any` value: runtime kind-aware dispatch;
        // OOB → catchable RangeError, elem-kind mismatch → catchable
        // TypeError, so the throw check follows.
        if arr_ty == Type::Any {
            // L3b #13 (chunk 528) — string keys store by property
            // per ES ToPropertyKey. A compile-time literal rides
            // the member-assign path (`o["k"] = v` ≡ `o.k = v`,
            // lastIndex / length hints included); a dynamic string
            // key rides the key-parameterized core (no hint —
            // recorded boundary).
            if let Expr::String(lit) = self.ast.get_expr(index) {
                let lit = lit.clone();
                let obj_ident = if let Expr::Ident(n) = self.ast.get_expr(obj) {
                    Some(n.clone())
                } else {
                    None
                };
                let v_raw = self.lower_expr(value);
                // Chunk 567 — SHARE, no consume (see
                // pack_any_slot_value_shared).
                let v_ty = self.operand_ty(&v_raw);
                let (tag_op, value_op) = self.pack_any_slot_value_shared(value, &v_raw, v_ty);
                crate::ssa_lower_assign_member_any::emit_any_member_set(
                    self, arr_val, &lit, tag_op, value_op, &obj_ident,
                );
                return v_raw;
            }
            if matches!(
                self.expr_types.get(&index),
                Some(crate::check::Type::String)
            ) {
                return self.lower_any_index_assign_str_key(obj, arr_val, index, value);
            }
            let idx_val = self.lower_index_operand(index);
            let v_raw = self.lower_expr(value);
            // Chunk 567 — SHARE, no consume.
            let v_ty = self.operand_ty(&v_raw);
            let (tag_op, value_op) = self.pack_any_slot_value_shared(value, &v_raw, v_ty);
            // L3b #3 (chunk 527) — an Ident receiver rides its
            // variable slot along so a dynobj store that resizes
            // writes the fresh cell back (same two shapes as the
            // member-set gate); other receivers pass NULL.
            let recv_slot = if let Expr::Ident(n) = self.ast.get_expr(obj) {
                if let Some(info) = self.locals.get(n) {
                    Operand::Value(info.slot)
                } else if self.globals.contains_key(n) {
                    let name = n.clone();
                    let gref = self.f.append_inst(
                        self.cur_block,
                        InstKind::GlobalRef(name),
                        Type::Ptr,
                        None,
                    );
                    Operand::Value(gref)
                } else {
                    Operand::ConstPtrNull
                }
            } else {
                Operand::ConstPtrNull
            };
            let cur_block = self.cur_block;
            self.f.append_void(
                cur_block,
                InstKind::Call(
                    self.intrinsics.any_index_set,
                    vec![arr_val, idx_val, tag_op, value_op, recv_slot],
                ),
            );
            self.emit_throw_check(None);
            return v_raw;
        }
        let elem_ty = match arr_ty {
            Type::Arr(arr_id) => self.arr_layouts[arr_id.0 as usize],
            other => panic!("ssa-lower: index assign on non-array {other:?}"),
        };
        let idx_val = self.lower_index_operand(index);
        // bug-327 C3 — resolve the receiver's write-back slot (same
        // two shapes arr_push supports). Present → the growable
        // helper; absent → the plain entry whose OOB path is a loud
        // RangeError.
        let writeback: Option<WriteBack> = if let Expr::Ident(name) = self.ast.get_expr(obj) {
            if self.locals.contains_key(name) {
                Some(WriteBack::Local)
            } else if self.globals.contains_key(name) {
                Some(WriteBack::Global)
            } else {
                None
            }
        } else {
            None
        };
        // P0.10 — Array<Any>[i] = <concrete>. The Any slots are
        // 8-byte NaN-boxed AnyValues; the runtime helper boxes the
        // (tag, value) pair and writes the slot, dropping any old
        // heap cell. Skip the generic StoreDyn path (which would
        // bypass the box/drop bookkeeping).
        if matches!(elem_ty, Type::Any) {
            let v_raw = self.lower_expr(value);
            // Chunk 567 — SHARE, no consume: arr_set_any/_grow store
            // the pair raw (slot takes the passed reference), the
            // unfixed sibling of the chunk-565 push/fill lanes.
            let v_ty = self.operand_ty(&v_raw);
            let (tag_op, value_op) = self.pack_any_slot_value_shared(value, &v_raw, v_ty);
            self.emit_arr_set_any(writeback, arr_val, arr_ty, idx_val, tag_op, value_op);
            // Both entries can raise (dense-limit / temporary-receiver
            // RangeError) — propagate.
            self.emit_throw_check(None);
            return v_raw;
        }
        // Typed tier. The value lowers BEFORE the bounds guard — both
        // for ES evaluation order and because the join block returns
        // it (a write_blk-local def would be unreachable on the OOB
        // path).
        let v = self.lower_expr(value);
        // Chunk 575 — an array value entering a container slot must
        // be self-describing for the cycle walker: field-store /
        // any-box boundaries chain-mark whatever is nested at that
        // moment, but a LATER stored array was born UNSET and broke
        // the cycle walk there (86MB probe). No-op for non-Arr elems.
        self.emit_arr_mark_kind(&v, &elem_ty);
        // Chunk 567 — a typed-tier elem store SHARES the rhs: a
        // borrow-shape value takes +1 so the slot owns its stake
        // while the source binding keeps its own (re-assign
        // drop-old no longer steals it — UAF, probe-proven); owned
        // temps keep transferring their fresh reference.
        let transfers = self.expr_transfers_ownership(value);
        // W4 — align the stored value with the elem width. The
        // reverse direction (f64 value into an i64 elem) means the
        // width analysis missed a write site — loud over bit-punning.
        let v = match (elem_ty, self.operand_ty(&v)) {
            (Type::F64, Type::I64) => self.coerce_to_f64(v),
            (Type::I64, Type::F64) => panic!(
                "ssa-lower: f64 value into i64 array elem — \
                 container width analysis missed this write"
            ),
            _ => v,
        };
        let join_blk = self.emit_index_bounds_guard(&arr_val, &idx_val);
        // The slot's +1 lands inside the in-bounds write block — the
        // OOB path stores nothing and must not mint a stake.
        if !transfers && !elem_ty.is_copy() {
            self.emit_rc_inc(v.clone());
        }
        // T-13.5: head-aware byte offset for indexed assign.
        let (offset_base, offset) =
            self.emit_arr_slot_byte_offset(arr_val.clone(), idx_val, 3, is_non_deque);
        // Drop old elem if non-Copy. M1.2 MVP only ships i64
        // elements (Copy), so this branch currently never fires; lays
        // groundwork for non-Copy element types in a follow-up.
        if !elem_ty.is_copy() {
            let old = self.f.append_inst(
                self.cur_block,
                InstKind::LoadDyn(elem_ty, offset_base.clone(), offset.clone()),
                elem_ty,
                None,
            );
            self.emit_drop_value(Operand::Value(old), elem_ty);
        }
        self.f.append_void(
            self.cur_block,
            InstKind::StoreDyn(v.clone(), offset_base.clone(), offset),
        );
        let wb = self.cur_block;
        self.f.set_term(wb, Terminator::Br(join_blk));
        self.cur_block = join_blk;
        v
    }

    /// Share-aware wrapper around [`Self::pack_any_slot_value`]
    /// (chunk 567): the Any-slot runtimes store the pair raw (the
    /// slot takes ownership of the passed reference), so a
    /// borrow-shape rhs takes +1 here — refcounted values via
    /// rc_inc, boxed Any through the owned unbox (chunk 610 — fuses
    /// the old separate payload inc, which double-counted a
    /// ShortStr's materialized rc=1 Str and leaked) — keeping the
    /// source binding's stake; owned temps transfer their fresh
    /// reference (a ShortStr materialization IS that fresh ref).
    fn pack_any_slot_value_shared(
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
    fn pack_any_slot_value(
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

    /// Route the Any-slot write to the growable helper (write-back
    /// receiver present — the realloc'd pointer is stored back to the
    /// local slot / const-global) or the plain entry (loud RangeError
    /// on OOB).
    fn emit_arr_set_any(
        &mut self,
        writeback: Option<WriteBack>,
        arr_val: Operand,
        arr_ty: Type,
        idx_val: Operand,
        tag_op: Operand,
        value_op: Operand,
    ) {
        match writeback {
            Some(wb) => {
                let new_arr = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(
                        self.intrinsics.arr_set_any_grow,
                        vec![arr_val, idx_val, tag_op, value_op],
                    ),
                    arr_ty,
                    None,
                );
                // B1 — cell fixed across grow; write-back retired.
                let _ = (wb, new_arr);
            }
            None => {
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(
                        self.intrinsics.arr_set_any,
                        vec![arr_val, idx_val, tag_op, value_op],
                    ),
                );
            }
        }
    }

    /// bug-327 C3: guard the inline slot store with
    /// `0 <= idx && idx < len` (IPred has no unsigned compare, so
    /// the negative-index shape gets its own arm). Emits the OOB
    /// reject block; leaves `cur_block` on the in-bounds write block
    /// and returns the join block both paths branch to.
    fn emit_index_bounds_guard(&mut self, arr_val: &Operand, idx_val: &Operand) -> BlockId {
        let len = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, arr_val.clone(), ARR_LEN_OFF),
            Type::I64,
            None,
        );
        let ge_zero = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Sge, idx_val.clone(), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        let lt_len = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Slt, idx_val.clone(), Operand::Value(len)),
            Type::Bool,
            None,
        );
        let in_bounds = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(
                crate::ssa::BinOp::And,
                Operand::Value(ge_zero),
                Operand::Value(lt_len),
            ),
            Type::Bool,
            None,
        );
        let write_blk = self.f.add_block();
        let oob_blk = self.f.add_block();
        let join_blk = self.f.add_block();
        let cb = self.cur_block;
        self.f.set_term(
            cb,
            Terminator::CondBr {
                cond: Operand::Value(in_bounds),
                then_blk: write_blk,
                else_blk: oob_blk,
            },
        );
        self.cur_block = oob_blk;
        self.f.append_void(
            self.cur_block,
            InstKind::Call(self.intrinsics.arr_oob_write_reject, vec![idx_val.clone()]),
        );
        self.emit_throw_check(None);
        let ob = self.cur_block;
        self.f.set_term(ob, Terminator::Br(join_blk));
        self.cur_block = write_blk;
        join_blk
    }
}

impl<'a> LowerCtx<'a> {
    /// Dynamic string key on an `any` receiver — store through the
    /// key-parameterized member-set core with the runtime Str cell
    /// as the key (a Substr view materializes to an owned temp
    /// released after the call).
    fn lower_any_index_assign_str_key(
        &mut self,
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
        crate::ssa_lower_assign_member_any::emit_any_member_set_dyn(
            self, obj_val, key_v, -1, tag_op, value_op, &obj_ident,
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
}
