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
    if ns != "Object" {
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
    // BORROW semantics — see module doc for rc-balance rationale.
    let v = ctx.lower_expr(args[0]);
    let v_ty = ctx.operand_ty(&v);
    let arg_is_ident = matches!(ctx.ast.get_expr(args[0]), Expr::Ident(_));
    let proto = match v_ty {
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
