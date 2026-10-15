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
//! Which reads rewrite: `globalThis.X` reads the global object's
//! property whatever scope it sits in (§9.4.2), and the bare `X` is
//! the same thing only where no declaration of `X` is in scope.
//!
//! - Where no declaration of `X` is in scope at the read
//!   (`globalthis_member_scope`), the bare name is the builtin. A
//!   sloppy script's top-level `var X` / `function X` counts as no
//!   declaration here: it IS the global object's property (§16.1.7),
//!   so the bare name reaches the same value the read does.
//! - Where one is — a parameter, a local, a module or script top-level
//!   `let` / `const` / `class`, an import — that declaration is renamed
//!   out of the way when the rename cannot be observed
//!   (`rename`), and the read becomes the bare
//!   builtin; otherwise it stays a member read of the runtime object.
//! - A class declaration of `X` anywhere keeps every read of
//!   `globalThis.X` a member read: a nested class is hoisted to the
//!   top level later, where the bare name would reach it.
//! - A binding NAMED `globalThis` anywhere disables the pass: the
//!   object itself could be the user's.
//!
//! Runs at the top of the desugar pipeline: eval-inlined statements
//! (prelude) are already in place, and no later pass consumes a
//! `globalThis` member shape.

mod rename;

use std::collections::{HashMap, HashSet};

use crate::check_js_semantics::is_known_builtin_global;

use super::globalthis_member_scope::{script_global_props, take_census};
use super::hoist_nested_classes_census::count_class_decl_names;
use super::{Ast, Expr, ExprId, Param, Stmt};
use rename::{renamable, rename};

pub fn desugar_globalthis_members(ast: &mut Ast) {
    if binds_anywhere(ast, "globalThis") {
        return;
    }
    // Mutation positions keep the member shape (loud) — collect the
    // ExprIds standing as an Assign target, a Delete operand, or a
    // PostIncr target before rewriting anything.
    let mut excluded: HashSet<ExprId> = HashSet::new();
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
    let mut sites: Vec<(ExprId, String)> = Vec::new();
    for (i, e) in ast.exprs.iter().enumerate() {
        let eid = ExprId(i as u32);
        let Expr::Member { obj, name } = e else {
            continue;
        };
        if excluded.contains(&eid)
            || !matches!(ast.get_expr(*obj), Expr::Ident(n) if n == "globalThis")
        {
            continue;
        }
        // `globalThis.globalThis` re-mentions the object itself —
        // that value needs G2's singleton, not a bare-name rewrite.
        if name == "globalThis" || !is_known_builtin_global(name) {
            continue;
        }
        sites.push((eid, name.clone()));
    }
    if sites.is_empty() {
        return;
    }
    let watch: HashSet<String> = sites.iter().map(|(_, n)| n.clone()).collect();
    let props = if ast.sloppy_script_goal {
        script_global_props(ast, &watch)
    } else {
        HashSet::new()
    };
    let census = take_census(ast, &watch, &props);
    let classes = class_names(ast, &watch);
    // A read the walk did not reach (a side-table root) is judged by
    // whether the program declares the name at all.
    let mut declared: HashMap<String, bool> = HashMap::new();
    let mut shadowed = |ast: &Ast, eid: ExprId, name: &str| {
        classes.contains(name)
            || match census.site_shadowed.get(&eid) {
                Some(&b) => b,
                None => *declared
                    .entry(name.to_string())
                    .or_insert_with(|| binds_anywhere(ast, name)),
            }
    };
    let mut keep: HashSet<ExprId> = HashSet::new();
    for (eid, name) in &sites {
        if shadowed(ast, *eid, name) {
            keep.insert(*eid);
        }
    }
    let mut names: Vec<&String> = watch.iter().collect();
    names.sort();
    for name in names {
        let hit = sites.iter().any(|(e, n)| n == name && keep.contains(e));
        // a rename would also move the script's own top-level binding,
        // which is the property the read reaches
        if hit && !props.contains(name) && !classes.contains(name) && renamable(ast, &census, name)
        {
            let fresh = rename(ast, &census, name, |ast, c| {
                binds_anywhere(ast, c)
                    || ast
                        .exprs
                        .iter()
                        .any(|e| matches!(e, Expr::Ident(n) if n == c))
            });
            ast.renamed_binding_display.insert(fresh, name.clone());
            keep.retain(|e| sites.iter().all(|(s, n)| s != e || n != name));
        }
    }
    for (eid, name) in sites {
        if !keep.contains(&eid) {
            ast.exprs[eid.0 as usize] = Expr::Ident(name);
        }
    }
}

/// The watched names some class declaration in the program spells.
/// `hoist_nested_classes` moves a class declared in a block or a
/// function body to the top level under its own name, so after it a
/// bare `X` anywhere may reach the user's class even where the scopes
/// the census follows say no declaration of `X` is in scope.
fn class_names(ast: &Ast, watch: &HashSet<String>) -> HashSet<String> {
    let mut counts = HashMap::new();
    let arrow_bodies = ast.exprs.iter().filter_map(|e| match e {
        Expr::ArrowFn { body, .. } => Some(body),
        _ => None,
    });
    for s in ast.stmts.iter().chain(arrow_bodies.flatten()) {
        count_class_decl_names(s, &mut counts);
    }
    counts.into_keys().filter(|n| watch.contains(n)).collect()
}

/// Some declaration anywhere in the program — statement-level or in
/// an arrow's parameter list or body — binds `target`.
fn binds_anywhere(ast: &Ast, target: &str) -> bool {
    bound_in(&ast.stmts, target)
        || ast.exprs.iter().any(|e| match e {
            Expr::ArrowFn { params, body, .. } => {
                params.iter().any(|p| p.name == target) || bound_in(body, target)
            }
            _ => false,
        })
}

/// Any declaration site in `stmts` claiming `target` — a LetDecl /
/// UsingDecl / FnDecl / ClassDecl name, an fn param, a catch param,
/// or a for-of loop var. Recursion covers every stmt shape that owns
/// a body.
fn bound_in(stmts: &[Stmt], target: &str) -> bool {
    let param_hit = |params: &[Param]| params.iter().any(|p| p.name == target);
    let one = |s: &Stmt| bound_in(std::slice::from_ref(s), target);
    for s in stmts {
        let hit = match s {
            Stmt::LetDecl { name, .. } | Stmt::UsingDecl { name, .. } => name == target,
            Stmt::FnDecl {
                name, params, body, ..
            } => name == target || param_hit(params) || bound_in(body, target),
            Stmt::Try {
                body,
                catch_param,
                catch_body,
                finally_body,
                ..
            } => {
                catch_param.as_deref() == Some(target)
                    || bound_in(body, target)
                    || bound_in(catch_body, target)
                    || finally_body.as_ref().is_some_and(|f| bound_in(f, target))
            }
            Stmt::ForOf { var_name, body, .. } | Stmt::ForOfSplitIter { var_name, body, .. } => {
                var_name == target || one(body)
            }
            Stmt::For { init, body, .. } => init.as_ref().is_some_and(|i| one(i)) || one(body),
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Labeled { body, .. } => {
                one(body)
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => one(then_branch) || else_branch.as_ref().is_some_and(|e| one(e)),
            Stmt::Switch { cases, default, .. } => {
                cases.iter().any(|c| bound_in(&c.body, target))
                    || default.as_ref().is_some_and(|d| bound_in(d, target))
            }
            // The pass runs BEFORE desugar_classes, so method bodies
            // are still inside the ClassDecl — a method param (or a
            // decl inside one) claiming the name must gate too.
            Stmt::ClassDecl {
                name,
                ctor,
                methods,
                static_methods,
                static_init,
                ..
            } => {
                name == target
                    || ctor
                        .as_ref()
                        .is_some_and(|c| param_hit(&c.params) || bound_in(&c.body, target))
                    || methods
                        .iter()
                        .chain(static_methods.iter())
                        .any(|m| param_hit(&m.params) || bound_in(&m.body, target))
                    || static_init.iter().any(|si| match si {
                        super::StaticInit::Block(b) => bound_in(b, target),
                        super::StaticInit::Field(_) => false,
                    })
            }
            Stmt::Block(inner) | Stmt::Multi(inner) => bound_in(inner, target),
            _ => false,
        };
        if hit {
            return true;
        }
    }
    false
}
