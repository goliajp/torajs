//! The CLASS-member-return face — a marked fn-expr returned from a
//! flattened class member body — plus the 399-01 annotation seeding
//! that lets the bare (unannotated) spelling join it. Split out of
//! [`super::fnexpr_this_faces`] (rotation 400 file-size split; that
//! file stood at 482 of the 500 line limit). Bodies verbatim except
//! where noted.

use super::fnexpr_this_faces::{FacePatch, collect_face};
use super::{Expr, ExprId, Stmt};

/// 399-01 — a class member with NO return annotation whose every
/// value-return is a marked fn-expr still carrying `__this` gets its
/// return annotation seeded to `any`, BEFORE the face walk runs.
///
/// The face collector below demands an annotation spelled exactly
/// `any`, and rotation 399 measured why merely admitting the absent
/// spelling is wrong: an absent annotation is inferred FROM THE
/// RETURNED EXPRESSION, a function expression infers a concrete
/// closure type, and the caller then holds a typed callee and calls
/// it down the lane that does not shift argv — `cannot read
/// properties of null or undefined` where the unpromoted spelling
/// merely answered the wrong object. Seeding the annotation itself
/// closes that hole at its root: the FnDecl's `return_type` is the
/// single source every downstream reader consumes (the checker's
/// `__cm_*` globals entry, the SSA `effective_ret_ty` — which passes
/// `any` through untouched), so the caller now holds an `any` callee
/// and every call path to the promoted body rides the argv-shifting
/// any lane.
///
/// The bar is ALL value-returns, not ANY: a body mixing a `__this`
/// fn-expr return with any other return shape keeps today's behavior
/// (the silent-wrong already registered as 399-01's residue) rather
/// than widening the other paths' types.
pub(super) fn seed_bare_member_return_ann(
    stmts: &mut [Stmt],
    exprs: &[Expr],
    fn_expr_exprs: &std::collections::HashSet<ExprId>,
) {
    for s in stmts.iter_mut() {
        let Stmt::FnDecl {
            name,
            return_type,
            body,
            ..
        } = s
        else {
            continue;
        };
        if !is_class_member_fn(name) || return_type.is_some() {
            continue;
        }
        let mut rets = Vec::new();
        body_value_returns(body, &mut rets);
        if rets.is_empty() {
            continue;
        }
        let all_marked_this = rets.iter().all(|e| {
            fn_expr_exprs.contains(e)
                && matches!(&exprs[e.0 as usize], Expr::Closure { captures, .. }
                    if captures.iter().any(|c| c == "__this"))
        });
        if all_marked_this {
            *return_type = Some("any".to_string());
        }
    }
}

/// Every value-return `ExprId` in the member's own body — same walk
/// spine as [`collect_return_faces_in`] (nested compound statements
/// walk through, a nested `function` declaration is its own return
/// scope and is skipped), so the seeding bar judges exactly the
/// returns the face walk will collect.
fn body_value_returns(body: &[Stmt], out: &mut Vec<ExprId>) {
    for s in body {
        if let Stmt::Return(Some(e)) = s {
            out.push(*e);
        }
        if matches!(s, Stmt::FnDecl { .. }) {
            continue;
        }
        super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| {
            body_value_returns(inner, out)
        });
    }
}

/// The CLASS-method half of the objlit method-return position: a
/// marked fn-expr returned from a class member body whose return
/// annotation is spelled exactly `any` — spelled by the program, or
/// seeded by [`seed_bare_member_return_ann`] just above.
///
/// `desugar_classes` flattens each member into a top-level FnDecl
/// (`__cm_<C>__<m>`, plus the receiver-polymorphic `__cmany_` /
/// `__smany_` clones and the `__sm_` statics), so the objlit
/// collector — which recognizes its methods by the lifted closure
/// names `objlit_nominal` published — never saw one. The value leaves
/// through the same channel; what differs is that a class member
/// states its return type, so the crossing has to be checked rather
/// than assumed.
///
/// The proof is [`super::fnexpr_this_shapes`]'s any-lane one: the cell
/// escapes, so it must cross into the any lane and STAY there, because
/// every any-lane call path shifts argv on FLAG_CLOSURE_RECV_FIRST
/// while a typed indirect call does not. Only an annotation reading
/// exactly `any` proves that crossing — which is why the seeding pass
/// writes the annotation instead of this collector admitting its
/// absence (rotation 399 measured the admit-absent version wrong; doc
/// on the seeder).
///
/// The constructor is excluded: `__cm_<C>__ctor` answers the instance
/// through the construct channel, not a value the caller reads.
///
/// Without this, `class M { run(): any { return function () { …this… } } }`
/// handed the returned function the METHOD's receiver — the same
/// arrow-semantics answer the object-literal spelling used to give, and
/// one the method's own receiver only satisfies by coincidence.
pub(super) fn collect_class_method_return_faces(
    stmts: &[Stmt],
    exprs: &[Expr],
    fn_expr_exprs: &std::collections::HashSet<ExprId>,
    patches: &mut Vec<FacePatch>,
) {
    for s in stmts {
        let Stmt::FnDecl {
            name,
            return_type,
            body,
            ..
        } = s
        else {
            continue;
        };
        if !is_class_member_fn(name) || return_type.as_deref() != Some("any") {
            continue;
        }
        collect_return_faces_in(body, stmts, exprs, fn_expr_exprs, patches);
    }
}

/// The flattened class-member FnDecl names, constructor excluded. All
/// four prefixes are synthesized — no source can spell one — the same
/// test the boxed-adapter synthesis reads them by.
fn is_class_member_fn(name: &str) -> bool {
    let member = name.starts_with("__cm_")
        || name.starts_with("__cmany_")
        || name.starts_with("__sm_")
        || name.starts_with("__smany_");
    member && !name.ends_with("__ctor")
}

/// Every `return <marked fn-expr>` in the METHOD's own body — nested
/// compound statements walk through the shared spine, but a nested
/// `function` declaration is its own return scope and is skipped.
/// Shared with the objlit method-return collector in
/// [`super::fnexpr_this_faces`].
pub(super) fn collect_return_faces_in(
    body: &[Stmt],
    all_stmts: &[Stmt],
    exprs: &[Expr],
    fn_expr_exprs: &std::collections::HashSet<ExprId>,
    patches: &mut Vec<FacePatch>,
) {
    for s in body {
        if let Stmt::Return(Some(e)) = s {
            collect_face(all_stmts, exprs, *e, fn_expr_exprs, patches);
        }
        if matches!(s, Stmt::FnDecl { .. }) {
            continue;
        }
        super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| {
            collect_return_faces_in(inner, all_stmts, exprs, fn_expr_exprs, patches)
        });
    }
}
