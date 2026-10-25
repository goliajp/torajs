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

pub(crate) mod absent_in;
mod priv_in;
pub(crate) use priv_in::try_lower_priv;

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
    Some(lower_has_property(ctx, key_op, args[0], obj_op))
}

/// §7.3.12 HasProperty on two already-lowered operands — the whole
/// answer, from the shape dispatch down to the kernel call.
///
/// Split out of [`try_lower`] so `Reflect.has` can reach it. §28.1.9
/// steps 1-3 and §13.10.1 steps 5-6 are the same three steps in the
/// same order (non-Object throws, ToPropertyKey, [[HasProperty]]),
/// so the two surfaces are one emit — but their ARGUMENT ORDER is
/// opposite (`k in o` evaluates the key first, `Reflect.has(o, k)`
/// the object), which is why the operands arrive lowered rather than
/// as expressions.
///
/// `key_expr` is the key's source expression, needed only to decide
/// whether an `any` key's ToPropertyKey temp is owned.
pub(crate) fn lower_has_property(
    ctx: &mut LowerCtx<'_>,
    key_op: Operand,
    key_expr: ExprId,
    obj_op: Operand,
) -> Operand {
    let obj_ty = ctx.operand_ty(&obj_op);
    // A function value is an Object per §13.10.1 — box the closure
    // cell (borrow'd NaN-box, no rc traffic) and take the Any
    // kernels' full own + prototype-chain face. The fn-as-value
    // collector already wrapped a bare top-FnDecl Ident rhs into its
    // forwarder closure, so a Closure operand is what reaches here.
    let (obj_op, obj_ty) = if matches!(obj_ty, Type::Closure(_)) {
        (ctx.box_to_any(obj_op), Type::Any)
    } else {
        (obj_op, obj_ty)
    };
    let cur_block = ctx.cur_block;
    // A typed-array rhs only keeps its fast lane under a NUMERIC key.
    // Any other static key kind — Any (only names its kind at run
    // time), Str ("1" is an index key, "q" walks the prototype
    // chain), Bool/Symbol (§7.1.19 gives "true", not 1) — takes the
    // boxed Any face below, which resolves §7.1.19 on the value; a
    // blind coerce_to_i64 here rejected the Any and mis-keyed the
    // rest (S12.6.3_A5's eval-inlined `i in arr`).
    let (mut obj_op, mut obj_ty) = if matches!(obj_ty, Type::Arr(_))
        && !matches!(ctx.operand_ty(&key_op), Type::I64 | Type::F64)
    {
        (ctx.box_to_any(obj_op), Type::Any)
    } else {
        (obj_op, obj_ty)
    };
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
        return Operand::Value(r);
    }
    let obj_sid = if let Type::Obj(sid) = &obj_ty {
        Some(*sid)
    } else {
        None
    };
    if let Some(sid) = obj_sid {
        let layout = ctx.struct_layouts[sid.0 as usize].clone();
        if let Expr::String(s) = ctx.ast.get_expr(key_expr) {
            // An accessor is an own property (§10.4) but sits in the
            // layout under a synthetic slot name, so the plain-name
            // scan alone would deny it (RFC 20260714-objlit-accessor).
            //
            // The synthetic spellings themselves are NOT properties.
            // The runtime lane guards them via `accessor_name_kind`
            // and `Object.hasOwn`'s own fold guards them here — this
            // scan did not, so a bare layout-name match answered true
            // for `"__getter_b" in o`. A synthetic key falls through
            // to the runtime rather than folding, which is where the
            // guard already lives.
            let is_internal_spelling = crate::check_type_of_object_lit::accessor_slot(s).is_some();
            let present = !is_internal_spelling
                && layout.iter().any(|(n, _)| {
                    n == s
                        || crate::check_type_of_object_lit::accessor_slot(n)
                            .is_some_and(|(_, prop)| *prop == **s)
                });
            if present && !crate::ssa_lower_object_lit::absent::own_set_moves(ctx) {
                return Operand::ConstBool(true);
            }
            // a member a literal left out or a delete removed is no own
            // property, and the chain answers for it
            if present {
                let s = s.clone();
                return absent_in::absent_member_in(ctx, obj_op, &s);
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
    // Every other shape takes the same route. `"get" in new Map()` is
    // HasProperty on an ordinary object (§7.3.12) and the Any kernel
    // answers it — own face, then the whole prototype chain — while
    // `"a" in 42` is §13.10.1 step 5's TypeError, which that same
    // kernel arms. Knowing the shape statically used to REJECT the
    // program in both cases. Boxing is a borrow, like the Closure and
    // struct arms above. `FnSig` stays out: a bare function-pointer
    // value has no cell to box, and the fn-as-value collector wraps
    // the spellings that reach here into closures.
    if !matches!(obj_ty, Type::Any | Type::FnSig(_)) {
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
            return Operand::Value(r);
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
            return Operand::Value(r);
        }
        // Everything else resolves through the §7.1.19 kernel the
        // define family asks: a Symbol arrives as itself, everything
        // else as its ToString.
        //
        // An `any` key only names its kind at run time, so it has to
        // come here. A key whose kind IS known — a Bool, an object,
        // a shape with no lane above — comes here too, boxed (a
        // borrow), rather than getting a lane of its own: §7.1.19 is
        // defined on the value, and one resolver answering the whole
        // key domain is what keeps `true in o` and `o["true"]`
        // agreeing. The lanes above are the two fast paths, not the
        // list of admitted kinds — a `panic!` stood here instead and
        // `true in o` was a compile error.
        {
            let key_any = if matches!(key_ty, Type::Any) {
                key_op
            } else {
                ctx.box_to_any(key_op)
            };
            let k = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.anyv_to_property_key, vec![key_any]),
                Type::Ptr,
                None,
            );
            ctx.release_owned_temp(key_expr, &key_op);
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
            return Operand::Value(r);
        }
    }
    panic!("ssa-lower: `in` rhs unsupported type {obj_ty:?}");
}
