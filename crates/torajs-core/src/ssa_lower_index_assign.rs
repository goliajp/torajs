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

/// Where the (possibly realloc'd) array pointer can be stored back.
enum WriteBack {
    /// Ident bound to a mutable local — `info.slot` alloca.
    Local(crate::ssa::ValueId),
    /// Ident bound to a K.3 const-global array.
    Global(String),
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
            let idx_val = self.lower_index_operand(index);
            let v_raw = self.lower_expr(value);
            self.consume_if_ident(value);
            let v_ty = self.operand_ty(&v_raw);
            let (tag_op, value_op) = self.pack_any_slot_value(&v_raw, v_ty);
            let cur_block = self.cur_block;
            self.f.append_void(
                cur_block,
                InstKind::Call(
                    self.intrinsics.any_index_set,
                    vec![arr_val, idx_val, tag_op, value_op],
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
            if let Some(info) = self.locals.get(name).copied() {
                Some(WriteBack::Local(info.slot))
            } else if self.globals.contains_key(name) {
                Some(WriteBack::Global(name.clone()))
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
            self.consume_if_ident(value);
            let v_ty = self.operand_ty(&v_raw);
            let (tag_op, value_op) = self.pack_any_slot_value(&v_raw, v_ty);
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
        self.consume_if_ident(value);
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
        // T-13.5: head-aware byte offset for indexed assign.
        let offset = self.emit_arr_slot_byte_offset(arr_val.clone(), idx_val, 3, is_non_deque);
        // Drop old elem if non-Copy. M1.2 MVP only ships i64
        // elements (Copy), so this branch currently never fires; lays
        // groundwork for non-Copy element types in a follow-up.
        if !elem_ty.is_copy() {
            let old = self.f.append_inst(
                self.cur_block,
                InstKind::LoadDyn(elem_ty, arr_val.clone(), offset.clone()),
                elem_ty,
                None,
            );
            self.emit_drop_value(Operand::Value(old), elem_ty);
        }
        self.f.append_void(
            self.cur_block,
            InstKind::StoreDyn(v.clone(), arr_val, offset),
        );
        let wb = self.cur_block;
        self.f.set_term(wb, Terminator::Br(join_blk));
        self.cur_block = join_blk;
        v
    }

    /// Pack the value into a (tag, value) operand pair using the same
    /// scheme as box_to_any but without the heap allocation.
    fn pack_any_slot_value(&mut self, v_raw: &Operand, v_ty: Type) -> (Operand, Operand) {
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
                // NaN-box decoders (Step 7e-A).
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
                match wb {
                    WriteBack::Local(slot) => {
                        self.f.append_void(
                            self.cur_block,
                            InstKind::Store(Operand::Value(new_arr), Operand::Value(slot), 0),
                        );
                    }
                    WriteBack::Global(name) => {
                        let gref = self.f.append_inst(
                            self.cur_block,
                            InstKind::GlobalRef(name),
                            Type::Ptr,
                            None,
                        );
                        self.f.append_void(
                            self.cur_block,
                            InstKind::Store(Operand::Value(new_arr), Operand::Value(gref), 0),
                        );
                    }
                }
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
