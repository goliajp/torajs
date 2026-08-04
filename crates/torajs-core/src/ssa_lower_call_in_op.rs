//! Synthetic `__torajs_in_op(key, obj)` callable lowered as chunk-41
//! of the `Expr::Call` god-arm decomp (chunks 1-40 = ... + Date.UTC
//! arity 1-6 trailing-default pad).
//!
//! T-45 — the parser rewrites a binary `<key> in <obj>` to this
//! synthetic call so the lowering can switch on the obj's static SSA
//! type:
//!
//! - `Type::Arr(_)` — numeric bounds check `key ∈ [0, len)` with the
//!   key coerced to `I64`. ES §13.10.1 lookup for indexed Array.
//! - `Type::Obj(sid)` — typed struct rhs: the field set is statically
//!   known from the layout, so a literal-string key folds to a compile-
//!   time `ConstBool`. Non-literal keys are rejected (the right answer
//!   would always be the literal-key constant fold anyway).
//! - `Type::Any` — dispatch on the **key**'s static type:
//!   `Number → __torajs_in_op_any_num`,
//!   `String → __torajs_in_op_any_str`, and an `any` key through
//!   §7.1.19 first (the resolver the define family shares) so a
//!   symbol riding an `any` reaches the str kernel as itself.
//!   Both helpers read `HeapHeader::type_tag@+4` and route by Tag
//!   (`Arr → bounds check`, `DynObj → __torajs_dynobj_has`, else
//!   `false`). The old single-path arm called `dynobj_has`
//!   unconditionally and SIGSEGV'd when the Any cell was actually an
//!   Array (see `torajs-rc::in_op_any`).
//!
//! Returns `Some(op)` on hit; `None` on miss (callee not the
//! `__torajs_in_op` synthetic Ident or arity != 2). Unsupported
//! `obj_ty` / `key_ty` combinations `panic!` — the typechecker is
//! expected to reject them upstream, so reaching the panic indicates
//! a compiler bug.

use crate::ast::{Expr, ExprId};
use crate::ssa::{IPred, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Ident(n) = ctx.ast.get_expr(callee) else {
        return None;
    };
    if n != "__torajs_in_op" || args.len() != 2 {
        return None;
    }
    let key_op = ctx.lower_expr(args[0]);
    let obj_op = ctx.lower_expr(args[1]);
    let obj_ty = ctx.operand_ty(&obj_op);
    // A function value is an Object per §13.10.1 — box the closure
    // cell (borrow'd NaN-box, no rc traffic) and take the Any
    // kernels' full own + prototype-chain face. The fn-as-value
    // collector already wrapped a bare top-FnDecl Ident rhs into its
    // forwarder closure, so a Closure operand is what reaches here.
    let (mut obj_op, mut obj_ty) = if matches!(obj_ty, Type::Closure(_)) {
        (ctx.box_to_any(obj_op), Type::Any)
    } else {
        (obj_op, obj_ty)
    };
    let cur_block = ctx.cur_block;
    if matches!(obj_ty, Type::Arr(_)) {
        // 刀 13d (RFC 20260721-array-proto-cluster) — was a raw
        // `0 <= key < len` bounds check, blind to hole shadows
        // (delete / length-grow / elision) and to prototype digit
        // keys. The kernel keeps the clean in-bounds answer at one
        // flag test and walks §7.3.11 for everything else.
        let key_i64 = ctx.coerce_to_i64(key_op);
        let live = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.arr_has_index, vec![obj_op, key_i64]),
            Type::I64,
            None,
        );
        let r = ctx.f.append_inst(
            cur_block,
            InstKind::ICmp(IPred::Ne, Operand::Value(live), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        return Some(Operand::Value(r));
    }
    let obj_sid = if let Type::Obj(sid) = &obj_ty {
        Some(*sid)
    } else {
        None
    };
    if let Some(sid) = obj_sid {
        let layout = ctx.struct_layouts[sid.0 as usize].clone();
        if let Expr::String(s) = ctx.ast.get_expr(args[0]) {
            // An accessor is an own property (§10.4) but sits in the
            // layout under a synthetic slot name, so the plain-name
            // scan alone would deny it (RFC 20260714-objlit-accessor).
            let present = layout.iter().any(|(n, _)| {
                n == s
                    || crate::check_type_of_object_lit::accessor_slot(n)
                        .is_some_and(|(_, prop)| prop == s)
            });
            if present {
                return Some(Operand::ConstBool(true));
            }
        }
        // The layout scan can only prove PRESENCE. Absence is not a
        // constant: `in` is HasProperty (§7.3.12), and class-prototype
        // members plus the Object.prototype root answer through the
        // runtime chain — box the struct (borrow'd NaN-box) and take
        // the Any kernels' face. Non-literal keys route the same way,
        // which retires the old literal-key-only panic.
        obj_op = ctx.box_to_any(obj_op);
        obj_ty = Type::Any;
    }
    if matches!(obj_ty, Type::Any) {
        let key_ty = ctx.operand_ty(&key_op);
        if matches!(key_ty, Type::I64 | Type::F64) {
            let key_i64 = ctx.coerce_to_i64(key_op);
            let r = ctx.f.append_inst(
                cur_block,
                InstKind::Call(ctx.intrinsics.in_op_any_num, vec![obj_op, key_i64]),
                Type::Bool,
                None,
            );
            // The kernel arms a TypeError for a non-Object rhs
            // (§13.10.1 step 5) and still returns a bool, so without
            // this the throw would sit pending and `"q" in undefined`
            // would answer false instead of throwing.
            ctx.emit_throw_check(None);
            return Some(Operand::Value(r));
        }
        // §6.1.7 — a Symbol key rides the same kernel as a Str key,
        // uncoerced per §7.1.19 step 2: the own probe and the user
        // [[Prototype]] walk both key off the cell's own tag, and the
        // kernel's name-keyed chain faces bow out for a symbol.
        if matches!(key_ty, Type::Str | Type::Symbol) {
            let r = ctx.f.append_inst(
                cur_block,
                InstKind::Call(ctx.intrinsics.in_op_any_str, vec![obj_op, key_op]),
                Type::Bool,
                None,
            );
            // See the num arm — same pending-throw contract.
            ctx.emit_throw_check(None);
            return Some(Operand::Value(r));
        }
        // An `any` key only names its kind at run time, and §7.1.19
        // is defined on the value, so it resolves through the same
        // kernel the define family asks: a Symbol arrives as itself,
        // everything else as its ToString. Without this the whole
        // program was rejected, which is how `sym in o` reads when
        // the symbol travels in an `any` — the usual way it travels.
        if matches!(key_ty, Type::Any) {
            let k = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.anyv_to_property_key, vec![key_op.clone()]),
                Type::Ptr,
                None,
            );
            ctx.release_owned_temp(args[0], &key_op);
            ctx.emit_throw_check(None);
            let r = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.in_op_any_str,
                    vec![obj_op, Operand::Value(k)],
                ),
                Type::Bool,
                None,
            );
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.anyv_property_key_drop,
                    vec![Operand::Value(k)],
                ),
            );
            // See the num arm — same pending-throw contract.
            ctx.emit_throw_check(None);
            return Some(Operand::Value(r));
        }
        panic!("ssa-lower: `<key> in <obj:any>` unsupported key type {key_ty:?}");
    }
    panic!("ssa-lower: `in` rhs unsupported type {obj_ty:?}");
}

/// ES2022 §13.10 ergonomic brand check —
/// `__torajs_priv_in_op(key, obj)`, the parser's synthetic for
/// `#x in o` (rotation 297). The key arrives as the resolved
/// `__priv_<C>__<x>` String literal (compiler-minted, interned
/// directly — never a runtime value); the rhs boxes to Any and the
/// `__torajs_any_member_priv_has` kernel answers the own-brand
/// probe, throwing the §13.10.1 step-5 TypeError for a non-Object
/// rhs.
pub(crate) fn try_lower_priv(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Ident(n) = ctx.ast.get_expr(callee) else {
        return None;
    };
    if n != "__torajs_priv_in_op" || args.len() != 2 {
        return None;
    }
    let Expr::String(key) = ctx.ast.get_expr(args[0]) else {
        return None;
    };
    let key = key.clone();
    let key_str = ctx.intern_string_literal(&key);
    let obj_op = ctx.lower_expr(args[1]);
    let obj_ty = ctx.operand_ty(&obj_op);
    let boxed = if matches!(obj_ty, Type::Any) {
        obj_op.clone()
    } else {
        // Borrow'd NaN-box (no rc traffic) — same face the `in`
        // struct arm takes.
        ctx.box_to_any(obj_op.clone())
    };
    let cur_block = ctx.cur_block;
    let r = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.any_member_priv_has,
            vec![boxed, Operand::Value(key_str)],
        ),
        Type::Bool,
        None,
    );
    // Kernel arms the non-Object TypeError and still returns a bool
    // — same pending-throw contract as the `in` kernels.
    ctx.emit_throw_check(None);
    ctx.release_owned_temp(args[1], &obj_op);
    Some(Operand::Value(r))
}
