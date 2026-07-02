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
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

impl<'a> LowerCtx<'a> {
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
            let v_raw = self.lower_expr(fval_eid);
            self.consume_if_ident(fval_eid);
            let v_ty = self.operand_ty(&v_raw);
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
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Call(
                            self.intrinsics.any_payload_rc_inc,
                            vec![Operand::Value(tag_v), Operand::Value(val_v)],
                        ),
                    );
                    let key_str = self.intern_string_literal(&fname);
                    let slot = self.alloca(Type::Ptr, Some("__dynobj_init_slot"));
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Store(Operand::Value(dynobj), Operand::Value(slot), 0),
                    );
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Call(
                            self.intrinsics.dynobj_set,
                            vec![
                                Operand::Value(slot),
                                Operand::Value(key_str),
                                Operand::Value(tag_v),
                                Operand::Value(val_v),
                            ],
                        ),
                    );
                    continue;
                }
                _ if v_ty.is_refcounted() => {
                    self.emit_rc_inc(v_raw.clone());
                    (4, v_raw)
                }
                Type::Ptr if matches!(v_raw, Operand::ConstPtrNull) => (0, Operand::ConstI64(0)),
                _ => panic!("ssa-lower: dynobj init unsupported field type {v_ty:?}"),
            };
            let key_str = self.intern_string_literal(&fname);
            let slot = self.alloca(Type::Ptr, Some("__dynobj_init_slot"));
            self.f.append_void(
                self.cur_block,
                InstKind::Store(Operand::Value(dynobj), Operand::Value(slot), 0),
            );
            self.f.append_void(
                self.cur_block,
                InstKind::Call(
                    self.intrinsics.dynobj_set,
                    vec![
                        Operand::Value(slot),
                        Operand::Value(key_str),
                        Operand::ConstI64(tag),
                        val_op,
                    ],
                ),
            );
        }
        Operand::Value(dynobj)
    }
}
