//! Reified function VALUES as call targets — the family
//! [`super::route_early`] consults before the static replay lanes.
//!
//! A builtin member value (`const m = s.slice` / `const f =
//! String.fromCharCode`), a namespace static read inline
//! (`Math.max.call(...)`), a class-instance method value, a builtin
//! constructor: each of these types as a `Function` in the checker
//! but LOWERS to a runtime cell, and the call lands on a kernel that
//! runs the spec's own steps. So the signature the member table
//! carries describes the ANSWER, and neither its arity nor its
//! parameter types may be enforced on the way in.
//!
//! Two surfaces, same rule: `.call` / `.apply` / `.bind`, and the
//! plain call.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

/// L3b ⑥ — `Function.prototype.call` / `.apply` on a statically
/// fn-typed VALUE (`const f = add; f.call(u, 2, 3)` /
/// `f.apply(u, [2, 3])`): the named-fn form never reaches here (the
/// chunk-138 AST desugar rewrote it), and an any-held fn keeps the
/// runtime dispatch (the any-receiver arm runs first). The thisArg
/// types for effect then drops (the desugar's no-this subset rule);
/// the remaining args forward to the general fn-call admit AGAINST
/// THE ORIGINAL eid, so its arity gate / per-arg subtype loop /
/// arity-pad recording all key exactly like the lowering wedge's
/// replayed value-callee call (`ssa_lower_call_fn_call_value`, same
/// eid + rest args). `apply` admits the LITERAL argsArray form only
/// — the chunk-138 desugar's own bound; a runtime array needs a
/// variadic spread substrate, so that shape keeps its loud reject.
/// RFC 20260725-str-method-value-reify — `.call` / `.apply` /
/// `.bind` on a reified String-receiver method value: a registered
/// binding (`const m = s.slice; m.call(s, 1)`) or the inline Member
/// form (`s.slice.call(x, 1)`). Any-dispatched so the member
/// table's fixed-arity sig never rejects the optional-argument
/// forms — the runtime re-dispatch is spec-exact.
pub(super) fn try_builtin_mv_fn_surface(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &[ExprId],
) -> Option<Result<Type, String>> {
    let Expr::Member { obj, name } = ast.get_expr(*callee) else {
        return None;
    };
    if !matches!(name.as_str(), "call" | "apply" | "bind") {
        return None;
    }
    let fn_face = matches!(checker.type_of(ast, *obj), Ok(Type::Function(..)))
        && (is_builtin_mv_read(checker, ast, *obj)
            || is_ns_static_read(ast, *obj)
            || is_class_method_read(checker, ast, *obj));
    if !fn_face && !is_builtin_ctor_read(checker, ast, *obj) {
        return None;
    }
    for a in args {
        if let Err(e) = checker.type_of(ast, *a) {
            return Some(Err(e));
        }
    }
    Some(Ok(Type::Any))
}

/// A plain call through a binding holding a reified builtin member
/// value (`const f = String.fromCharCode; f("65")`).
///
/// The arguments are typechecked but not shape-matched against the
/// member table's signature. That signature names what the spec
/// COERCES each argument to, not what it demands: §22.1.2.1 step 2
/// is ToUint16, §21.3.2.24 ToNumber. The direct form has read them
/// that way since rotation 463, and the call lands on the same
/// kernel either way — it just arrives through a binding. The
/// signature's return type still describes the answer, so only the
/// way in relaxes.
///
/// The `.call` / `.apply` / `.bind` surface of the same values is
/// [`try_builtin_mv_fn_surface`] above; this is the direct-call half.
pub(super) fn try_builtin_mv_plain_call(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &[ExprId],
) -> Option<Result<Type, String>> {
    let Expr::Ident(n) = ast.get_expr(*callee) else {
        return None;
    };
    if !checker.lookup(n).is_some_and(|info| info.builtin_mv) {
        return None;
    }
    let Ok(Type::Function(_, ret)) = checker.type_of(ast, *callee) else {
        return None;
    };
    for a in args {
        if let Err(e) = checker.type_of(ast, *a) {
            return Some(Err(e));
        }
    }
    Some(Ok(*ret))
}

/// A reified builtin-method value (RFC 20260725-str-method-value-
/// reify): a binding the let-decl marked `builtin_mv`, or the
/// inline `s.slice` Member form (receiver types to a builtin
/// prototype family, member types Function, name interns to a
/// builtin mid with a meta row).
fn is_builtin_mv_read(checker: &mut Checker, ast: &Ast, obj: ExprId) -> bool {
    match ast.get_expr(obj) {
        Expr::Ident(n) => checker.lookup(n).is_some_and(|info| info.builtin_mv),
        Expr::Member { obj: inner, name } => {
            let recv_ok = checker
                .type_of(ast, *inner)
                .is_ok_and(|t| crate::ssa_lower_member::mv_family_of_checker_ty(&t).is_some());
            if !recv_ok {
                return false;
            }
            let mid = torajs_rc::any_method_id(name);
            mid != torajs_rc::ANY_METHOD_UNKNOWN && torajs_rc::any_method_meta(mid).is_some()
        }
        _ => false,
    }
}

/// A builtin CONSTRUCTOR read as the receiver (`Number.bind(null)` /
/// `Promise.call(p, fn)`) — the bare namespace ident whose value
/// face is the interned ctor cell. Gated on the same proto-tag
/// table the ident lowering's `try_builtin_ctor_ident` uses (so
/// JSON / Math — no ctor cell — keep their no-member reject) and on
/// the name being unbound (a user binding owns it). The cell's
/// runtime dispatch runs the per-family ctor-as-function conversion
/// or raises the catchable TypeError — never a silent value.
fn is_builtin_ctor_read(checker: &mut Checker, ast: &Ast, obj: ExprId) -> bool {
    matches!(ast.get_expr(obj), Expr::Ident(n)
        if crate::ssa_lower_member_builtin_namespace::proto_method_tag(n).is_some()
            && checker.lookup(n).is_none())
}

/// A reified namespace-static read (`Array.from` / `Math.max` — the
/// intern-table truth the lowering bakes a cell for). Its
/// `.call/.apply/.bind` surface is any-dispatched (RFC
/// 20260808-construct-channel B6 刀 2): the cell's runtime dispatch
/// is spec-exact — recv-first ids read the thisArg, receiver-less
/// ids ignore it per their spec — while the legacy static sig this
/// arm preempts would reject the polymorphic forms.
fn is_ns_static_read(ast: &Ast, obj: ExprId) -> bool {
    matches!(ast.get_expr(obj), Expr::Member { obj: ns, name: m }
        if matches!(ast.get_expr(*ns), Expr::Ident(n)
            if torajs_rc::ns_static::ns_static_id(n, m) >= 0))
}

/// RFC 20260820-member-call-route 刀 1 — a class-instance METHOD
/// read (`a.m` where `m` is a method of the receiver's class or an
/// ancestor). The member read types Function (the class-method arm
/// strips `__this`), but it LOWERS to a runtime any cell — S2.34
/// boxes the receiver and resolves the reified class-method cell off
/// the prototype — so `.call/.apply/.bind` on it must any-dispatch:
/// only the runtime kernel can re-bind the thisArg (the detached
/// binding form already rides this lane). Fields never fire (only
/// `__cm_`-table methods count), so a fn-typed FIELD read keeps the
/// `try_fn_value_call` static replay below. Lowering mirror:
/// `ssa_lower_any_method_call`'s `class_method_value` gate.
fn is_class_method_read(checker: &mut Checker, ast: &Ast, obj: ExprId) -> bool {
    let Expr::Member {
        obj: inner,
        name: m,
    } = ast.get_expr(obj)
    else {
        return false;
    };
    let Ok(inner_ty) = checker.type_of(ast, *inner) else {
        return false;
    };
    let Some(mut cname) = crate::check_type_of_member_accessor::class_name_of(&inner_ty, ast)
    else {
        return false;
    };
    loop {
        if checker.globals.contains_key(&format!("__cm_{cname}__{m}"))
            || checker
                .globals
                .contains_key(&format!("__cm_gen_{cname}__{m}"))
        {
            return true;
        }
        match ast.class_parents.get(&cname) {
            Some(Some(p)) => cname = p.clone(),
            _ => return false,
        }
    }
}
