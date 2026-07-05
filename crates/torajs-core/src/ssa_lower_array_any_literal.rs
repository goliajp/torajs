//! Array<Any> literal lowering helper for `LowerCtx<'a>` extracted
//! from `ssa_lower.rs` chunk 391 — Path A.3-batch12.
//!
//! Single method:
//!
//! - `lower_array_any_literal(ids)` — T-10.c (v0.4.0) codegen for a
//!   heterogeneous Array literal. `arr_alloc_any(N)` sized to fit the
//!   non-spread literal count, then per-element box + `arr_push_any`
//!   with the matching Any tag (ANY_NULL=0 / ANY_BOOL=1 via zext /
//!   ANY_I64=2 / ANY_F64=3 via bitcast / ANY_HEAP=4 with `rc_inc` /
//!   ANY_UNDEF=5 for `undefined` literal). Spread items route through
//!   `arr_extend_any` and `Type::Set` spread lifts through the shared
//!   `Array.from(set)` helper (S141). Nested Array literals recurse
//!   through this same fn so inner arrays are also `Arr<Any>` per-slot
//!   NaN-box (see comment about `[[1,2],[3,4]]` SIGSEGV root cause).
//!
//! Method body is byte-for-byte preserved from the source; the sibling
//! reaches LowerCtx fields via `impl<'a> super::LowerCtx<'a>`, so call
//! sites (recursive nested arrays, plus external callers in the
//! ObjectLit/LetDecl/etc. paths) need zero edits.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{LowerCtx, intern_arr_layout};

impl<'a> LowerCtx<'a> {
    /// T-10.c (v0.4.0) — emit codegen for a heterogeneous Array
    /// literal. alloc_any(N) sized to fit, then per-element box +
    /// push_any with the matching tag. Returns the (possibly grown)
    /// array pointer as Operand::Value.
    pub(crate) fn lower_array_any_literal(&mut self, ids: &[ExprId]) -> Operand {
        // P5.6 — spreads inside Array<Any> literals walk through
        // arr_extend_any (which understands the 16-byte tagged
        // slot layout); non-spread items still push tag/value
        // pairs via arr_push_any. The arr_alloc_any size hint is
        // the literal-count (spreads grow via realloc — same
        // strategy as push's growth on overflow).
        let arr_id = intern_arr_layout(self.arr_layouts, Type::Any);
        let literal_count: i64 = ids
            .iter()
            .filter(|id| !matches!(self.ast.get_expr(**id), Expr::Spread { .. }))
            .count() as i64;
        let mut arr = self.f.append_inst(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.arr_alloc_any,
                vec![Operand::ConstI64(literal_count)],
            ),
            Type::Arr(arr_id),
            None,
        );
        for &eid in ids {
            // P5.6 — spread item routes through arr_extend_any.
            // Inner must lower to Type::Arr(any_arr_id); typed
            // Array<T> spread into Array<Any> needs per-elem box
            // (defer; reject with subset-boundary msg).
            if let Expr::Spread { expr: inner } = self.ast.get_expr(eid) {
                let inner_eid = *inner;
                let mut src_op = self.lower_expr(inner_eid);
                let mut src_ty = self.operand_ty(&src_op);
                // S141 — `[...set]` inside Array<Any> literal: route
                // through the shared Array.from(set) helper to land an
                // Arr<Any> the existing arr_extend_any path can splice.
                if matches!(src_ty, Type::Set) {
                    src_op = crate::ssa_lower_arr_from_set::emit(self, src_op);
                    src_ty = self.operand_ty(&src_op);
                }
                // RFC 20260704 S5+ — `[...anyval]`: materialize the
                // runtime-dispatched iterable through the unified
                // iteration protocol. The result is an owned temp —
                // extend copies + rc_incs the slots, so drop after.
                let mut src_is_owned_temp = false;
                if matches!(src_ty, Type::Any) {
                    src_op = crate::ssa_lower_arr_from_any::emit(self, src_op);
                    src_ty = self.operand_ty(&src_op);
                    src_is_owned_temp = true;
                }
                let inner_is_any_arr = match src_ty {
                    Type::Arr(src_arr_id) => {
                        matches!(self.arr_layouts[src_arr_id.0 as usize], Type::Any)
                    }
                    _ => false,
                };
                if !inner_is_any_arr {
                    panic!(
                        "ssa-lower: spread of {src_ty:?} into Array<Any> literal not yet supported (P5.6 subset — Array<Any> spread only; typed-Array spread into Any requires per-elem box, follow-up)"
                    );
                }
                let new_arr = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(
                        self.intrinsics.arr_extend_any,
                        vec![Operand::Value(arr), src_op.clone()],
                    ),
                    Type::Arr(arr_id),
                    None,
                );
                if src_is_owned_temp {
                    self.emit_drop_value(src_op, src_ty);
                }
                arr = new_arr;
                continue;
            }
            // Nested Array literal: recurse so the inner array is also
            // Arr<Any> (per-slot NaN-box). Without this, lower_expr routes
            // through the typed Arr<T> fast path and the outer slot's
            // ANY_HEAP unwrap exposes raw 8-byte int slots that
            // __torajs_arr_print_any decodes as NaN-box AnyValues → deref
            // `1` SIGSEGV on `[[1,2],[3,4]]`. Same root as the LetDecl
            // `let x: any = [...]` arm above.
            if let Expr::Array(inner_ids) = self.ast.get_expr(eid) {
                let inner_eids: Vec<ExprId> = inner_ids.clone();
                let inner_arr = self.lower_array_any_literal(&inner_eids);
                self.emit_rc_inc(inner_arr.clone());
                arr = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(
                        self.intrinsics.arr_push_any,
                        vec![Operand::Value(arr), Operand::ConstI64(4), inner_arr],
                    ),
                    Type::Arr(arr_id),
                    None,
                );
                continue;
            }
            let val = self.lower_expr(eid);
            let val_ty = self.operand_ty(&val);
            let (tag_op, value_op) = self.pack_any_elem(val, val_ty, Some(eid));
            arr = self.f.append_inst(
                self.cur_block,
                InstKind::Call(
                    self.intrinsics.arr_push_any,
                    vec![Operand::Value(arr), tag_op, value_op],
                ),
                Type::Arr(arr_id),
                None,
            );
        }
        Operand::Value(arr)
    }

    /// Pack an already-lowered element into the `(tag, value)` pair
    /// `arr_push_any` consumes. ANY_NULL=0, ANY_BOOL=1, ANY_I64=2,
    /// ANY_F64=3, ANY_HEAP=4, ANY_UNDEF=5. Refcounted values rc_inc
    /// (the slot takes an owning ref, the source keeps its own);
    /// `Type::Any` values unbox to their runtime pair +
    /// `any_payload_rc_inc` (the box stays a borrow of its owner).
    /// `eid` recovers the `undefined`-vs-`null` AST shape for
    /// ConstPtrNull operands (S127-1); `None` tags plain null.
    pub(crate) fn pack_any_elem(
        &mut self,
        val: Operand,
        val_ty: Type,
        eid: Option<ExprId>,
    ) -> (Operand, Operand) {
        match val_ty {
            Type::I64 | Type::I32 => (Operand::ConstI64(2), val),
            Type::F64 => {
                // T-10.d.ii — pun f64 bits to i64 so push_any
                // (i64 third param) carries them exactly.
                // print_any reverses the bitcast at decode time.
                let bits = self.f.append_inst(
                    self.cur_block,
                    InstKind::BitCastF64ToI64(val),
                    Type::I64,
                    None,
                );
                (Operand::ConstI64(3), Operand::Value(bits))
            }
            Type::Bool => {
                let zext = self.f.append_inst(
                    self.cur_block,
                    InstKind::ZExtBoolToI64(val),
                    Type::I64,
                    None,
                );
                (Operand::ConstI64(1), Operand::Value(zext))
            }
            Type::Any => {
                // RFC 20260704 S5+ — an `any` element carries its own
                // runtime tag: unbox the pair and rc-bump the heap
                // payload (the box is a borrow of its owner; the slot
                // needs an independent ref).
                let tag_v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.any_unbox_tag, vec![val.clone()]),
                    Type::I64,
                    None,
                );
                let val_v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.any_unbox_value, vec![val]),
                    Type::I64,
                    None,
                );
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(
                        self.intrinsics.any_payload_rc_inc,
                        vec![Operand::Value(tag_v), Operand::Value(val_v)],
                    ),
                );
                (Operand::Value(tag_v), Operand::Value(val_v))
            }
            _ if val_ty.is_refcounted() => {
                // Heap-typed value: rc_inc to hold an owning ref
                // for the array slot. push_any's third param is
                // i64 in the SSA decl; LLVM treats ptr ↔ i64 as
                // ABI-compatible (same machine word), so passing
                // the ptr operand directly works at the call site
                // without an explicit PtrToInt SSA op (which the
                // current InstKind enum doesn't expose). Drop
                // walks via __torajs_arr_drop_any when the array
                // dies.
                //
                // RFC 20260704 S1 — a typed Array entering an
                // Array<Any> slot crosses into `any` just like
                // box_to_any: record its elem kind so the runtime
                // readers (index / print / drop walkers) don't
                // NaN-box-walk raw scalar slots. No-op for non-Arr.
                self.emit_arr_mark_kind(&val, &val_ty);
                self.emit_rc_inc(val.clone());
                (Operand::ConstI64(4), val)
            }
            Type::Ptr => {
                // Ptr that's null (Type::Null lowers to ConstPtrNull
                // → Type::Ptr). Tag as ANY_NULL with value 0.
                // S127-1: `undefined` literal also lowers to
                // ConstPtrNull (Type::Ptr). Recover the original
                // AST shape so the slot tags ANY_UNDEF=5, else
                // `[undefined]` collapses to `[null]` and
                // strict-eq / .indexOf(undefined) mis-fires.
                // Same root as W-D narrow trunk's box_to_any
                // ConstPtrNull arm (S126-1/-3).
                if matches!(
                    eid.map(|e| self.ast.get_expr(e)),
                    Some(Expr::Ident(n)) if n == "undefined"
                ) {
                    (Operand::ConstI64(5), Operand::ConstI64(0))
                } else {
                    (Operand::ConstI64(0), Operand::ConstI64(0))
                }
            }
            other => panic!(
                "not yet supported: lower_array_any_literal element type {other:?} \
                 (T-10.d will add F64 + boxed-primitive coverage)"
            ),
        }
    }
}
