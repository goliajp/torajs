//! Name-census helpers for the [`super::fnexpr_this`] knives — the
//! program-wide BY-NAME walks (declaration counting, shadow
//! detection, `as`-peeling) that knife 2's zero-alias proofs and the
//! named-fn callback mirror share. Split out of
//! [`super::fnexpr_this_recvs`] (rotation 400 file-size split): that
//! sibling answers "which bindings are receivers of shape X", this
//! one answers "how is this NAME declared across the program".
//! Bodies verbatim.
//!
//! 399-04 lives here: the `__cmany_` / `__smany_` twin clones make
//! every by-name census in this file count a method-body declaration
//! twice — `promote_variable_routed`'s decl census compensates
//! (rotation 399), the rest do not yet.

use super::{Expr, Stmt};

/// Is this the name of a receiver-polymorphic clone?
///
/// Both prefixes are synthesized — no source can spell one — which is
/// the same test the boxed-adapter synthesis and the module-metadata
/// method walk already read them by. Every by-name census that COUNTS
/// declarations must treat a declaration inside one of these bodies
/// as a copy of its mono-body source, not a second declaration
/// (399-04; the walk pattern is `collect_decls_split` in
/// [`super::fnexpr_this_routed`]).
pub(super) fn is_twin_body_name(name: &str) -> bool {
    name.starts_with("__cmany_") || name.starts_with("__smany_")
}

/// Rotation 328 — every binding whose init is a marked fn-expr
/// Closure still carrying `__this` in its capture list. These are the
/// zero-face candidates for the all-direct-call profile: a name never
/// standing in any face position can still promote when its every use
/// is a direct-call callee (the `closure_local` lane seeds a boxed
/// `undefined` into the promoted `__this` slot — §10.2.1.2 strict-mode
/// call-site `this`, the same framing as `this_param.rs` blade 1).
/// Recursion set mirrors [`collect_decls_by_name`]: a decl this walk
/// finds in a position that walk cannot re-find fails its
/// `decls.len() == 1` guard and stays loud — never mis-paired.
pub(super) fn collect_this_fnexpr_decl_names(
    stmts: &[Stmt],
    exprs: &[Expr],
    fn_expr_exprs: &std::collections::HashSet<super::ExprId>,
    out: &mut Vec<String>,
) {
    for s in stmts {
        if let Stmt::LetDecl { name, init, .. } = s {
            let init = peel_as(exprs, *init);
            if fn_expr_exprs.contains(&init)
                && matches!(&exprs[init.0 as usize], Expr::Closure { captures, .. }
                    if captures.iter().any(|c| c == "__this"))
            {
                out.push(name.clone());
            }
        }
        super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| {
            collect_this_fnexpr_decl_names(inner, exprs, fn_expr_exprs, out)
        });
    }
}

/// See past an `as` suffix to the value it ascribes.
///
/// `const K = function () { …this… } as any` and `const K: any =
/// function () { …this… }` declare the same thing, but only the
/// second one used to promote: every test here asks whether the
/// init IS the marked fn-expr, and the suffix spelling hands over an
/// `Expr::As` wrapping it. The use side has the same shape —
/// `(K as any).s()` hides the Ident from the member-object test —
/// and TS asks for that spelling whenever the member is not on the
/// declared type.
pub(super) fn peel_as(exprs: &[Expr], mut e: super::ExprId) -> super::ExprId {
    while let Expr::As { expr, .. } = &exprs[e.0 as usize] {
        e = *expr;
    }
    e
}

/// Does the lifted FnDecl named `fn_name` declare a rest param
/// (`...args`)? Rest-tail closures dispatch through the boxed
/// variadic entry, which materializes params off argv — cut 2's
/// mixed promotion excludes them.
pub(super) fn fn_has_rest_param(stmts: &[Stmt], fn_name: &str) -> bool {
    stmts.iter().any(|s| {
        matches!(s, Stmt::FnDecl { name, params, .. }
            if name == fn_name && params.iter().any(|p| p.is_rest))
    })
}

/// Every `LetDecl` matching `name`, over the full
/// [`for_each_nested_list`] spine — knife 2's uniqueness guard needs
/// the program-wide count, and it must re-find every decl the
/// candidate walk above can find (same spine = never mis-paired).
pub(super) fn collect_decls_by_name(
    stmts: &[Stmt],
    name: &str,
    out: &mut Vec<(bool, super::ExprId)>,
) {
    for s in stmts {
        if let Stmt::LetDecl {
            mutable,
            name: dn,
            init,
            ..
        } = s
        {
            if dn == name {
                out.push((*mutable, *init));
            }
        }
        super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| {
            collect_decls_by_name(inner, name, out)
        });
    }
}

/// Does any NON-LetDecl declaration site rebind `name` — an fn param
/// (incl. lifted closure params), a `catch (name)`, or a for-of /
/// for-in loop variable? Those scopes' `name` reads are different
/// bindings, but the knife-2 use-vs-face parity walks Ident nodes
/// program-wide by NAME, so a shadowed name cannot be paired to its
/// binding syntactically — a mispair would stamp RECV on a face whose
/// runtime value is the shadow. Shadowed names keep the loud reject
/// (same bar as the `decls.len() != 1` guard, which only sees
/// LetDecls).
pub(super) fn name_shadowed_elsewhere(stmts: &[Stmt], name: &str) -> bool {
    for s in stmts {
        let hit = match s {
            Stmt::FnDecl { params, body, .. } => {
                params.iter().any(|p| p.name == name) || name_shadowed_elsewhere(body, name)
            }
            Stmt::Try {
                body,
                catch_param,
                catch_body,
                finally_body,
                ..
            } => {
                catch_param.as_deref() == Some(name)
                    || name_shadowed_elsewhere(body, name)
                    || name_shadowed_elsewhere(catch_body, name)
                    || finally_body
                        .as_ref()
                        .is_some_and(|f| name_shadowed_elsewhere(f, name))
            }
            Stmt::ForOf { var_name, body, .. } | Stmt::ForOfSplitIter { var_name, body, .. } => {
                var_name == name || name_shadowed_elsewhere(std::slice::from_ref(body), name)
            }
            Stmt::For { init, body, .. } => {
                init.as_ref()
                    .is_some_and(|i| name_shadowed_elsewhere(std::slice::from_ref(i), name))
                    || name_shadowed_elsewhere(std::slice::from_ref(body), name)
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Labeled { body, .. } => {
                name_shadowed_elsewhere(std::slice::from_ref(body), name)
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                name_shadowed_elsewhere(std::slice::from_ref(then_branch), name)
                    || else_branch
                        .as_ref()
                        .is_some_and(|e| name_shadowed_elsewhere(std::slice::from_ref(e), name))
            }
            Stmt::Switch { cases, default, .. } => {
                cases.iter().any(|c| name_shadowed_elsewhere(&c.body, name))
                    || default
                        .as_ref()
                        .is_some_and(|d| name_shadowed_elsewhere(d, name))
            }
            Stmt::Block(inner) | Stmt::Multi(inner) => name_shadowed_elsewhere(inner, name),
            _ => false,
        };
        if hit {
            return true;
        }
    }
    false
}
