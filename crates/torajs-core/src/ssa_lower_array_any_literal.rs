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
use crate::ssa::{BinOp as SsaBinOp, InstKind, Operand, Type};
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
                    let any_src = src_op.clone();
                    src_op = crate::ssa_lower_arr_from_any::emit(self, any_src.clone());
                    // The owned any source itself (Call-shaped product)
                    // is only borrowed by the materialization — release
                    // it here; the drop below covers the materialized
                    // array, not this source (same account as the
                    // primary spread lane).
                    self.release_owned_temp(inner_eid, &any_src);
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
                        vec![Operand::Value(arr), Operand::ConstI64(4), inner_arr.clone()],
                    ),
                    Type::Arr(arr_id),
                    None,
                );
                // Chunk 727 — the inner literal is a fresh block
                // (rc=1) and the slot took its own +1 above; drop
                // the stranded original stake.
                self.release_owned_temp(eid, &inner_arr);
                continue;
            }
            // §13.2.4 elision — the slot pushes as undefined and
            // immediately marks itself a HOLE (not an own property:
            // `1 in [0,,2]` false, indexOf skips). The runtime
            // helper reads len-1, which IS this slot since pushes
            // are sequential.
            if matches!(self.ast.get_expr(eid), Expr::Elision) {
                arr = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(
                        self.intrinsics.arr_push_any,
                        vec![
                            Operand::Value(arr),
                            Operand::ConstI64(5),
                            Operand::ConstI64(0),
                        ],
                    ),
                    Type::Arr(arr_id),
                    None,
                );
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(
                        self.intrinsics.arr_mark_last_hole,
                        vec![Operand::Value(arr)],
                    ),
                );
                continue;
            }
            let val = self.lower_expr(eid);
            let val_ty = self.operand_ty(&val);
            let (tag_op, value_op) = self.pack_any_elem(val.clone(), val_ty, Some(eid));
            arr = self.f.append_inst(
                self.cur_block,
                InstKind::Call(
                    self.intrinsics.arr_push_any,
                    vec![Operand::Value(arr), tag_op, value_op],
                ),
                Type::Arr(arr_id),
                None,
            );
            // Chunk 727 — pack_any_elem gives the slot its own +1
            // (Str slot-value / refcounted catch-all), so a fresh
            // owned element's original stake strands: `let a: any =
            // ["y" + i]` leaked one cell per fresh element (probe
            // q726fc 15.4MB / r727d two-elem 24.5MB vs 6.2MB flat;
            // typed-lane twin fixed in chunk 547).
            //
            // Rotation 460 — the Any arm takes the same +1: its
            // `any_unbox_value_owned` FUSES the unbox with a payload
            // rc_inc (chunk 610's own words), so a BORROWED any local
            // is right to keep its scope drop and an OWNED any temp
            // is left holding a stake nobody releases. The chunk-610
            // carve-out read the fused inc as a transfer and excluded
            // the whole type; `release_owned_temp` already answers
            // "borrow" for an Ident, so the narrow half of it was
            // never load-bearing. `function mk(n): any { return "s" +
            // n } … let a: any[] = [mk(i)]` leaked one cell per
            // iteration (13.2MB vs 6.8MB flat over 200k) while the
            // `: string`-returning twin stayed flat.
            self.release_owned_temp(eid, &val);
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
    /// `(tag, value)` of a Str-shaped slot through its pair kernels —
    /// the owned-Str and the Substr pair share the call shape; only the
    /// kernels differ (the Substr value half materializes an inline
    /// view — rotation 468).
    fn slot_pair(
        &mut self,
        tag_fid: crate::ssa::FuncId,
        val_fid: crate::ssa::FuncId,
        val: Operand,
    ) -> (Operand, Operand) {
        let tag_v = self.f.append_inst(
            self.cur_block,
            InstKind::Call(tag_fid, vec![val.clone()]),
            Type::I64,
            None,
        );
        let val_v = self.f.append_inst(
            self.cur_block,
            InstKind::Call(val_fid, vec![val]),
            Type::I64,
            None,
        );
        (Operand::Value(tag_v), Operand::Value(val_v))
    }

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
                // RFC 20260708-typed-arr-oob-read chunk 2 — a
                // possibly-sentinel F64 elem packs as ANY_UNDEF
                // when the bits match. Branch-free select:
                // z = (bits == SENTINEL); tag = 3 + 2z (3→5);
                // value = bits & (z - 1) (bits→0 on the sentinel).
                if let Some(e) = eid
                    && crate::ssa_lower_nullable_guard::is_undef_f64_source(self, e)
                {
                    let is_undef = self.f.append_inst(
                        self.cur_block,
                        InstKind::ICmp(
                            crate::ssa::IPred::Eq,
                            Operand::Value(bits),
                            Operand::ConstI64(
                                crate::ssa_lower_nullable_guard::F64_UNDEF_SENTINEL_BITS as i64,
                            ),
                        ),
                        Type::Bool,
                        None,
                    );
                    let z = self.f.append_inst(
                        self.cur_block,
                        InstKind::ZExtBoolToI64(Operand::Value(is_undef)),
                        Type::I64,
                        None,
                    );
                    let two_z = self.f.append_inst(
                        self.cur_block,
                        InstKind::BinOp(SsaBinOp::Add, Operand::Value(z), Operand::Value(z)),
                        Type::I64,
                        None,
                    );
                    let tag = self.f.append_inst(
                        self.cur_block,
                        InstKind::BinOp(SsaBinOp::Add, Operand::ConstI64(3), Operand::Value(two_z)),
                        Type::I64,
                        None,
                    );
                    let mask = self.f.append_inst(
                        self.cur_block,
                        InstKind::BinOp(SsaBinOp::Sub, Operand::Value(z), Operand::ConstI64(1)),
                        Type::I64,
                        None,
                    );
                    let value = self.f.append_inst(
                        self.cur_block,
                        InstKind::BinOp(SsaBinOp::And, Operand::Value(bits), Operand::Value(mask)),
                        Type::I64,
                        None,
                    );
                    return (Operand::Value(tag), Operand::Value(value));
                }
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
                // runtime tag: unbox the pair with the slot's own
                // stake (chunk 610 — owned unbox fuses unbox_value +
                // payload_rc_inc; a ShortStr's materialized rc=1 Str
                // IS the slot's ref, the separate inc double-counted
                // it and leaked).
                let tag_v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.any_unbox_tag, vec![val.clone()]),
                    Type::I64,
                    None,
                );
                let val_v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.any_unbox_value_owned, vec![val]),
                    Type::I64,
                    None,
                );
                (Operand::Value(tag_v), Operand::Value(val_v))
            }
            // RFC 20260707 chunk 3 — a Str slot decodes its three
            // shapes at runtime (NULL = null / undefined sentinel /
            // heap Str); the value half takes the slot's +1 (heap
            // case only), replacing the catch-all's emit_rc_inc.
            Type::Str => {
                let (t, v) = (
                    self.intrinsics.anyv_str_slot_tag,
                    self.intrinsics.anyv_str_slot_value,
                );
                self.slot_pair(t, v, val)
            }
            // A substring view element (`[a[0], …]` over a split
            // product) goes through the Substr pair helpers: the
            // sentinel decodes to undefined, and an inline view is
            // materialized — the any slot owns a copy rather than a
            // pointer into a block that can die first (rotation 468).
            Type::Substr => {
                let (t, v) = (
                    self.intrinsics.anyv_substr_slot_tag,
                    self.intrinsics.anyv_substr_slot_value,
                );
                self.slot_pair(t, v, val)
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
                self.emit_arr_mark_kind(&val);
                self.emit_rc_inc(val.clone());
                // RFC 20260722 chunk B — a possibly-undef-cell heap
                // elem (Nullable field read / find miss, `[u]` into
                // an any[] slot) packs ANY_UNDEF via the runtime tag
                // cmp instead of a raw tag-4 pointing at the oddball
                // header (which printed [object]). The inc above
                // no-ops on the static cell.
                if val_ty.spells_undef_with_generic_cell()
                    && let Some(e) = eid
                    && crate::ssa_lower_nullable_guard::is_undefable_heap_source(self, e)
                {
                    return self.heap_slot_tag_value(val);
                }
                (Operand::ConstI64(4), val)
            }
            Type::Ptr => {
                // Ptr that's null (Type::Null lowers to ConstPtrNull
                // → Type::Ptr). Tag as ANY_NULL with value 0.
                // S127-1: `undefined` literal also lowers to
                // ConstPtrNull (Type::Ptr). Recover the original AST
                // shape OR the checker's Undefined typing (chunk 807
                // — the checker probe alone regressed: expr_types
                // only holds walked expressions, so a bare
                // `undefined` ident in an un-walked position
                // collapsed to `[null]` and .indexOf(undefined)
                // mis-fired; the AST probe alone missed derived
                // shapes like a void-call element). Same root as
                // W-D narrow trunk's box_to_any ConstPtrNull arm
                // (S126-1/-3).
                let is_undef = matches!(
                    eid.map(|e| self.ast.get_expr(e)),
                    Some(Expr::Ident(n)) if n == "undefined"
                ) || matches!(
                    eid.and_then(|e| self.expr_types.get(&e)),
                    Some(crate::check::Type::Undefined)
                );
                if is_undef {
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
