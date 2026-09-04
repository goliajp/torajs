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
//! Restoring also has to CLEAR the mangled `Ident` node pass 2 minted
//! for the call it is undoing. The node is now orphaned — nothing
//! reaches it — but the arguments-object collectors read the arena as
//! evidence: an `Ident` naming a `__cm_` fn means some direct call
//! still speaks the old signature, so the method-argv face refuses to
//! reshape it. Leaving the orphan there made a demoted method answer
//! `arguments.length` 0 while the runtime lane was holding the true
//! count all along.
//!
//! Ordering: BEFORE the static expanders — the gate below asks
//! whether one of them will take the site, and stands aside when it
//! will, so no working direct call is pushed onto the slower lane.
//! Also BEFORE `desugar_arguments_object`, because several of its
//! collectors key on arena `Ident`s naming a fn (`collect_method_argv`
//! and `collect_named_static_argv` both read exactly the name this
//! pass removes), and they should read the final shape rather than one
//! this pass is about to change. AFTER `materialize_expr_defaults`,
//! whose output the default gate reads.
//!
//! `this` receivers stay out: pass 2 records them in
//! `cm_this_static_calls` instead (the cmany twin mint reads that
//! entry), and a `this.m(...xs)` site keeps the loud reject until
//! that account is settled.
//!
//! A body that READS `arguments` is not offered to the static
//! expanders at all: `apply_spread_args` index-expands a spread to the
//! callee's declared arity, so the count dies with the trimmed tail
//! (`c.m(...[1, 2, 3])` on `m(a)` answered `arguments.length` 1 —
//! silently, since nothing rejected). It needs one more thing before
//! it can move: the runtime count reaches a `__cm_` body only through
//! the method-argv face, and the face has conditions of its own. So
//! such a site is demoted only when the face will then take the
//! method — every condition asked through the collector's own
//! predicates (`decl_admits`, `old_signature_lane`) rather than a
//! second copy of them, plus the two this pass is uniquely placed to
//! answer: that it is removing EVERY arena `Ident` naming the fn, and
//! that a spread among those sites already kept the constant-fold
//! tier out (`uniform_direct_call_argc` answers `None` on a spread by
//! construction). Where the face will not take it, the site stays as
//! it is rather than trading its answer for a worse one.

use super::apply_args::peel_hidden_params;
use super::arguments_object_method_argv::{decl_admits, old_signature_lane};
use super::arguments_object_walkers::{
    body_has_any_arguments_touch, body_has_bare_arguments_assign,
};
use super::forwarders_object::snapshot_fn_sigs;
use super::spread_callee_wrap::static_expander_takes;
use super::{Ast, Expr, ExprId, Stmt};

pub fn demote_dynamic_spread_method_calls(ast: &mut Ast) {
    if ast.speculative_cm_rewrites.is_empty() {
        return;
    }
    let (fn_sigs, _, _) = snapshot_fn_sigs(ast);
    // (call node, restored shape, callee node to clear, callee name)
    let mut restores: Vec<(usize, ExprId, usize, String)> = Vec::new();
    for i in 0..ast.exprs.len() {
        let Expr::Call { callee, args } = &ast.exprs[i] else {
            continue;
        };
        let callee = *callee;
        let Expr::Ident(name) = ast.get_expr(callee) else {
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
        // A body that reads `arguments` is one no static expander can
        // answer for: `apply_spread_args` index-expands the spread to
        // the callee's declared arity, and the count dies with the
        // trimmed tail (`c.m(...[1,2,3])` on `m(a)` answered
        // `arguments.length` 1). So the expander's claim is not
        // consulted for those sites — the face decides them below.
        if !body_reads_arguments(ast, name) && static_expander_takes(ast, args, user) {
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
        restores.push((i, alt, callee.0 as usize, name.clone()));
    }
    // A body that reads `arguments` only moves if the method-argv
    // face will then carry the count to it — see the module doc.
    let old_sig_lane = old_signature_lane(ast);
    let refused: std::collections::HashSet<String> = restores
        .iter()
        .map(|(_, _, _, n)| n)
        .filter(|n| {
            body_reads_arguments(ast, n) && !face_will_admit(ast, n, &old_sig_lane, &restores)
        })
        .cloned()
        .collect();
    restores.retain(|(_, _, _, name)| !refused.contains(name));
    for (i, alt, callee, _) in restores {
        ast.exprs[callee] = Expr::Ident("undefined".into());
        ast.exprs[i] = ast.exprs[alt.0 as usize].clone();
        ast.speculative_cm_rewrites.remove(&ExprId(i as u32));
    }
}

/// Whether `collect_method_argv` will take `name` once this pass has
/// removed the callee `Ident`s in `restores`. The declaration and
/// class-table conditions are the collector's own; the arena
/// condition is the one only this pass can answer, since it is the
/// one it is about to change.
fn face_will_admit(
    ast: &Ast,
    name: &str,
    old_sig_lane: &std::collections::HashSet<String>,
    restores: &[(usize, ExprId, usize, String)],
) -> bool {
    if old_sig_lane.contains(name) {
        return false;
    }
    let decl_ok = ast.stmts.iter().any(|s| match s {
        Stmt::FnDecl {
            name: n,
            params,
            body,
            ..
        } if n == name => {
            decl_admits(ast, params, body) && !body_has_bare_arguments_assign(ast, body)
        }
        _ => false,
    });
    if !decl_ok {
        return false;
    }
    // Every arena `Ident` spelling the name has to be one of the
    // callee nodes about to be cleared; one left standing keeps the
    // collector's arena gate shut and the count would never arrive.
    let cleared: std::collections::HashSet<usize> = restores
        .iter()
        .filter(|(_, _, _, n)| n == name)
        .map(|(_, _, c, _)| *c)
        .collect();
    ast.exprs
        .iter()
        .enumerate()
        .all(|(i, e)| !matches!(e, Expr::Ident(n) if n == name) || cleared.contains(&i))
}

/// Whether the named FnDecl's body reads its `arguments` object at
/// all — length, index, or whole-object escape.
fn body_reads_arguments(ast: &Ast, name: &str) -> bool {
    ast.stmts.iter().any(|s| match s {
        Stmt::FnDecl { name: n, body, .. } if n == name => body_has_any_arguments_touch(ast, body),
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

    /// A name the `__dispatch_` / vtable lanes can reach is refused
    /// outright — those call by the old signature and leave no arena
    /// `Ident` for the check below to find.
    #[test]
    fn a_name_on_the_old_signature_lane_never_earns_the_face() {
        let mut ast = Ast::default();
        let mut lane = std::collections::HashSet::new();
        lane.insert("__cm_C__m".to_string());
        ast.stmts.push(fn_decl("__cm_C__m", Vec::new(), Vec::new()));
        assert!(!face_will_admit(&ast, "__cm_C__m", &lane, &[]));
    }

    /// An `Ident` this pass is not clearing keeps the collector's
    /// arena gate shut, so the count would never arrive.
    #[test]
    fn one_ident_left_standing_is_enough_to_refuse() {
        let mut ast = Ast::default();
        let a = ast.add_expr(Expr::Ident("arguments".into()));
        let len = ast.add_expr(Expr::Member {
            obj: a,
            name: "length".into(),
        });
        let this = Param {
            name: "__this".into(),
            type_ann: None,
            default: None,
            is_rest: false,
        };
        ast.stmts
            .push(fn_decl("__cm_C__m", vec![this], vec![Stmt::Expr(len)]));
        let lane = std::collections::HashSet::new();
        // no reference at all — the face takes it
        assert!(face_will_admit(&ast, "__cm_C__m", &lane, &[]));
        let callee = ast.add_expr(Expr::Ident("__cm_C__m".into()));
        // …and stops taking it once one stands unclaimed
        assert!(!face_will_admit(&ast, "__cm_C__m", &lane, &[]));
        // claiming that very node brings it back
        let claimed = [(
            0usize,
            ExprId(0),
            callee.0 as usize,
            "__cm_C__m".to_string(),
        )];
        assert!(face_will_admit(&ast, "__cm_C__m", &lane, &claimed));
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
