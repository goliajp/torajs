//! Any-box / (tag, value) family helpers for `LowerCtx<'a>` extracted
//! from `ssa_lower.rs` chunk 373.
//!
//! Accessors that share the "SSA scalar ↔ AnyBox" boundary:
//! `box_to_any_from_expr` / `box_to_any` / `box_to_tag_value` /
//! `lower_to_tag_value` / `any_unbox_value_as_ptr`. Method bodies are
//! byte-for-byte preserved from the source; siblings and
//! `ssa_lower.rs` reach them through the impl block on the shared
//! `crate::ssa_lower::LowerCtx` type. `emit_any_dynobj_writeback`
//! lives in the `ssa_lower_any_box_writeback` sibling (rotation 204
//! file-size split).

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
    #[track_caller]
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
        // RFC 20260710 C2b + 20260722 chunk B — an undefable heap
        // source (Nullable optional-field read, or a heap-elem
        // find/findLast miss) may carry the generic undefined cell;
        // re-encode it as ANY_UNDEF so the any world never sees the
        // oddball. Zero branches: tag = 4 + (val == sentinel), and
        // `anyv_box_from_pair` tag 5 ignores the payload. Plain heap
        // sources keep the zero-cmp encoding below.
        if crate::ssa_lower_nullable_guard::is_undefable_heap_source(self, eid)
            && val_ty.spells_undef_with_generic_cell()
        {
            // Ownership mirrors box_to_any's refcounted arm (chunk
            // 753 — pure encoding, the caller owns the stake story);
            // the typed-arr kind mark self-gates on the sentinel's
            // static flag at runtime.
            self.emit_arr_mark_kind(&val);
            let (tag, value) = self.heap_slot_tag_value(val);
            let v = self.f.append_inst(
                self.cur_block,
                InstKind::Call(self.intrinsics.any_box, vec![tag, value]),
                Type::Any,
                None,
            );
            return Operand::Value(v);
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
        // RFC 20260710 C2b + 20260722 chunk B — an undefable heap
        // source (Nullable field read / heap-elem find miss) may
        // carry the generic undefined cell: encode ANY_UNDEF
        // instead of a heap tag pointing at the oddball. Ownership
        // mirrors box_to_tag_value's refcounted arm (slot takes its
        // +1); the inc and the kind mark both no-op on the sentinel
        // through the runtime static gates.
        if crate::ssa_lower_nullable_guard::is_undefable_heap_source(self, eid)
            && val_ty.spells_undef_with_generic_cell()
        {
            self.emit_rc_inc(val.clone());
            self.emit_arr_mark_kind(&val);
            let (tag, v) = self.heap_slot_tag_value(val.clone());
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
            // Rotation 185 — the (tag, value) twin of box_to_any's
            // Substr arm: a Substr slot may hold the Substr-shaped
            // undefined sentinel (string index OOB read), which the
            // refcounted catch-all would encode as a tag-4 heap cell
            // (Map.set / member-assign consumers then misread it as
            // a string). The pair helpers decode at runtime; the
            // value half takes the slot's +1 for a heap view.
            Type::Substr => {
                let tag_v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.anyv_substr_slot_tag, vec![val.clone()]),
                    Type::I64,
                    None,
                );
                let val_v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.anyv_substr_slot_value, vec![val]),
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

    #[track_caller]
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
            // inside the box helper. RC-NEUTRAL like every other arm
            // (rotation 184 — the "heap rc_inc happens there too"
            // claim that used to sit here was stale and seeded two
            // use-after-free sites): anyv_box_str_slot is a pure
            // encode; the caller owns the stake story.
            Type::Str => {
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.anyv_box_str_slot, vec![val]),
                    Type::Any,
                    None,
                );
                return Operand::Value(v);
            }
            // Rotation 185 — a Substr slot may hold the Substr-shaped
            // undefined sentinel (string index OOB read, optindex
            // route included); the box helper decodes it to a real
            // ANY_UNDEF so the any world's typeof / strict-eq / print
            // all agree. RC-NEUTRAL like every box-family kernel —
            // the caller owns the stake story. Heap views stay a pure
            // tag-4 encode (any-world readers dispatch on the header's
            // Tag::Str + FLAG_SUBSTR_VIEW).
            Type::Substr => {
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.anyv_box_substr_slot, vec![val]),
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
            // A call that produces no value produces `undefined`
            // (§14.10) — ANY_UNDEF=5, payload 0. `check_stmt_let_decl`
            // (chunk 618) already states this and normalizes Void to
            // Undefined, but only for a `let` init; every other store
            // site reached the panic below instead. A generator's
            // let-lift is one such site — it rewrites `let a = f()`
            // into `this.a = f()`, an assignment, so a void call
            // landing in an any-typed field crashed the lowerer.
            //
            // Nothing that works today can regress through this arm:
            // what it replaces is a panic.
            Type::Void => (5, Operand::ConstI64(0)),
            other => panic!(
                "ssa-lower: box_to_any element type {other:?} not supported (from {})",
                core::panic::Location::caller()
            ),
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
}
