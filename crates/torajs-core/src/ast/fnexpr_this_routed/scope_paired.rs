//! 399-03 — the multiply-declared path of [`super`], split out at the
//! file-size line.

use crate::ast::fnexpr_this_faces::FacePatch;
use crate::ast::fnexpr_this_names::{name_shadowed_elsewhere, peel_as};
use crate::ast::fnexpr_this_pairing::pair_decls_scoped;
use crate::ast::fnexpr_this_shapes::UseShapes;
use crate::ast::{Expr, ExprId, Stmt};

/// 399-03 — the multiply-declared path: run knife 2's full
/// per-binding proof on every scope-paired declaration group, and
/// promote ALL of them or none.
///
/// All-or-nothing is load-bearing, not caution: the downstream
/// consumers (`fnexpr_recv_locals`, the direct-call `undefined`
/// seeding in `ssa_lower_call_closure_local`) are name-keyed, so a
/// partially promoted name would shift argv on call sites whose
/// runtime value is an unpromoted closure — the exact silent wrong
/// the zero-alias bar forbids. Each group's proof is the single-decl
/// path's, verbatim in spirit: init is a marked fn-expr still
/// carrying `__this`, every non-face use admits a receiver-safe
/// shape, and a group with a call face or a direct call stays off
/// the boxed-argv lanes. The group count must match the by-name
/// census (`collect_decls_split` mono + twin) — a mismatch means the
/// pairing walk missed a declaration the flat walk can see, and the
/// name keeps the loud reject.
#[allow(clippy::too_many_arguments)]
pub(super) fn try_promote_scope_paired(
    stmts: &[Stmt],
    exprs: &[Expr],
    fn_expr_exprs: &std::collections::HashSet<ExprId>,
    closure_argc_locals: &std::collections::HashSet<String>,
    closure_argv_locals: &std::collections::HashSet<String>,
    name: &str,
    face_eids: &[ExprId],
    shapes: &UseShapes,
    call_faces: &std::collections::HashSet<ExprId>,
    expected_decls: usize,
    patches: &mut Vec<FacePatch>,
    fnexpr_recv_faces: &mut std::collections::HashSet<ExprId>,
    fnexpr_recv_locals: &mut std::collections::HashSet<String>,
) {
    // A non-LetDecl shadow (fn param / catch / loop var) still voids
    // the name outright — the pairing walk assumes those are absent.
    if name_shadowed_elsewhere(stmts, name) {
        return;
    }
    let Some(groups) = pair_decls_scoped(stmts, exprs, name) else {
        return;
    };
    if groups.len() != expected_decls {
        return;
    }
    let mut plans: Vec<(ExprId, String, Vec<ExprId>)> = Vec::new();
    for g in &groups {
        let init = peel_as(exprs, g.init);
        if !fn_expr_exprs.contains(&init) {
            return;
        }
        let Expr::Closure { fn_name, captures } = &exprs[init.0 as usize] else {
            return;
        };
        if !captures.iter().any(|c| c == "__this") {
            return;
        }
        let g_faces: Vec<ExprId> = g
            .uses
            .iter()
            .filter(|e| face_eids.contains(e))
            .copied()
            .collect();
        if !g
            .uses
            .iter()
            .filter(|e| !g_faces.contains(e))
            .all(|e| shapes.admits(*e, name))
        {
            return;
        }
        // A WRITE to any group's binding keeps the whole name loud:
        // this path ends by registering the name on
        // `fnexpr_recv_locals` unconditionally (all-or-nothing is
        // load-bearing here), and that static list is exactly what a
        // rebindable binding must stay off of.
        if g.uses.iter().any(|e| shapes.assign_target.contains(e)) {
            return;
        }
        // The boxed-argv bar, per group — same reasoning as the
        // single-decl path's.
        if (g.uses.iter().any(|e| shapes.callee.contains(e))
            || g_faces.iter().any(|e| call_faces.contains(e)))
            && (closure_argc_locals.contains(name) || closure_argv_locals.contains(name))
        {
            return;
        }
        plans.push((init, fn_name.clone(), g_faces));
    }
    // a by-name alias cannot tell which declaration it reads
    if !crate::ast::fnexpr_this_alias::called_alias_names(stmts, exprs, shapes, name).is_empty() {
        return;
    }
    for (init, fn_name, g_faces) in plans {
        patches.push(FacePatch { eid: init, fn_name });
        fnexpr_recv_faces.extend(g_faces);
    }
    fnexpr_recv_locals.insert(name.to_string());
}
