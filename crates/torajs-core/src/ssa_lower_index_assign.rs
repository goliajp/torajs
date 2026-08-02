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
//!    branch; OOB calls `__torajs_arr_typed_set_grow` (RFC
//!    20260721-typed-grow-on-write — grow-as-holes + store, closing
//!    the bug-327 C3 loud-reject placeholder; negative and
//!    beyond-dense-limit indexes stay catchable RangeErrors).

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
        // Chunk 745 — struct receiver + compile-time literal index:
        // `g[0] = v` ≡ `g."0" = v` per ES ToPropertyKey (§7.1.19);
        // the member-assignment lane handles the field store (struct
        // layout / setter / rc discipline). Same gate as the checker
        // lane in `check_assign_target::check_index`.
        if matches!(
            self.expr_types.get(&obj),
            Some(crate::check::Type::Struct(_))
        ) && let Some(name) = crate::ast::literal_prop_key(self.ast, index)
        {
            return crate::ssa_lower_assign_member::lower(self, obj, name, value);
        }
        // M1.4 — `arr[i] = value`. 11-A1: peek receiver before
        // consuming `obj` for the head-elision flag.
        let is_non_deque = self.arr_expr_is_non_deque(obj);
        let arr_val = self.lower_expr(obj);
        let arr_ty = self.operand_ty(&arr_val);
        // RFC 20260802 刀 3 后半 — a STRUCT receiver's keyed WRITE
        // rides the any lane (the 刀 3a read box's write mirror): the
        // box is a pure tag-4 encode, and member_set's struct arm
        // dispatches layout field / accessor / expando (blade 2).
        // The computed-field ctor prefix (`(this as any)[key] = v`)
        // lands here too — the As is a bare pass-through for a heap
        // source, so the operand still reads Type::Obj.
        let (arr_val, arr_ty) = if matches!(arr_ty, Type::Obj(_)) {
            (self.box_to_any(arr_val), Type::Any)
        } else {
            (arr_val, arr_ty)
        };
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
            // §6.1.7 / §7.1.19 step 2 — a symbol key reaches the set
            // core as its own cell, uncoerced.
            if matches!(
                self.expr_types.get(&index),
                Some(crate::check::Type::Symbol)
            ) {
                return self.lower_any_index_assign_symbol_key(obj, arr_val, index, value);
            }
            // Cluster #1 blade 4 — an `any`-typed key rides the keyed
            // set kernel's runtime ToPropertyKey dispatch.
            if matches!(self.expr_types.get(&index), Some(crate::check::Type::Any)) {
                return self.lower_any_index_assign_any_key(obj, arr_val, index, value);
            }
            let idx_val = self.lower_index_operand(index);
            let v_raw = self.lower_expr(value);
            // Chunk 567 — SHARE, no consume.
            let v_ty = self.operand_ty(&v_raw);
            let (tag_op, value_op) = self.pack_any_slot_value_shared(value, &v_raw, v_ty);
            let recv_slot = self.resolve_any_recv_slot(obj);
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
        self.emit_arr_mark_kind(&v);
        // Chunk 567 — a typed-tier elem store SHARES the rhs: a
        // borrow-shape value takes +1 so the slot owns its stake
        // while the source binding keeps its own (re-assign
        // drop-old no longer steals it — UAF, probe-proven); owned
        // temps keep transferring their fresh reference.
        let transfers = self.expr_transfers_ownership(value);
        let (v, transfers) = self.coerce_elem_store(elem_ty, value, v, transfers);
        let join_blk = self.emit_index_bounds_guard(&arr_val, &idx_val, &v, elem_ty, transfers);
        // The slot's +1 lands inside the in-bounds write block — the
        // OOB path stores nothing and must not mint a stake.
        if !transfers && !elem_ty.is_copy() {
            self.emit_rc_inc(v.clone());
        }
        self.emit_index_integrity_guard(&arr_val, &idx_val);
        // T-13.5: head-aware byte offset for indexed assign.
        let (offset_base, offset) =
            self.emit_arr_slot_byte_offset(arr_val.clone(), idx_val.clone(), 3, is_non_deque);
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
        self.emit_hole_revive_branch(&arr_val, &idx_val, join_blk);
        v
    }

    /// Chunk B (RFC 20260721-typed-grow-on-write) — an in-bounds
    /// store into a HOLE index revives it as a default data
    /// property (§10.1.5.1). One header-word test keeps the
    /// plain-array hot path call-free: FLAG_ARR_EXOTIC_INDEX is
    /// bit 15 of the u16 flags at byte 6, i.e. bit 63 of the LE
    /// header word — the load shares the len load's cache line.
    /// Terminates the current (write) block and leaves `cur_block`
    /// on `join_blk`.
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
    /// the negative-index shape gets its own arm). The OOB block
    /// grows-as-holes through `__torajs_arr_typed_set_grow` (RFC
    /// 20260721-typed-grow-on-write — the kernel owns the negative /
    /// dense-limit RangeErrors and stores `v` at `idx` otherwise);
    /// leaves `cur_block` on the in-bounds write block and returns
    /// the join block both paths branch to. `v` is the W4-coerced
    /// value — the grow arm mints its own slot stake (mirror of the
    /// in-bounds arm's) and hands the kernel the raw 8-byte form.
    fn emit_index_bounds_guard(
        &mut self,
        arr_val: &Operand,
        idx_val: &Operand,
        v: &Operand,
        elem_ty: Type,
        transfers: bool,
    ) -> BlockId {
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
        // The grow arm stores for real — it mints the slot's +1 the
        // same way the in-bounds arm does (the kernel's reject arms
        // drop the transferred stake so it never leaks).
        if !transfers && !elem_ty.is_copy() {
            self.emit_rc_inc(v.clone());
        }
        let raw_v = match elem_ty {
            Type::Bool => {
                let z = self.f.append_inst(
                    self.cur_block,
                    InstKind::ZExtBoolToI64(v.clone()),
                    Type::I64,
                    None,
                );
                Operand::Value(z)
            }
            _ => self.raw_slot_arg(v.clone()),
        };
        self.f.append_void(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.arr_typed_set_grow,
                vec![arr_val.clone(), idx_val.clone(), raw_v],
            ),
        );
        self.emit_throw_check(None);
        let ob = self.cur_block;
        self.f.set_term(ob, Terminator::Br(join_blk));
        self.cur_block = write_blk;
        join_blk
    }

    /// W4 — align the stored value with the element's width, and
    /// answer whether the slot now takes the value's own reference.
    ///
    /// The reverse width direction (an f64 value into an i64 element)
    /// means the container width analysis missed a write site, and is
    /// loud rather than bit-punned.
    ///
    /// An `any` rhs is the same crossing the binding and the
    /// assignment boundaries decode (`ssa_lower_stmt_let_decl`'s
    /// scalar row, `ssa_lower_assign_ident`'s coercion table). Without
    /// it the NaN-box bits land in the slot and read back as the
    /// element: `a[0] = v` with `v: any` holding 3 answered NaN, and a
    /// member-shaped source answered the raw box.
    fn coerce_elem_store(
        &mut self,
        elem_ty: Type,
        value: ExprId,
        v: Operand,
        transfers: bool,
    ) -> (Operand, bool) {
        match (elem_ty, self.operand_ty(&v)) {
            (Type::F64, Type::I64) => (self.coerce_to_f64(v), transfers),
            (Type::I64, Type::F64) => panic!(
                "ssa-lower: f64 value into i64 array elem — \
                 container width analysis missed this write"
            ),
            (Type::I64 | Type::F64, Type::Any) => {
                // ToNumber only READS the box, and the decoded slot is
                // Copy, so the source's own stake needs settling here.
                let n = self.coerce_any_to_number(v.clone(), elem_ty);
                self.release_owned_temp(value, &v);
                (n, true)
            }
            (Type::Str, Type::Any) => {
                // ToString mints a fresh owned Str — the slot takes
                // exactly that reference, so this is a transfer.
                let s = self.coerce_to_str(v.clone(), Type::Any);
                self.release_owned_temp(value, &v);
                (s, true)
            }
            _ => (v, transfers),
        }
    }
}
