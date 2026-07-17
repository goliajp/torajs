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

impl<'a> LowerCtx<'a> {
    /// Emit the shared `dynobj_set` shape: intern the field name, alloc
    /// the ptr-slot, store the dynobj header ptr into it, then call
    /// `__torajs_dynobj_set(slot, key, tag, val)`. Every field path
    /// (undefined, nested object, plain box, Any/Str runtime tag) ends
    /// here — chunk 819 consolidated the 5 repeated call sites.
    fn emit_dynobj_set(&mut self, dynobj: ValueId, fname: &str, tag: Operand, val: Operand) {
        let key_str = self.intern_string_literal(fname);
        let slot = self.alloca(Type::Ptr, Some("__dynobj_init_slot"));
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::Value(dynobj), Operand::Value(slot), 0),
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.dynobj_set,
                vec![Operand::Value(slot), Operand::Value(key_str), tag, val],
            ),
        );
    }

    /// P3.2 — `let x: any = { f1: v1, f2: v2 }` lowering. Allocate
    /// a dynobj via `__torajs_dynobj_alloc()`, populate each field
    /// via `dynobj_set`, then box the dynobj ptr as ANY_HEAP=4 so
    /// the slot holds an Any-box pointing at the dynobj. Subsequent
    /// `x.foo` reads/writes route through the dynobj substrate.
    /// Empty `{}` produces a zero-entry dynobj (allocates the header
    /// + initial bucket array but no entries).
    pub(crate) fn lower_dynobj_init(&mut self, eid: ExprId) -> Operand {
        let fields = match self.ast.get_expr(eid).clone() {
            Expr::ObjectLit { fields } => fields,
            _ => panic!("lower_dynobj_init called on non-ObjectLit"),
        };
        // Allocate the dynobj.
        let dynobj = self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.dynobj_alloc, Vec::new()),
            Type::Ptr,
            None,
        );
        // For each (name, value), set into the dynobj. Box value
        // first using the same scheme as box_to_any but inlined.
        for (fname, fval_eid) in fields {
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
            if let Some(prop) = fname.strip_prefix("__getter_").map(str::to_string) {
                self.guard_anylane_recv_face(fval_eid, "accessor getter");
                self.emit_dynobj_accessor_field(dynobj, &prop, fval_eid, true);
                continue;
            }
            if let Some(prop) = fname.strip_prefix("__setter_").map(str::to_string) {
                self.guard_anylane_recv_face(fval_eid, "accessor setter");
                self.emit_dynobj_accessor_field(dynobj, &prop, fval_eid, false);
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
                self.emit_dynobj_set(dynobj, &fname, Operand::ConstI64(5), Operand::ConstI64(0));
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
                self.emit_dynobj_set(dynobj, &fname, Operand::ConstI64(4), nested);
                continue;
            }
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
                    self.emit_dynobj_set(
                        dynobj,
                        &fname,
                        Operand::Value(tag_v),
                        Operand::Value(val_v),
                    );
                    if transfers {
                        self.emit_drop_value(v_keep, Type::Any);
                    }
                    continue;
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
                    self.emit_dynobj_set(
                        dynobj,
                        &fname,
                        Operand::Value(tag_v),
                        Operand::Value(val_v),
                    );
                    if transfers {
                        self.emit_drop_value(v_keep, Type::Str);
                    }
                    continue;
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
                _ => panic!("ssa-lower: dynobj init unsupported field type {v_ty:?}"),
            };
            self.emit_dynobj_set(dynobj, &fname, Operand::ConstI64(tag), val_op);
            if transfers && v_ty.is_refcounted() {
                self.emit_drop_value(v_keep, v_ty);
            }
        }
        Operand::Value(dynobj)
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
        dynobj: ValueId,
        prop: &str,
        face_eid: ExprId,
        is_get: bool,
    ) {
        let obj_any = self.box_to_any(Operand::Value(dynobj));
        crate::ssa_lower_accessor::emit_accessor_define(
            self,
            obj_any,
            &crate::ssa_lower_object_define::DefineKey::Name(prop),
            &None,
            if is_get { Some(face_eid) } else { None },
            if is_get { None } else { Some(face_eid) },
            Some(true),
            Some(true),
        );
    }

    /// Loud-reject guard for an accessor face whose `__this` kept the
    /// struct-typed nominal stamp — a route the AST any-lane predicate
    /// can't see (`{...} as any` non-empty / ObjectLit into a user any
    /// param). Its body reads struct offsets off a dynobj receiver;
    /// silently installing it trades a checkable reject for garbage
    /// reads (knife 2 widens the predicate instead).
    fn guard_anylane_recv_face(&self, face_eid: ExprId, what: &str) {
        if let Expr::Closure { fn_name, .. } = self.ast.get_expr(face_eid)
            && let Some(ann) = self.closure_this_ann(fn_name)
            && ann != "any"
        {
            panic!(
                "ssa-lower: object-literal {what} with a struct-typed receiver \
                 (`{fn_name}`, this: {ann}) reached the any lane — not yet supported"
            );
        }
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
