//! 399-03 — the multiply-declared path of [`super`], split out at the
//! file-size line.

use crate::ast::fnexpr_this_faces::FacePatch;
use crate::ast::fnexpr_this_names::{name_shadowed_by_non_param, peel_as};
use crate::ast::fnexpr_this_pairing::{DeclGroup, pair_decls_scoped};
use crate::ast::fnexpr_this_shapes::UseShapes;
use crate::ast::{Ast, Expr, ExprId, Stmt};

impl Ast {
    /// Is `eid` a use of a promoted fn-expr binding — an Ident whose
    /// name is in `fnexpr_recv_locals` and which binds to a promoted
    /// declaration, not to another same-name one?
    pub fn is_fnexpr_recv_use(&self, eid: ExprId) -> bool {
        matches!(self.get_expr(eid), Expr::Ident(n) if self.fnexpr_recv_locals.contains(n))
            && !self.fnexpr_recv_foreign_uses.contains(&eid)
    }
}

/// What the census knows about one multiply-declared name.
pub(super) struct ScopePaired<'a> {
    pub(super) stmts: &'a [Stmt],
    pub(super) exprs: &'a [Expr],
    pub(super) fn_expr_exprs: &'a std::collections::HashSet<ExprId>,
    pub(super) closure_argc_locals: &'a std::collections::HashSet<String>,
    pub(super) closure_argv_locals: &'a std::collections::HashSet<String>,
    pub(super) name: &'a str,
    pub(super) face_eids: &'a [ExprId],
    /// Every `Ident(name)` node in the arena.
    pub(super) use_eids: &'a [ExprId],
    /// The program-wide use checks passed: every non-face use admits a
    /// receiver-safe shape, and a written name has no face.
    pub(super) flat_ok: bool,
    pub(super) shapes: &'a UseShapes,
    pub(super) call_faces: &'a std::collections::HashSet<ExprId>,
    /// Declarations the by-name census counts (mono + twin).
    pub(super) expected_decls: usize,
}

/// Run knife 2's full per-binding proof on every scope-paired
/// declaration group whose init is a marked fn-expr reading `this`,
/// and promote ALL of them or none.
///
/// All-or-nothing over the candidates is load-bearing, not caution:
/// the downstream consumers read the name through
/// `Ast::is_fnexpr_recv_use`, so a candidate left unpromoted would
/// shift argv on call sites whose runtime value is its unpromoted
/// closure. Each group's proof is the single-decl path's: every
/// non-face use admits a receiver-safe shape, the binding is never
/// written, and a group with a call face or a direct call stays off
/// the boxed-argv lanes. The group count must match the by-name
/// census — a mismatch means the pairing walk missed a declaration
/// the flat walk can see, and the name keeps the loud reject.
///
/// A group whose init is anything else (`const g = 1` in another
/// function) is a different binding that happens to share the name.
/// It needs no proof, but every one of its uses joins
/// `fnexpr_recv_foreign_uses` so no consumer treats it as promoted.
/// That only holds if the pairing saw every use: an `Ident(name)` the
/// statement walk did not reach (a node only a side table points at)
/// would read as a promoted use, so with such a group present every
/// Ident of the name must belong to some group. Without one, the
/// program-wide checks still stand in for the candidates as before.
pub(super) fn try_promote_scope_paired(
    c: ScopePaired<'_>,
    patches: &mut Vec<FacePatch>,
    fnexpr_recv_faces: &mut std::collections::HashSet<ExprId>,
    fnexpr_recv_locals: &mut std::collections::HashSet<String>,
    fnexpr_recv_foreign_uses: &mut std::collections::HashSet<ExprId>,
) {
    let name = c.name;
    // A catch or loop-variable binding still voids the name outright —
    // the pairing walk assumes those are absent. A parameter is a
    // group of its own.
    if name_shadowed_by_non_param(c.stmts, name) {
        return;
    }
    let Some(groups) = pair_decls_scoped(c.stmts, c.exprs, name) else {
        return;
    };
    if groups.iter().filter(|g| g.init.is_some()).count() != c.expected_decls {
        return;
    }
    let mut plans: Vec<(ExprId, String, Vec<ExprId>)> = Vec::new();
    let mut foreign: Vec<ExprId> = Vec::new();
    for g in &groups {
        let Some((init, fn_name)) = candidate(&c, g) else {
            foreign.extend(g.uses.iter().copied());
            continue;
        };
        let Some(g_faces) = prove(&c, g) else {
            return;
        };
        plans.push((init, fn_name, g_faces));
    }
    if plans.is_empty() {
        return;
    }
    // Decided by whether another binding EXISTS, not by whether the
    // walk met a use of it: a use the walk never reached (a parameter
    // default) would otherwise read as promoted.
    if plans.len() == groups.len() {
        if !c.flat_ok {
            return;
        }
    } else if !c
        .use_eids
        .iter()
        .all(|e| groups.iter().any(|g| g.uses.contains(e)))
    {
        return;
    }
    // A top-level candidate next to a same-name binding that is not
    // one is read through a global whose closure type does not match
    // the promoted body (`g(1)` at top level passed a boxed argument to
    // a typed parameter), so the name keeps the loud reject.
    if plans.len() != groups.len()
        && plans
            .iter()
            .any(|(init, ..)| declared_at_top(c.stmts, c.exprs, name, *init))
    {
        return;
    }
    // a by-name alias cannot tell which declaration it reads
    if !crate::ast::fnexpr_this_alias::called_alias_names(c.stmts, c.exprs, c.shapes, name)
        .is_empty()
    {
        return;
    }
    for (init, fn_name, g_faces) in plans {
        patches.push(FacePatch { eid: init, fn_name });
        fnexpr_recv_faces.extend(g_faces);
    }
    fnexpr_recv_locals.insert(name.to_string());
    fnexpr_recv_foreign_uses.extend(foreign);
}

/// Is `init` the (peeled) initializer of a top-level declaration of
/// `name` (through `Multi`, which shares the top-level scope)?
fn declared_at_top(stmts: &[Stmt], exprs: &[Expr], name: &str, init: ExprId) -> bool {
    stmts.iter().any(|s| match s {
        Stmt::LetDecl {
            name: n, init: i, ..
        } => n == name && peel_as(exprs, *i) == init,
        Stmt::Multi(inner) => declared_at_top(inner, exprs, name, init),
        _ => false,
    })
}

/// The group's init, when it is a marked fn-expr still capturing
/// `__this` — the only kind of binding knife 2 promotes.
fn candidate(c: &ScopePaired<'_>, g: &DeclGroup) -> Option<(ExprId, String)> {
    let init = peel_as(c.exprs, g.init?);
    if !c.fn_expr_exprs.contains(&init) {
        return None;
    }
    match &c.exprs[init.0 as usize] {
        Expr::Closure { fn_name, captures } if captures.iter().any(|x| x == "__this") => {
            Some((init, fn_name.clone()))
        }
        _ => None,
    }
}

/// Knife 2's per-binding proof for one candidate group; answers its
/// face uses.
fn prove(c: &ScopePaired<'_>, g: &DeclGroup) -> Option<Vec<ExprId>> {
    let name = c.name;
    let g_faces: Vec<ExprId> = g
        .uses
        .iter()
        .filter(|e| c.face_eids.contains(e))
        .copied()
        .collect();
    if !g
        .uses
        .iter()
        .filter(|e| !g_faces.contains(e))
        .all(|e| c.shapes.admits(*e, name))
    {
        return None;
    }
    // A WRITE to a candidate's binding keeps the whole name loud: the
    // name lands on `fnexpr_recv_locals`, the static list a rebindable
    // binding must stay off of.
    if g.uses.iter().any(|e| c.shapes.assign_target.contains(e)) {
        return None;
    }
    // The boxed-argv bar, per group — same reasoning as the
    // single-decl path's.
    if (g.uses.iter().any(|e| c.shapes.callee.contains(e))
        || g_faces.iter().any(|e| c.call_faces.contains(e)))
        && (c.closure_argc_locals.contains(name) || c.closure_argv_locals.contains(name))
    {
        return None;
    }
    Some(g_faces)
}
