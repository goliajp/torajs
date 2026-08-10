//! `Object.getPrototypeOf(arg)` namespace static method pulled out of
//! [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` dispatch as chunk-19
//! of the `Expr::Call` god-arm decomp (chunks 1-18 = Arr higher-order +
//! Map dispatch + Set dispatch + Arr.push + Number instance methods +
//! bare-name globals + Str regex methods + Number namespace + Array.from +
//! Arr predicate iter + Arr.flatMap + Object.entries + fn-indirect +
//! Number/String/Boolean coercion + universal methods + closure-local +
//! Object.values + Object.keys).
//!
//! P4.2 Phase B+C — three receiver routes:
//! - `Type::Obj(_)`: typed-tier instance reads its runtime class tag
//!   from `OBJ_CLASS_TAG_OFF` and looks up the registered `__proto_<C>`
//!   Any-box via the runtime side table (`__torajs_proto_get`).
//! - `Type::Any`: route through `__torajs_get_proto_of_any` which reads
//!   the `__proto__` field from the wrapped dynobj.
//! - Other types: return ANY_NULL Any-box (tag 0 — no prototype
//!   available). Pre-P4.2 the stub returned `ConstPtrNull` unconditionally.
//!
//! BORROW semantics: getPrototypeOf reads the argument's class tag /
//! `__proto__` field but does not take ownership. For Ident args, leave
//! the binding's `moved` flag alone (end-of-scope drop handles the dec).
//! For non-Ident temps the caller built up an owned value — dec it once
//! we're done reading. Pre-fix `consume_if_ident + emit_drop_value` for
//! the Ident case caused a double-decrement: the intercept dec'd the
//! slot's value (freeing it) and subsequent reads of the ident saw a
//! dangling pointer (UAF).
//!
//! S265 — ES §20.1.2.12 step 1 trailing-arg ignore: ToObject(obj)
//! discards anything beyond the first arg; lower-and-drop them for
//! spec L-to-R side-effect order (check.rs already typecheck-drops them).
//!
//! Returns `Some(result)` when callee matches `Object.getPrototypeOf`
//! with a non-empty args list. `None` lets the caller fall through to
//! the next arm.

use crate::ast::{Expr, ExprId};
use crate::check::Type as CheckType;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{LowerCtx, OBJ_CLASS_TAG_OFF};

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let (ns_id, m_name) = match ctx.ast.get_expr(callee) {
        Expr::Member { obj, name } => (*obj, name.clone()),
        _ => return None,
    };
    if m_name != "getPrototypeOf" {
        return None;
    }
    let Expr::Ident(ns) = ctx.ast.get_expr(ns_id) else {
        return None;
    };
    // §28.1.4 Reflect.getPrototypeOf (rotation 266 刀 R2) shares
    // this lowering with a strict IsObject gate emitted before the
    // general dispatch (every primitive throws, no ToObject wrap).
    // The static fast paths below (generator factory / top-level fn)
    // are object receivers by construction, so they stay valid for
    // both flavors.
    let is_reflect = ns == "Reflect";
    if ns != "Object" && !is_reflect {
        return None;
    }
    if args.is_empty() {
        return None;
    }
    // S265 — eval-and-drop trailing args (silent-ignore per ES
    // §20.1.2.12 step 1 — ToObject(obj) discards anything beyond the
    // first arg).
    for a in args.iter().skip(1) {
        let _ = ctx.lower_expr(*a);
    }
    if let Some(res) = lower_nullish_or_any_gate(ctx, args[0], is_reflect) {
        return Some(res);
    }
    // RFC 20260713 blade 5 cut 4 — a generator factory fn as the
    // receiver: `Object.getPrototypeOf(g)` answers the
    // %GeneratorFunction.prototype% / %AsyncGeneratorFunction
    // .prototype% intrinsic singleton per §27.3 (compile-time
    // resolved from the factory name; the fn value itself is never
    // materialized).
    if let Expr::Ident(fn_name) = ctx.ast.get_expr(args[0])
        && ctx.ast.generator_factory_classes.contains_key(fn_name)
    {
        let kind: i64 = if ctx.ast.async_generator_fns.contains(fn_name) {
            1
        } else {
            0
        };
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.genfn_proto, vec![Operand::ConstI64(kind)]),
            Type::Any,
            None,
        );
        return Some(Operand::Value(v));
    }
    // Ordinary top-level fn receiver — §10.2 a function's
    // [[Prototype]] is %Function.prototype% (builtin-proto tag 13),
    // mirroring the runtime classifier's Tag::Closure answer. Bare
    // fn idents lower to non-refcounted FnSig operands, which used
    // to fall through to the null arm.
    if let Expr::Ident(fn_name) = ctx.ast.get_expr(args[0])
        && ctx.fn_table.contains_key(fn_name)
    {
        let proto = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.get_builtin_prototype,
                vec![Operand::ConstI64(13)],
            ),
            Type::Ptr,
            None,
        );
        // Owned box off the borrowed singleton slot — the consumer's
        // drop balances (see any_boxed_builtin_proto's twin note).
        ctx.emit_rc_inc(Operand::Value(proto));
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.any_box,
                vec![Operand::ConstI64(4), Operand::Value(proto)],
            ),
            Type::Any,
            None,
        );
        return Some(Operand::Value(v));
    }
    // BORROW semantics — see module doc for rc-balance rationale.
    let v = ctx.lower_expr(args[0]);
    let v_ty = ctx.operand_ty(&v);
    // Reflect flavor — strict IsObject gate before the dispatch: a
    // typed Str/BigInt/Symbol (spec primitives) or non-heap typed
    // value throws here instead of answering the Object-flavor
    // wrapper proto / null-box.
    if is_reflect {
        crate::ssa_lower_object_define::emit_receiver_typecheck(ctx, args[0], &v, v_ty.clone());
    }
    let arg_is_ident = matches!(ctx.ast.get_expr(args[0]), Expr::Ident(_));
    let proto = match v_ty {
        // An EMPTY struct layout is a dynobj at runtime (`{}` has no
        // struct surface — the dynobj-certain shape), so the
        // class-tag load below would read a dynobj header field as a
        // tag. Route it through the runtime classifier instead — the
        // TAG_DYNOBJ arm answers %Object.prototype% (the inline
        // `Object.getPrototypeOf({})` spelling; a bound `{}` already
        // arrived Any-typed and never hit this arm).
        Type::Obj(sid) if ctx.struct_layouts[sid.0 as usize].is_empty() => {
            let boxed = ctx.box_to_any(v.clone());
            ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.get_proto_of_any, vec![boxed]),
                Type::Any,
                None,
            )
        }
        Type::Obj(_) => {
            let tag = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::I64, v.clone(), OBJ_CLASS_TAG_OFF),
                Type::I64,
                None,
            );
            ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.proto_get, vec![Operand::Value(tag)]),
                Type::Any,
                None,
            )
        }
        Type::Any => ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.get_proto_of_any, vec![v.clone()]),
            Type::Any,
            None,
        ),
        // RFC 20260713 blade 3 — typed heap receivers (Arr / Str /
        // Date / RegExp / Map / Set / Closure …) route through the
        // same runtime classifier, which answers the builtin
        // `<Ctor>.prototype` singleton per §10.1.1 (`getPrototypeOf(
        // [1]) === Array.prototype`). box_to_any is a pure pointer
        // encode for heap values — no rc traffic.
        t if t.is_refcounted() => {
            let boxed = ctx.box_to_any(v.clone());
            ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.get_proto_of_any, vec![boxed]),
                Type::Any,
                None,
            )
        }
        _ => ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.any_box,
                vec![Operand::ConstI64(0), Operand::ConstI64(0)],
            ),
            Type::Any,
            None,
        ),
    };
    if !arg_is_ident {
        // Temp value — we own the refcount, dec now.
        ctx.emit_drop_value(v, v_ty);
    }
    // proto_get / get_proto_of_any / any_box(NULL) all return an OWNED
    // Any-box (rc 1+). Caller takes ownership without further rc_inc.
    Some(Operand::Value(proto))
}

/// §20.1.2.12 step 1 — ToObject(obj) throws TypeError when obj is
/// null or undefined. Pre-fix tr silently returned null Any across
/// every route (Type::Any read `__proto__` off a nullish box; typed
/// primitives fell through to the `_` arm's ANY_NULL). test262
/// Object/getPrototypeOf/15.2.3.2-1-2 pins the throw. Compile-time
/// known nullish literal → emit unconditional throw and return a
/// dummy null Any-box (control flow already re-routed by throw
/// check); runtime-Any (or Nullable<T>) → gate then continue through
/// the Any reflection route.
///
/// Returns `Some(result)` when the arg is nullish literal / any /
/// nullable — caller propagates it up. `None` lets the caller fall
/// through to the typed reflection routes below.
fn lower_nullish_or_any_gate(
    ctx: &mut LowerCtx<'_>,
    arg0_eid: ExprId,
    strict: bool,
) -> Option<Operand> {
    let checker_ty0 = ctx.expr_types.get(&arg0_eid).cloned();
    let is_nullish_lit = matches!(
        checker_ty0,
        Some(CheckType::Null) | Some(CheckType::Undefined)
    );
    let is_any_shape = matches!(
        checker_ty0,
        Some(CheckType::Any) | Some(CheckType::Nullable(_))
    );
    if !is_nullish_lit && !is_any_shape {
        return None;
    }
    let boxed = if is_nullish_lit {
        // Compile-time null / undefined literal — any_box(NULL, NULL)
        // shapes an Any that reads as nullish, so the helper reliably
        // throws. Preserves L-to-R side-effect order: we still lower
        // the arg first even though the value is a compile-time
        // constant (the lower can drag side effects out of a `void
        // 0` or similar).
        let _ = ctx.lower_expr(arg0_eid);
        ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.any_box,
                vec![Operand::ConstI64(0), Operand::ConstI64(0)],
            ),
            Type::Any,
            None,
        )
    } else {
        let v = ctx.lower_expr(arg0_eid);
        let v_op = ctx.box_to_any_from_expr(arg0_eid, v);
        match v_op {
            Operand::Value(id) => id,
            other => ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Copy(Type::Any, other),
                Type::Any,
                None,
            ),
        }
    };
    // Reflect flavor (`strict`) swaps the nullish-only guard for the
    // full §10.1.6.3 IsObject gate — every primitive throws.
    let gate_fid = if strict {
        ctx.intrinsics.throw_typeerror_if_not_object
    } else {
        ctx.intrinsics.throw_typeerror_if_props_nullish
    };
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(gate_fid, vec![Operand::Value(boxed)]),
    );
    ctx.emit_throw_check(None);
    // For Nullable<T> the gate returns only when non-null, so the
    // Any-boxed value survives and we can go through the runtime
    // reflection path here. For nullish literal / uncaught-throw
    // the emit_throw_check either propagates or (in main) exits.
    if is_any_shape {
        let arg_is_ident = matches!(ctx.ast.get_expr(arg0_eid), Expr::Ident(_));
        let proto = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.get_proto_of_any, vec![Operand::Value(boxed)]),
            Type::Any,
            None,
        );
        if !arg_is_ident {
            ctx.emit_drop_value(Operand::Value(boxed), Type::Any);
        }
        return Some(Operand::Value(proto));
    }
    // Nullish literal path — the throw is unconditional, but we
    // still need to yield a typed operand for the caller's IR
    // sequencing. Return a null Any-box; nothing reads it because
    // emit_throw_check already re-routed control flow to the
    // catch or the propagation ret.
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_box,
            vec![Operand::ConstI64(0), Operand::ConstI64(0)],
        ),
        Type::Any,
        None,
    );
    Some(Operand::Value(v))
}
