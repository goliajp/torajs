//! RFC 20260807-global-object G1 — `globalThis.X` member READS of a
//! known builtin global rewrite to the bare name `X`.
//!
//! tr has no runtime global object (S5.6); `globalThis` resolves only
//! in `typeof` position (a static `"object"`). But a member read of a
//! KNOWN builtin through it is compile-time decidable: §9.3's global
//! object exposes exactly the builtins the bare name already resolves
//! to, and bun (module goal) answers `globalThis.Array === Array`
//! true. The rewrite makes `globalThis.Math.max(…)` /
//! `var A = globalThis.Array` work with zero runtime surface.
//!
//! READ positions only. A store (`globalThis.Array = x`), a `delete`,
//! or an incr target mutates the global object itself — that surface
//! belongs to G2 (the minted singleton) and stays loud here; the
//! rewrite would silently retarget the mutation onto the bare
//! binding. Unknown property names stay loud too (bun answers
//! `undefined`, but a silent undefined here could mask a typo'd
//! builtin until runtime — G2 owns the dynamic surface).
//!
//! Shadow gate: a user binding NAMED `globalThis` (decl / param /
//! catch / loop var) anywhere in the program disables the pass —
//! the by-name match cannot tell which scope a read resolves to
//! (the same program-wide bar `fnexpr_this` uses).
//!
//! Runs at the top of the desugar pipeline: eval-inlined statements
//! (prelude) are already in place, and no later pass consumes a
//! `globalThis` member shape.

use crate::check_js_semantics::is_known_builtin_global;

use super::{Ast, Expr, ExprId, Param, Stmt};

pub fn desugar_globalthis_members(ast: &mut Ast) {
    if globalthis_shadowed(&ast.stmts) {
        return;
    }
    // Mutation positions keep the member shape (loud) — collect the
    // ExprIds standing as an Assign target, a Delete operand, or a
    // PostIncr target before rewriting anything.
    let mut excluded: std::collections::HashSet<ExprId> = std::collections::HashSet::new();
    for e in &ast.exprs {
        match e {
            Expr::Assign { target, .. } => {
                excluded.insert(*target);
            }
            Expr::Delete { expr } => {
                excluded.insert(*expr);
            }
            Expr::PostIncr { target, .. } => {
                excluded.insert(*target);
            }
            _ => {}
        }
    }
    for i in 0..ast.exprs.len() {
        if excluded.contains(&ExprId(i as u32)) {
            continue;
        }
        let Expr::Member { obj, name } = &ast.exprs[i] else {
            continue;
        };
        if !matches!(&ast.exprs[obj.0 as usize], Expr::Ident(n) if n == "globalThis") {
            continue;
        }
        // `globalThis.globalThis` re-mentions the object itself —
        // that value needs G2's singleton, not a bare-name rewrite.
        if name == "globalThis" || !is_known_builtin_global(name) {
            continue;
        }
        let bare = name.clone();
        ast.exprs[i] = Expr::Ident(bare);
    }
}

/// Any declaration site claiming the name `globalThis` — a LetDecl,
/// an fn param, a catch param, or a for-of loop var. Recursion covers
/// every stmt shape that owns a body.
fn globalthis_shadowed(stmts: &[Stmt]) -> bool {
    let param_hit = |params: &[Param]| params.iter().any(|p| p.name == "globalThis");
    for s in stmts {
        let hit = match s {
            Stmt::LetDecl { name, .. } => name == "globalThis",
            Stmt::FnDecl {
                name, params, body, ..
            } => name == "globalThis" || param_hit(params) || globalthis_shadowed(body),
            Stmt::Try {
                body,
                catch_param,
                catch_body,
                finally_body,
                ..
            } => {
                catch_param.as_deref() == Some("globalThis")
                    || globalthis_shadowed(body)
                    || globalthis_shadowed(catch_body)
                    || finally_body
                        .as_ref()
                        .is_some_and(|f| globalthis_shadowed(f))
            }
            Stmt::ForOf { var_name, body, .. } | Stmt::ForOfSplitIter { var_name, body, .. } => {
                var_name == "globalThis" || globalthis_shadowed(std::slice::from_ref(body))
            }
            Stmt::For { init, body, .. } => {
                init.as_ref()
                    .is_some_and(|i| globalthis_shadowed(std::slice::from_ref(i)))
                    || globalthis_shadowed(std::slice::from_ref(body))
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Labeled { body, .. } => {
                globalthis_shadowed(std::slice::from_ref(body))
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                globalthis_shadowed(std::slice::from_ref(then_branch))
                    || else_branch
                        .as_ref()
                        .is_some_and(|e| globalthis_shadowed(std::slice::from_ref(e)))
            }
            Stmt::Switch { cases, default, .. } => {
                cases.iter().any(|c| globalthis_shadowed(&c.body))
                    || default.as_ref().is_some_and(|d| globalthis_shadowed(d))
            }
            // The pass runs BEFORE desugar_classes, so method bodies
            // are still inside the ClassDecl — a method param (or a
            // decl inside one) named `globalThis` must gate too.
            Stmt::ClassDecl {
                ctor,
                methods,
                static_methods,
                static_init,
                ..
            } => {
                ctor.as_ref()
                    .is_some_and(|c| param_hit(&c.params) || globalthis_shadowed(&c.body))
                    || methods
                        .iter()
                        .chain(static_methods.iter())
                        .any(|m| param_hit(&m.params) || globalthis_shadowed(&m.body))
                    || static_init.iter().any(|si| match si {
                        super::StaticInit::Block(b) => globalthis_shadowed(b),
                        super::StaticInit::Field(_) => false,
                    })
            }
            Stmt::Block(inner) | Stmt::Multi(inner) => globalthis_shadowed(inner),
            _ => false,
        };
        if hit {
            return true;
        }
    }
    false
}
