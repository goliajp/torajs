//! Dynamic-spread method-call demotion — the class-method half of
//! what [`super::spread_callee_wrap`] does for plain FnDecls.
//!
//! `desugar_classes` pass 2 rewrites `x.m(a)` into `__cm_<C>__m(x,
//! a)` purely by method NAME, before anything knows how many
//! arguments the site actually has. A spread in that argument list
//! makes the count a RUNTIME value, and the direct-call form cannot
//! spell one: the static expanders index-expand a spread against the
//! callee's declared arity (`apply_spread_args`) or pack a settled
//! tail into a rest param (`apply_rest_args`), and both decline the
//! shapes where neither is possible. The wrap pass would normally
//! hand such a callee to the runtime lane as a closure value, but a
//! `__cm_` callee cannot go that way — a forwarder's public face
//! drops the hidden `__this` and feeds it `undefined` (the rotation
//! 366 revert), so the receiver would be lost.
//!
//! The answer is to undo the rewrite instead of dressing it up: the
//! member-call shape it replaced is still in the arena (pass 2 clones
//! it into `Ast::speculative_cm_rewrites` for the `cm_demote`
//! decision), and restoring it puts the site back on the runtime
//! any-method spread lane (`__torajs_any_method_call_spread`), which
//! carries the receiver natively and reads argc off the materialized
//! argument array.
//!
//! Ordering: BEFORE the static expanders — the gate below asks
//! whether one of them will take the site, and stands aside when it
//! will, so no working direct call is pushed onto the slower lane.
//! Also BEFORE `desugar_arguments_object`, because several of its
//! collectors key on arena `Ident`s naming a fn (`collect_method_argv`
//! and `collect_named_static_argv` both read exactly the name this
//! pass removes), and they should read the final shape rather than one
//! this pass is about to change. It was not enough to give a demoted
//! class method its runtime `arguments` count — see below. AFTER
//! `materialize_expr_defaults`, whose output the default gate reads.
//!
//! `this` receivers stay out: pass 2 records them in
//! `cm_this_static_calls` instead (the cmany twin mint reads that
//! entry), and a `this.m(...xs)` site keeps the loud reject until
//! that account is settled.
//!
//! So does a body that reads `arguments`. The runtime lane knows the
//! true count — it is the length of the array it materializes — but
//! it does not reach a `__cm_` body: measured, a class method called
//! through this lane with a spread answers `arguments.length` 0,
//! while an object-literal method and a plain function on the same
//! lane both answer truthfully. (Where that count is lost is the
//! argv-face account, registered in plan-state L3b; a class method's
//! `arguments` works today only through the static face, which counts
//! a uniform direct-call site and declines a spread by construction.)
//! Demoting such a site would trade a loud reject for a wrong number
//! — the worse of the two. Those shapes keep the reject; what this
//! pass takes is the rest, which is ordinary JavaScript bun runs
//! today: two spreads, a non-trailing one, a computed source, or a
//! fixed prefix longer than the method's declared arity.

use super::apply_args::peel_hidden_params;
use super::arguments_object_walkers::body_has_any_arguments_touch;
use super::forwarders_object::snapshot_fn_sigs;
use super::spread_callee_wrap::static_expander_takes;
use super::{Ast, Expr, ExprId};

pub fn demote_dynamic_spread_method_calls(ast: &mut Ast) {
    if ast.speculative_cm_rewrites.is_empty() {
        return;
    }
    let (fn_sigs, _, _) = snapshot_fn_sigs(ast);
    let mut restores: Vec<(usize, ExprId)> = Vec::new();
    for i in 0..ast.exprs.len() {
        let Expr::Call { callee, args } = &ast.exprs[i] else {
            continue;
        };
        let Expr::Ident(name) = ast.get_expr(*callee) else {
            continue;
        };
        if !(name.starts_with("__cm_") || name.starts_with("__dispatch_")) {
            continue;
        }
        if !args
            .iter()
            .any(|a| matches!(ast.get_expr(*a), Expr::Spread { .. }))
        {
            continue;
        }
        let Some(&alt) = ast.speculative_cm_rewrites.get(&ExprId(i as u32)) else {
            continue;
        };
        // No signature means no way to ask the question — leave the
        // site exactly as it is.
        let Some((params, _, _)) = fn_sigs.get(name) else {
            continue;
        };
        // The rewritten arg list leads with the receiver and the
        // `__cm_` param row leads with `__this`, so the two line up
        // position for position and the shared gate reads them
        // directly.
        let user = peel_hidden_params(params);
        if static_expander_takes(ast, args, user) {
            continue;
        }
        // A default the CALL SITE still has to write in is one the
        // dynamic lane cannot replay (`apply_default_args` pads
        // direct calls only) — those sites keep the loud reject.
        // One `materialize_expr_defaults` already moved into the body
        // leaves the `undefined` pad behind it and replays for free.
        if user
            .iter()
            .any(|p| p.default.is_some_and(|d| !is_undefined(ast, d)))
        {
            continue;
        }
        // The count the runtime lane knows has no slot in a `__cm_`
        // body — see the module doc's argv-face account.
        if body_reads_arguments(ast, name) {
            continue;
        }
        restores.push((i, alt));
    }
    for (i, alt) in restores {
        ast.exprs[i] = ast.exprs[alt.0 as usize].clone();
        ast.speculative_cm_rewrites.remove(&ExprId(i as u32));
    }
}

/// Whether the named FnDecl's body reads its `arguments` object at
/// all — length, index, or whole-object escape.
fn body_reads_arguments(ast: &Ast, name: &str) -> bool {
    ast.stmts.iter().any(|s| match s {
        super::Stmt::FnDecl { name: n, body, .. } if n == name => {
            body_has_any_arguments_touch(ast, body)
        }
        _ => false,
    })
}

/// The pad `materialize_expr_defaults` leaves where a default used to
/// be spelled.
fn is_undefined(ast: &Ast, d: ExprId) -> bool {
    matches!(ast.get_expr(d), Expr::Ident(n) if n == "undefined")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Param, Stmt};

    fn fn_decl(name: &str, params: Vec<Param>, body: Vec<Stmt>) -> Stmt {
        Stmt::FnDecl {
            name: name.into(),
            type_params: Vec::new(),
            params,
            return_type: None,
            body,
            is_generator: false,
            span: crate::lexer::Span { start: 0, end: 0 },
        }
    }

    /// A body that never spells `arguments` is one the runtime lane
    /// can take.
    #[test]
    fn a_body_that_never_names_arguments_is_free_to_move() {
        let mut ast = Ast::default();
        let n = ast.add_expr(Expr::Number(1.0));
        ast.stmts.push(fn_decl(
            "__cm_C__m",
            vec![Param {
                name: "__this".into(),
                type_ann: None,
                default: None,
                is_rest: false,
            }],
            vec![Stmt::Expr(n)],
        ));
        assert!(!body_reads_arguments(&ast, "__cm_C__m"));
    }

    /// One that does keeps the loud reject — the count it would need
    /// has no slot in a `__cm_` body.
    #[test]
    fn a_body_that_reads_arguments_stays_put() {
        let mut ast = Ast::default();
        let a = ast.add_expr(Expr::Ident("arguments".into()));
        let len = ast.add_expr(Expr::Member {
            obj: a,
            name: "length".into(),
        });
        ast.stmts
            .push(fn_decl("__cm_C__m", Vec::new(), vec![Stmt::Expr(len)]));
        assert!(body_reads_arguments(&ast, "__cm_C__m"));
    }

    /// An unknown name answers no — a site with no signature to read
    /// is already declined a step earlier.
    #[test]
    fn a_name_with_no_declaration_reads_nothing() {
        let ast = Ast::default();
        assert!(!body_reads_arguments(&ast, "__cm_C__m"));
    }

    #[test]
    fn the_pad_left_behind_by_a_moved_default_is_not_a_default() {
        let mut ast = Ast::default();
        let undef = ast.add_expr(Expr::Ident("undefined".into()));
        let five = ast.add_expr(Expr::Number(5.0));
        assert!(is_undefined(&ast, undef));
        assert!(!is_undefined(&ast, five));
    }
}
