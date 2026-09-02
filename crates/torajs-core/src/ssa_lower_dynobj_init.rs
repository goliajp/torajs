//! Dynobj init helper for `LowerCtx<'a>` extracted from
//! `ssa_lower.rs` chunk 386 — Path A.3-batch7.
//!
//! Single method:
//!
//! - `lower_dynobj_init(eid)` — P3.2 lowering for
//!   `let x: any = { f1: v1, f2: v2 }`. Allocates a dynobj via
//!   `__torajs_dynobj_alloc()`, per-field boxes the value with the
//!   `Any`-box tag scheme (I64/I32=2, F64=3 via bitcast, Bool=1 via
//!   zext, ANY_HEAP=4 for refcounted types), then calls `dynobj_set`
//!   with the interned field name. `Type::Any` field values are
//!   unboxed with `any_unbox_tag`/`_value` shims and their payload's
//!   refcount is bumped via `any_payload_rc_inc` so the bucket owns
//!   the +1 (`{p: inner}.p === inner` identity + recursive field
//!   access `outer.p.x` preservation, P4.0 semantics).
//!
//! Method body is byte-for-byte preserved from the source; the sibling
//! reaches LowerCtx fields via `impl<'a> super::LowerCtx<'a>`, so call
//! sites (`ssa_lower_stmt_let_decl.rs`) need zero edits.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type, ValueId};
use crate::ssa_lower::LowerCtx;
use torajs_wtf8::{Wtf8, Wtf8Buf};

impl<'a> LowerCtx<'a> {
    /// The live dynobj pointer — a fresh Load off the shared init
    /// slot (never cache a pre-set pointer across a set: resize
    /// frees the old block).
    pub(crate) fn load_dynobj(&mut self, slot: ValueId) -> ValueId {
        self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::Ptr, Operand::Value(slot), 0),
            Type::Ptr,
            None,
        )
    }

    /// P3.2 — `let x: any = { f1: v1, f2: v2 }` lowering. Allocate
    /// a dynobj via `__torajs_dynobj_alloc()`, populate each field
    /// via `dynobj_set`, then box the dynobj ptr as ANY_HEAP=4 so
    /// the slot holds an Any-box pointing at the dynobj. Subsequent
    /// `x.foo` reads/writes route through the dynobj substrate.
    /// Empty `{}` produces a zero-entry dynobj (allocates the header
    /// + initial bucket array but no entries).
    /// Lower one non-special field's value and store it into the
    /// dynobj bucket. The `Any` and `Str` arms complete the store
    /// themselves (their owned-unbox fuses the payload inc into the
    /// decode), so they return early rather than yielding a
    /// `(tag, slot)` pair to the shared tail.
    pub(crate) fn emit_dynobj_field_value(
        &mut self,
        slot: ValueId,
        fname: &Wtf8,
        fval_eid: ExprId,
        runtime_key: Option<ValueId>,
        fresh: crate::ssa_lower_dynobj_init_computed::SetLane,
    ) {
        let v_raw = self.lower_expr(fval_eid);
        // Chunk 570 — SHARE: the bucket takes its own +1 (the
        // refcounted arm's rc_inc / the Any arm's payload inc);
        // no consume, so a borrow-shape value keeps the source
        // binding's stake and an owned temp releases its
        // surplus reference after the set (was a 32B/iter
        // orphan leak, probe-proven).
        let transfers = self.expr_transfers_ownership(fval_eid);
        let v_ty = self.operand_ty(&v_raw);
        let v_keep = v_raw.clone();
        let (tag, val_op): (i64, Operand) = match v_ty {
            Type::I64 | Type::I32 => (2, v_raw),
            Type::F64 => {
                let bits = self.f.append_inst(
                    self.cur_block,
                    InstKind::BitCastF64ToI64(v_raw),
                    Type::I64,
                    None,
                );
                (3, Operand::Value(bits))
            }
            Type::Bool => {
                let zext = self.f.append_inst(
                    self.cur_block,
                    InstKind::ZExtBoolToI64(v_raw),
                    Type::I64,
                    None,
                );
                (1, Operand::Value(zext))
            }
            // P4.0 — Type::Any must be unboxed BEFORE the
            // is_refcounted catch-all (Type::Any is itself
            // refcounted, so the `_ if v_ty.is_refcounted()`
            // arm would otherwise grab the any-box wrapper
            // ptr and store *that* as the bucket value with
            // tag=ANY_HEAP. Reads then return the wrapper ptr
            // instead of the underlying heap object, breaking
            // identity (`{p: inner}.p === inner`) and recursive
            // field access (`outer.p.x`). Forward (tag, val) via
            // any_unbox_tag/_value shims (Step 7c — was inline
            // `Load i64 +8/+16` direct-offset); bucket owns the
            // +1 on val via any_payload_rc_inc when tag == HEAP.
            Type::Any => {
                // Chunk 610 — owned unbox fuses unbox_value +
                // payload_rc_inc (ShortStr materialize was
                // double-counted by the separate inc and leaked).
                let tag_v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.any_unbox_tag, vec![v_raw.clone()]),
                    Type::I64,
                    None,
                );
                let val_v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.any_unbox_value_owned, vec![v_raw.clone()]),
                    Type::I64,
                    None,
                );
                self.emit_dynobj_set_for(
                    slot,
                    fname,
                    runtime_key,
                    Operand::Value(tag_v),
                    Operand::Value(val_v),
                    fresh,
                );
                if transfers {
                    self.emit_drop_value(v_keep, Type::Any);
                }
                return;
            }
            // RFC 20260707 chunk 3 — a Str slot decodes its
            // three shapes at runtime (NULL = null / undefined
            // sentinel / heap Str), so the tag is not static;
            // same continue shape as the Any arm. The value
            // half takes the bucket's +1 (heap case only).
            Type::Str => {
                let tag_v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.anyv_str_slot_tag, vec![v_raw.clone()]),
                    Type::I64,
                    None,
                );
                let val_v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.anyv_str_slot_value, vec![v_raw.clone()]),
                    Type::I64,
                    None,
                );
                self.emit_dynobj_set_for(
                    slot,
                    fname,
                    runtime_key,
                    Operand::Value(tag_v),
                    Operand::Value(val_v),
                    fresh,
                );
                if transfers {
                    self.emit_drop_value(v_keep, Type::Str);
                }
                return;
            }
            _ if v_ty.is_refcounted() => {
                // A typed Array stored into a dynobj bucket is read
                // back through the `any` world (`o.items[1]`), where
                // the elem-kind header picks the slot interpretation
                // — same boundary as the object_lit field store; a
                // raw-i64 array without the mark decodes its cells
                // as NaN-boxes and reads undefined. No-op for
                // non-Arr values.
                self.emit_arr_mark_kind(&v_raw);
                self.emit_rc_inc(v_raw.clone());
                (4, v_raw)
            }
            Type::Ptr if matches!(v_raw, Operand::ConstPtrNull) => (0, Operand::ConstI64(0)),
            // The same `null` arriving through a SLOT rather than as
            // the folded constant above — `var x = null; export { x
            // as default }` reads the local, and the namespace object
            // literal stores what it read. This is `box_to_any`'s
            // decision for a non-constant Ptr, and the any-lane
            // readers have answered `null` for a null payload since
            // P3.2 (`o.d = x` goes through it). No stake to take:
            // every heap type with an owner has its own refcounted
            // arm above, and a nested literal boxed to Any before
            // reaching here.
            Type::Ptr => (4, v_raw),
            _ => panic!("ssa-lower: dynobj init unsupported field type {v_ty:?}"),
        };
        self.emit_dynobj_set_for(
            slot,
            fname,
            runtime_key,
            Operand::ConstI64(tag),
            val_op,
            fresh,
        );
        if transfers && v_ty.is_refcounted() {
            self.emit_drop_value(v_keep, v_ty);
        }
    }

    pub(crate) fn lower_dynobj_init(&mut self, eid: ExprId) -> Operand {
        let fields = match self.ast.get_expr(eid).clone() {
            Expr::ObjectLit { fields } => fields,
            _ => panic!("lower_dynobj_init called on non-ObjectLit"),
        };
        // Allocate the dynobj + the single relocation slot every
        // per-field set shares (see `emit_dynobj_set`).
        let dynobj = self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.dynobj_alloc, Vec::new()),
            Type::Ptr,
            None,
        );
        let lanes = self.objlit_set_lanes(&fields);
        let slot = self.alloca(Type::Ptr, Some("__dynobj_init_slot"));
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::Value(dynobj), Operand::Value(slot), 0),
        );
        // Rotation 549 — the fresh dynobj owns every field set so far
        // and nothing owns IT until the literal answers: park the
        // relocation slot so a throw inside a later field's lower
        // drops the live block (`{ a: mk(i), b: boom() }` stranded the
        // object and its fields per caught throw, 214MB over 600k).
        let park = self.push_throw_slot(slot);
        // For each (name, value), set into the dynobj. Box value
        // first using the same scheme as box_to_any but inlined.
        for ((fname, fval_eid), fresh) in fields.into_iter().zip(lanes) {
            // RFC 20260725-objlit-computed-key 刀 3 — a computed-key
            // field evaluates its key at runtime (field order = spec
            // evaluation order) and stores under the ToString'd name.
            if let Some(&key_eid) = self.ast.objlit_computed_keys.get(&fval_eid) {
                // P-SURF S2.27 — a computed ACCESSOR face routes
                // through the accessor define kernel with the runtime
                // key (`DefineKey::Expr` — §7.1.19 Symbol pass-through
                // included); a computed data field keeps the
                // key-parameterized store.
                if let Some(&is_get) = self.ast.objlit_computed_accessors.get(&fval_eid) {
                    let what = if is_get {
                        "accessor getter"
                    } else {
                        "accessor setter"
                    };
                    self.guard_anylane_recv_face(fval_eid, what);
                    crate::ssa_lower_accessor::emit_accessor_define_into(
                        self,
                        slot,
                        &crate::ssa_lower_object_define::DefineKey::Expr(key_eid),
                        if is_get { Some(fval_eid) } else { None },
                        if is_get { None } else { Some(fval_eid) },
                        Some(true),
                        Some(true),
                    );
                    continue;
                }
                self.emit_dynobj_computed_field(slot, key_eid, fval_eid, fresh);
                continue;
            }
            // §13.2.5.5 — the literal `__proto__: v` member sets
            // [[Prototype]], never an own entry (RFC
            // 20260717-user-proto-chain): a cell lands in the
            // simulation slot, null marks the null-proto bit, any
            // other value is silently ignored — exactly the Annex B
            // setter core's contract (the fresh literal cannot be
            // non-extensible or form a cycle, so its refusal path
            // is unreachable). The value box is a borrow (the core
            // takes its own stake).
            if fname == "__proto__" && !self.ast.objlit_shorthand_proto_exprs.contains(&fval_eid) {
                self.emit_dynobj_proto_field(slot, fval_eid);
                continue;
            }
            // RFC 20260717-objlit-anylane-recv knife 1 — accessor
            // shorthand members (`{ get baz() {} }` parses to a
            // `__getter_baz` field) install a REAL AccessorPair entry
            // instead of a data field (which read back undefined and
            // answered no descriptor — the test262
            // verifyProperty-undefined-desc face). A this-using face
            // was promoted to the `__this: any` receiver-first shape
            // by `objlit_nominal`; a face still carrying a nominal
            // `__this` reached this lane through a route the AST
            // predicate can't see — reject loudly, the struct-typed
            // body would read garbage off the dynobj receiver.
            if let Some(prop) = fname.strip_prefix("__getter_").map(Wtf8Buf::from) {
                self.guard_anylane_recv_face(fval_eid, "accessor getter");
                self.emit_dynobj_accessor_field(slot, &prop, fval_eid, true);
                continue;
            }
            if let Some(prop) = fname.strip_prefix("__setter_").map(Wtf8Buf::from) {
                self.guard_anylane_recv_face(fval_eid, "accessor setter");
                self.emit_dynobj_accessor_field(slot, &prop, fval_eid, false);
                continue;
            }
            // Rotation 267 — `{ ...src }` runs the §7.3.25 runtime
            // CopyDataProperties walk into the fresh dynobj
            // (pointer-slot form: a member_set resize writes the
            // relocated block back through `slot`). Pre-fix the
            // sentinel fell through to the general path and stored a
            // literal `__spread__` key. MUST run before the
            // `undefined` fast arm below — `{ ...undefined }`'s value
            // expr IS `Ident("undefined")` (a nullish source
            // contributes nothing inside the kernel). The
            // destructuring-rest omit form copies then deletes the
            // excluded keys (recorded divergence: their getters run).
            if let Some(omit) = crate::check_type_of_object_lit::spread_omit_set(&fname) {
                let src = self.lower_expr(fval_eid);
                let src_ty = self.operand_ty(&src);
                let src_any = if matches!(src_ty, Type::Any) {
                    src.clone()
                } else {
                    self.box_to_any_from_expr(fval_eid, src.clone())
                };
                // §7.3.25 excludedItems ride to the kernel as one Str
                // cell of comma-separated names — the same spelling the
                // sentinel used to carry them here. The walk skips them
                // BEFORE [[Get]], which is the whole point: copying and
                // then deleting answers with the right properties but
                // runs the excluded getters, and that is a side effect
                // the program can see.
                let excluded = if omit.is_empty() {
                    Operand::ConstPtrNull
                } else {
                    Operand::Value(self.intern_string_literal(&omit.join(",")))
                };
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(
                        self.intrinsics.dynobj_spread_from,
                        vec![Operand::Value(slot), src_any, excluded],
                    ),
                );
                self.release_owned_temp(fval_eid, &src);
                self.emit_throw_check(None);
                continue;
            }
            // RFC 20260712-object-create-define-props — a nested
            // ObjectLit recurses through the dynobj lane (mirror of
            // lower_array_any_literal's nested-array recursion): the
            // whole literal tree lives in the any world, so runtime
            // descriptor walks (defineProperties runtime props /
            // Object.create props) and reflection see dynobj-backed
            // inner objects instead of anon-stamped structs. The
            // fresh dynobj's +1 transfers into the bucket (no inc,
            // no drop). Spread sentinels keep the general path.
            // An `undefined` field stores the ANY_UNDEF slot pair —
            // the general lower answers the same ConstPtrNull shape
            // as `null`, silently collapsing the two (`{u: undefined}`
            // read back `=== null`; probe-proven). A local binding
            // shadowing `undefined` keeps the general path.
            if matches!(self.ast.get_expr(fval_eid), Expr::Ident(n) if n == "undefined")
                && !self.locals.contains_key("undefined")
            {
                self.emit_dynobj_set(
                    slot,
                    &fname,
                    Operand::ConstI64(5),
                    Operand::ConstI64(0),
                    fresh,
                );
                continue;
            }
            // `as` casts are value-layer pass-throughs — strip them so
            // `{ p: {} as any }` recurses the same as `{ p: {} }`.
            let mut lit_eid = fval_eid;
            while let Expr::As { expr, .. } = self.ast.get_expr(lit_eid) {
                lit_eid = *expr;
            }
            if fname != "__spread__" && matches!(self.ast.get_expr(lit_eid), Expr::ObjectLit { .. })
            {
                let nested = self.lower_dynobj_init(lit_eid);
                self.emit_dynobj_set(slot, &fname, Operand::ConstI64(4), nested, fresh);
                continue;
            }
            self.emit_dynobj_field_value(slot, &fname, fval_eid, None, fresh);
        }
        self.pop_throw_temp(park);
        // The live pointer — a set may have relocated the block.
        let out = self.load_dynobj(slot);
        Operand::Value(out)
    }

    /// RFC 20260717-objlit-anylane-recv knife 1 — install one accessor
    /// shorthand face as a live AccessorPair entry on the fresh
    /// dynobj. Rides `emit_accessor_define` wholesale (the dynobj
    /// boxes to an Any face for its receiver arm; object-literal
    /// accessors are enumerable + configurable per §13.2.5.5, and the
    /// define kernel's redefine merge folds a get + set on the same
    /// prop into one entry). `lower_accessor_face` answers the
    /// receiver-first BOXED|RECV kind for promoted this-using faces
    /// and generic kinds for this-free ones.
    fn emit_dynobj_accessor_field(
        &mut self,
        slot: ValueId,
        prop: &Wtf8,
        face_eid: ExprId,
        is_get: bool,
    ) {
        crate::ssa_lower_accessor::emit_accessor_define_into(
            self,
            slot,
            &crate::ssa_lower_object_define::DefineKey::Name(prop),
            if is_get { Some(face_eid) } else { None },
            if is_get { None } else { Some(face_eid) },
            Some(true),
            Some(true),
        );
    }

    /// Loud-reject guard for an accessor face that READS a struct-typed
    /// `__this` — a route the AST any-lane predicate can't see (`{...}
    /// as any` non-empty / ObjectLit into a user any param): its struct
    /// offsets off a dynobj receiver read garbage. A receiver-first face
    /// is exempt — `objlit_nominal_settle` owns that admission rule.
    fn guard_anylane_recv_face(&self, face_eid: ExprId, what: &str) {
        if let Expr::Closure { fn_name, .. } = self.ast.get_expr(face_eid)
            && let Some(ann) = self.closure_this_ann(fn_name)
            && ann != "any"
            && !self.ast.fnexpr_recv_fns.contains(fn_name)
        {
            panic!(
                "ssa-lower: object-literal {what} with a struct-typed receiver \
                 (`{fn_name}`, this: {ann}) reached the any lane — not yet supported"
            );
        }
    }

    /// RFC 20260717 knife 2g promotion gate — an ObjectLit at an
    /// any-lane argv slot is safe to promote to the dynobj lane only
    /// when every closure-valued field (method or accessor face,
    /// recursively through nested literals) carries an any-face
    /// `__this` or none: a nominal-this face would hit
    /// `guard_anylane_recv_face`'s reject and turn a working
    /// struct-lane shape into a compile error (gate regression
    /// error-proto-tostring-001's `.call({ get name() {...} })`),
    /// and a promoted nominal method reads struct offsets off a
    /// dynobj receiver if invoked. Non-promotable literals keep the
    /// pre-existing struct route.
    pub(crate) fn objlit_promotable(&self, eid: ExprId) -> bool {
        let Expr::ObjectLit { fields } = self.ast.get_expr(eid) else {
            return false;
        };
        fields
            .iter()
            .all(|(_, feid)| match self.ast.get_expr(*feid) {
                Expr::Closure { fn_name, .. } => {
                    matches!(
                        self.closure_this_ann(fn_name).as_deref(),
                        None | Some("any")
                    )
                }
                Expr::ObjectLit { .. } => self.objlit_promotable(*feid),
                _ => true,
            })
    }

    /// The `__this` param ann of a lifted closure's FnDecl — `None`
    /// when the fn has no receiver param (this-free face).
    fn closure_this_ann(&self, fn_name: &str) -> Option<String> {
        for s in &self.ast.stmts {
            if let crate::ast::Stmt::FnDecl { name, params, .. } = s
                && name == fn_name
            {
                return params
                    .iter()
                    .find(|p| p.name == "__this")
                    .map(|p| p.type_ann.clone().unwrap_or_default());
            }
        }
        None
    }
}
