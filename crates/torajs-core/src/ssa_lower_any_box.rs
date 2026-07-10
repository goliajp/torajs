//! Any-box / (tag, value) family helpers for `LowerCtx<'a>` extracted
//! from `ssa_lower.rs` chunk 373.
//!
//! Six accessors that share the "SSA scalar ↔ AnyBox" boundary:
//! `box_to_any_from_expr` / `box_to_any` / `box_to_tag_value` /
//! `lower_to_tag_value` / `any_unbox_value_as_ptr` /
//! `emit_any_dynobj_writeback`. Method bodies are byte-for-byte
//! preserved from the source; siblings and `ssa_lower.rs` reach them
//! through the impl block on the shared `crate::ssa_lower::LowerCtx`
//! type.

use crate::ast::{Expr, ExprId};
use crate::short_str_encode::encode_short_str_literal;
use crate::ssa::{InstKind, Operand, Type, ValueId};
use crate::ssa_lower::LowerCtx;

impl<'a> LowerCtx<'a> {
    /// P1.5 — `box_to_any` variant that knows the source frontend
    /// type, so it can pick ANY_UNDEF=5 vs ANY_NULL=0 for the
    /// pointer-shaped cases. The tag is the only thing that
    /// distinguishes null from undefined at the runtime level
    /// (both lower to ConstPtrNull); the per-tag rules in
    /// any_typeof / any_to_str / any_to_bool / etc. then preserve
    /// the spec distinction downstream.
    pub(crate) fn box_to_any_from_expr(&mut self, eid: ExprId, val: Operand) -> Operand {
        let is_undef = matches!(
            self.expr_types.get(&eid),
            Some(crate::check::Type::Undefined)
        );
        let val_ty = self.operand_ty(&val);
        if is_undef && matches!(val_ty, Type::Ptr) {
            // ANY_UNDEF=5, payload 0.
            let v = self.f.append_inst(
                self.cur_block,
                InstKind::Call(
                    self.intrinsics.any_box,
                    vec![Operand::ConstI64(5), Operand::ConstI64(0)],
                ),
                Type::Any,
                None,
            );
            return Operand::Value(v);
        }
        // Step 8d — IR-side const ShortStr emit for compile-time short
        // string literals. When boxing a Type::Str whose source expression
        // is a string literal of ≤ SHORT_STR_CAP bytes, bypass the runtime
        // `any_box(4, str_ptr)` call: encode the bytes directly into a
        // NaN-box ShortStr u64 at compile time, then emit
        // IntToPtr(ConstI64(short_u64)) typed as Any. The dead StaticStrRef
        // inst left behind is dropped by LLVM DCE (no side effects);
        // STATIC_LITERAL strings carry a no-op rc_dec path so any leftover
        // scope-end drop is also a no-op at runtime.
        if matches!(val_ty, Type::Str)
            && let Expr::String(s) = self.ast.get_expr(eid)
            && let Some(short_u64) = encode_short_str_literal(s.as_bytes())
        {
            let v = self.f.append_inst(
                self.cur_block,
                InstKind::IntToPtr(Operand::ConstI64(short_u64 as i64)),
                Type::Any,
                None,
            );
            return Operand::Value(v);
        }
        // RFC 20260708-typed-arr-oob-read chunk 2 — a possibly-
        // sentinel F64 (number[] index read / alias) boxes to
        // ANY_UNDEF when the bits match, so the any world sees a
        // real `undefined` instead of a NaN with our payload.
        if val_ty == Type::F64 && crate::ssa_lower_nullable_guard::is_undef_f64_source(self, eid) {
            return self.box_f64_or_undef(val);
        }
        self.box_to_any(val)
    }

    /// Lower an expression to its `(tag, value)` pair, with the same
    /// frontend-type awareness as `box_to_any_from_expr`. Used by sites
    /// that need both the unboxed pair *and* the spec-correct ANY_UNDEF
    /// tag for an `undefined` literal (P6.1 Map.set / has / delete /
    /// get etc.) — plain `box_to_tag_value` would otherwise see only
    /// the SSA-level `Type::Ptr` + `ConstPtrNull` and emit ANY_NULL,
    /// collapsing undefined and null into the same key.
    pub(crate) fn lower_to_tag_value(&mut self, eid: ExprId) -> (Operand, Operand) {
        let (tag, val, _, _) = self.lower_to_tag_value_raw(eid);
        (tag, val)
    }

    /// [`Self::lower_to_tag_value`] variant that also hands back the
    /// raw lowered operand and its type, so consuming stores can
    /// settle an owned temp's surplus reference after the slot takes
    /// its +1 from `box_to_tag_value` (chunk 566 member-assign share).
    pub(crate) fn lower_to_tag_value_raw(
        &mut self,
        eid: ExprId,
    ) -> (Operand, Operand, Operand, Type) {
        let is_undef = matches!(
            self.expr_types.get(&eid),
            Some(crate::check::Type::Undefined)
        );
        let val = self.lower_expr(eid);
        let val_ty = self.operand_ty(&val);
        if is_undef && matches!(val_ty, Type::Ptr) {
            return (Operand::ConstI64(5), Operand::ConstI64(0), val, Type::Ptr);
        }
        // RFC 20260708-typed-arr-oob-read chunk 3 — a possibly-
        // sentinel F64 (number[] index read / alias) crossing into
        // the (tag, value) world (Map.set / Set.add / member-assign
        // family) resolves to ANY_UNDEF when the bits match, so
        // `m.get(k)` round-trips a real `undefined` instead of a
        // NaN with our payload.
        if val_ty == Type::F64 && crate::ssa_lower_nullable_guard::is_undef_f64_source(self, eid) {
            let (tag, v) = self.tag_value_f64_or_undef(val.clone());
            return (tag, v, val, val_ty);
        }
        let (tag, v) = self.box_to_tag_value(val.clone());
        (tag, v, val, val_ty)
    }

    /// Any-dynamic-access RFC (20260704) S1 — when a typed
    /// `Type::Arr(..)` value crosses into the `any` world, emit one
    /// `__torajs_arr_mark_kind(arr, chain)` call so the heap block
    /// becomes self-describing (the V8 ElementsKind shape). The
    /// chain packs 3 bits per nesting level, little-endian; the
    /// runtime helper recurses into nested `Tag::Arr` cells for
    /// `ARR_KIND_HEAP` levels with a remaining chain. No-op emit for
    /// non-array types and for `Arr<Any>` (FLAG_ARR_ANY blocks are
    /// NaN-box self-describing → chain 0).
    ///
    /// RFC 20260707 chunk 621 — the chain derives from the VALUE's
    /// own SSA type (the block's physical layout), never from the
    /// destination slot's static view: a typed array shared into an
    /// `Arr<Any>` slot (T-11 container widen) keeps its raw-slot
    /// layout, and the slot-typed chain (Any → 0) skipped the mark
    /// the kind-aware readers rely on.

    /// RFC 20260707 chunk 626 — call-arg admit station. When the
    /// callee's param slot is `Arr<Any>` and the arg's own SSA type
    /// is a typed array (T-11 container widen at the call boundary),
    /// mark the block's elem kind so the callee's kind-aware
    /// `Arr<Any>` readers can decode the raw layout. No-op when the
    /// arg is already `Arr<Any>` (chain 0), boxed `Any`, or not an
    /// array — `emit_arr_mark_kind` self-gates on the value's type.

    /// Chunk 641 — contextual empty-array-literal call arg. An empty
    /// `[]` has no element to infer from; when the callee's param is
    /// a typed `Arr(T)`, alloc the empty block with the PARAM's
    /// layout (mirror of `lower_let_init_val`'s V3-06 empty-literal
    /// annotation arm) instead of the default `Arr<Any>` — the
    /// checker's `empty_lit_into_arr` admit pairs with this so a
    /// FLAG_ARR_ANY block never lands behind a typed param slot
    /// (raw typed writes into NaN-box slots misdecode, chunk 614
    /// family). Returns None for non-empty / non-array-param shapes;
    /// the caller falls through to the plain `lower_expr`.
    pub(crate) fn try_lower_empty_array_arg(
        &mut self,
        arg: crate::ast::ExprId,
        expected: Option<&Type>,
    ) -> Option<Operand> {
        let Some(Type::Arr(aid)) = expected else {
            return None;
        };
        if !matches!(
            self.ast.get_expr(arg),
            crate::ast::Expr::Array(els) if els.is_empty()
        ) {
            return None;
        }
        let ty = Type::Arr(*aid);
        let alloc_fn = if self.arr_layouts[aid.0 as usize] == Type::Any {
            self.intrinsics.arr_alloc_any
        } else {
            self.intrinsics.arr_alloc
        };
        let v = self.f.append_inst(
            self.cur_block,
            InstKind::Call(alloc_fn, vec![Operand::ConstI64(0)]),
            ty,
            None,
        );
        Some(Operand::Value(v))
    }

    /// Kind values mirror `torajs_rc::ARR_KIND_*` (1=I64 raw, 2=F64
    /// raw, 3=Bool raw, 4=heap cell ptr; 0=UNSET/no-mark). Depth is
    /// capped at 21 levels (u64 / 3 bits) — deeper nests leave the
    /// tail UNSET, which consumers treat as the pre-RFC fallback.

    /// Extract `(tag_op, value_op)` for a freshly-lowered value, matching
    /// `box_to_any`'s tag scheme. Used by sites that need the unboxed
    /// pair instead of an Any-box (e.g. dynobj_set / fn_props_set
    /// which take tag + value as separate args). For statically-typed
    /// values the tag is `ConstI64(literal)`; for already-boxed Any
    /// it's a Load extracting the box's runtime tag at +8 — callers
    /// must pass the returned Operand straight through (don't unwrap
    /// to an i64 literal).
    pub(crate) fn box_to_tag_value(&mut self, val: Operand) -> (Operand, Operand) {
        let val_ty = self.operand_ty(&val);
        match val_ty {
            Type::I64 | Type::I32 => (Operand::ConstI64(2), val),
            Type::F64 => {
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
            // P4.0 — Type::Any must be unboxed BEFORE the
            // is_refcounted catch-all (Type::Any is itself
            // refcounted; would otherwise grab the any-box wrapper
            // ptr and tag=ANY_HEAP, dropping the real tag/value).
            // Step 7c: read via any_unbox_tag/_value shims (was
            // inline `Load i64 +8/+16` direct-offset). Chunk 610:
            // owned unbox fuses the old unbox_value +
            // any_payload_rc_inc pair — a heap cell still gets the
            // slot's +1 (inc moved inside the shim), while a
            // ShortStr's materialized rc=1 Str IS the slot's stake
            // (the separate inc double-counted it and leaked).
            Type::Any => {
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
            // RFC 20260707 chunk 3 — a Str slot carries three shapes
            // (NULL = JS null, the undefined sentinel cell, a heap
            // Str); the pair helpers decode at runtime. The value
            // half takes the stake (rc_inc inside, mirrors the
            // emit_rc_inc this arm used to emit).
            Type::Str => {
                let tag_v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.anyv_str_slot_tag, vec![val.clone()]),
                    Type::I64,
                    None,
                );
                let val_v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.anyv_str_slot_value, vec![val]),
                    Type::I64,
                    None,
                );
                (Operand::Value(tag_v), Operand::Value(val_v))
            }
            _ if val_ty.is_refcounted() => {
                self.emit_rc_inc(val.clone());
                // RFC 20260704 S1 — typed arr crossing into `any`
                // records its element kind on the heap header.
                self.emit_arr_mark_kind(&val);
                (Operand::ConstI64(4), val)
            }
            Type::Ptr if matches!(val, Operand::ConstPtrNull) => {
                (Operand::ConstI64(0), Operand::ConstI64(0))
            }
            other => panic!("ssa-lower: box_to_tag_value type {other:?} not supported"),
        }
    }

    pub(crate) fn box_to_any(&mut self, val: Operand) -> Operand {
        let val_ty = self.operand_ty(&val);
        let (tag, value_op): (i64, Operand) = match val_ty {
            Type::I64 | Type::I32 => (2, val),
            Type::F64 => {
                let bits = self.f.append_inst(
                    self.cur_block,
                    InstKind::BitCastF64ToI64(val),
                    Type::I64,
                    None,
                );
                (3, Operand::Value(bits))
            }
            Type::Bool => {
                let zext = self.f.append_inst(
                    self.cur_block,
                    InstKind::ZExtBoolToI64(val),
                    Type::I64,
                    None,
                );
                (1, Operand::Value(zext))
            }
            // RFC 20260707 chunk 3 — a Str slot decodes its three
            // shapes (NULL = null / sentinel = undefined / heap Str)
            // inside the box helper; heap rc_inc happens there too.
            Type::Str => {
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.anyv_box_str_slot, vec![val]),
                    Type::Any,
                    None,
                );
                return Operand::Value(v);
            }
            _ if val_ty.is_refcounted() => {
                // Heap-typed value: pass the ptr as i64
                // (ABI-compatible — ptr ↔ i64 share the machine
                // word). `anyv_box_from_pair` tag 4 is a pure NaN-box
                // ENCODING with zero rc traffic (chunk 753 — the
                // stale "helper bumps its refcount" claim here only
                // ever held for the Str-slot helper above): the
                // caller owns the stake story — either the source
                // keeps it (borrow'd box, no release) or transfers it
                // (moved binding / owned temp handed to an owning
                // consumer).
                // RFC 20260704 S1 — typed arr crossing into `any`
                // records its element kind on the heap header.
                self.emit_arr_mark_kind(&val);
                (4, val)
            }
            Type::Ptr => {
                // P3.2 — distinguish ConstPtrNull (the lowered `null`
                // literal) from a generic Ptr value (e.g. a dynobj
                // alloc result). Pre-P3.2 box_to_any treated all
                // Ptrs as ANY_NULL, which silently dropped dynobj
                // ptrs and made `let x: any = {}; x.foo` always
                // return undefined. Now ConstPtrNull → ANY_NULL=0;
                // any other Ptr → ANY_HEAP=4 with the ptr as value.
                if matches!(val, Operand::ConstPtrNull) {
                    (0, Operand::ConstI64(0))
                } else {
                    (4, val)
                }
            }
            other => panic!("ssa-lower: box_to_any element type {other:?} not supported"),
        };
        let v = self.f.append_inst(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.any_box,
                vec![Operand::ConstI64(tag), value_op],
            ),
            Type::Any,
            None,
        );
        Operand::Value(v)
    }

    /// Inverse of `box_to_any` for heap-payload reads — decode an
    /// AnyBox-encoded `Type::Any` operand back to its boxed-payload
    /// pointer. Emits the `any_unbox_value` shim call (returning the
    /// i64 value field) plus an IntToPtr cast, so callers stay
    /// decoupled from the AnyBox struct layout (Step 7d's NaN-box
    /// switch only has to swap the shim impl).
    ///
    /// OWNERSHIP: `any_unbox_value` MATERIALIZES a ShortStr into an
    /// owned heap Str — a caller that treats the result as a borrow
    /// leaks it (chunk 712's ~32B-per-member-read regression). Sites
    /// that only need "cell pointer or NULL" (tag probes, class
    /// dispatch) must use [`Self::any_cell_ptr_as_ptr`] instead.
    pub(crate) fn any_unbox_value_as_ptr(&mut self, obj: Operand) -> ValueId {
        let raw = self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.any_unbox_value, vec![obj]),
            Type::I64,
            None,
        );
        self.f.append_inst(
            self.cur_block,
            InstKind::IntToPtr(Operand::Value(raw)),
            Type::Ptr,
            None,
        )
    }

    /// Borrow-shaped variant (chunk 712) — a heap cell decodes to
    /// its pointer, every immediate (ShortStr included) to NULL.
    /// Nothing materializes, so the result carries no ownership.
    pub(crate) fn any_cell_ptr_as_ptr(&mut self, obj: Operand) -> ValueId {
        let raw = self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.any_cell_ptr, vec![obj]),
            Type::I64,
            None,
        );
        self.f.append_inst(
            self.cur_block,
            InstKind::IntToPtr(Operand::Value(raw)),
            Type::Ptr,
            None,
        )
    }

    /// Step 7d-A — `dynobj_set` / `dynobj_define` may resize +
    /// relocate the underlying heap block (`*obj_slot` updated).
    /// The variable's AnyValue still holds the OLD ptr; if the
    /// receiver was a named Ident, reload the post-resize ptr and
    /// store it back as a fresh NaN-box `AnyValue`. NaN-box Cell
    /// encoding is `ptr as u64` (identical bits — the PtrToInt +
    /// IntToPtr cast is a no-op at LLVM IR; LTO collapses them
    /// into the same SSA value). Non-Ident receivers (e.g.
    /// `arr[i].x = v`) don't have a hoisted slot; the resize-time
    /// dangling is a follow-up patch (no current conformance
    /// fixture exercises it under the 7/8 load factor +
    /// `INITIAL_CAP=8`).
    pub(crate) fn emit_any_dynobj_writeback(
        &mut self,
        obj_ident: &Option<String>,
        dynobj_slot: ValueId,
    ) {
        let Some(name) = obj_ident else {
            return;
        };
        let Some(info) = self.locals.get(name).copied() else {
            return;
        };
        if !matches!(info.ty, Type::Any) {
            return;
        }
        let new_dynobj = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::Ptr, Operand::Value(dynobj_slot), 0),
            Type::Ptr,
            None,
        );
        let new_dynobj_as_i64 = self.f.append_inst(
            self.cur_block,
            InstKind::PtrToInt(Operand::Value(new_dynobj)),
            Type::I64,
            None,
        );
        let new_any = self.f.append_inst(
            self.cur_block,
            InstKind::IntToPtr(Operand::Value(new_dynobj_as_i64)),
            Type::Any,
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::Value(new_any), Operand::Value(info.slot), 0),
        );
    }
}
