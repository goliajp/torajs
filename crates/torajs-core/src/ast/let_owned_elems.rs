//! Which `let` / `const` / `var` bindings must hold OWNED elements —
//! the census the split-product binding lane consults (rotation 468,
//! the write face of plan-state 467-01 and the escape face found
//! alongside it).
//!
//! `s.split(sep)` answers an array of substring VIEWS: 32-byte cells
//! in the split block's tail that read their text through the parent
//! string, typed `Arr<Substr>`. That representation is only right
//! while the product stays in its typed-tier home — read by index,
//! measured, iterated, joined. Two things break it:
//!
//! - an owned WRITE into it — `X.push(v)` / `unshift` / `splice` /
//!   `fill` / `copyWithin`, or `X[i] = v` — because an `Arr<Substr>`
//!   slot cannot take an owned string and every reader decodes the
//!   slots by the view layout (`a.push("z"+"w"); a.join("+")`
//!   SIGSEGV'd);
//! - a REBIND of the binding itself — `X = other` — because the slot
//!   then has to take whatever the program stores into it later, and
//!   an `Arr<Substr>` slot takes only a fresh split product;
//! - an ESCAPE of the whole array as a bare value — a call or `new`
//!   argument, the init of another binding, the value of an
//!   assignment, an array or object literal element, a return or
//!   yield — because the receiving side is typed by its own
//!   annotation (`string[]` → `Arr<Str>`, or `any`) and decodes the
//!   views as owned strings (`addx(parts)` with `xs: string[]` printed
//!   garbage; `let b = a; b.push(..)` crashed; `let v: any = a` then
//!   `for (const x of v)` read freed memory).
//!
//! For a binding listed here, the lane materializes the fresh product
//! in place (every view becomes an owned string — torajs-arr
//! `substr_materialize.rs`) and types the binding `Arr<Str>`, where
//! the mutators store any string shape and every receiver's
//! annotation already agrees. Sort / reverse / pop / shift only
//! permute or remove and are not writes; element reads, `.length`,
//! for-of, join and the other in-home consumers are not escapes.
//!
//! ## Shape
//!
//! Sibling of [`super::regex_result_props`] and
//! [`super::escape_analyze`]: the same per-binding window (a `let` is
//! in scope from its declaration to the end of its block, so the
//! trailing statements are every place it can be used — plus the
//! block's hoisted function declarations; a top-level binding is
//! reachable from every function body, so its window is the whole
//! program) and the same bias rule: a shape the walk cannot prove is
//! not a write or an escape COUNTS as one (a lifted closure that
//! captured the binding, an arrow whose body is not walked). A false
//! positive only materializes a product that did not need it; a false
//! negative stores an owned cell into a view-typed array or hands
//! views to a reader that cannot tell. Keyed by the declaration's
//! init ExprId, which is what the binding lane holds when it decides.
//!
//! ## Temporaries
//!
//! A product that is never bound has no window to walk, so the same
//! question is asked of its position instead: `view_home_exprs` lists
//! the expressions consumed where they are produced
//! ([`super::view_homes`]). A product anywhere else is on its way to
//! a receiver typed by its own annotation, and the lowering hands it
//! on as owned strings (`pre.push(line.split(","))` then `pre[i][0]`
//! printed garbage).

//!
//! ## Aliases
//!
//! A bare rebind or re-binding between two such bindings (`last =
//! parts`, `let b = a`) is an escape only when the two slots would
//! disagree on the layout. [`super::let_owned_elems_alias`] connects
//! the names into classes and rules per class: an unpoisoned class
//! fed by a split call carries views in every member — the `[]` and
//! alias inits among them get the view layout at their declaration
//! (`view_slot_inits`) — and is listed here in neither set.

mod writes;

use std::collections::HashSet;

use super::let_owned_elems_alias::{self as alias, AliasNames, Decl};
use super::let_owned_elems_nest::Nest;
use super::view_homes;
use super::{Ast, ExprId, Stmt};
use writes::{Query, stmt_writes};

/// Record the init ExprIds of the bindings that must hold owned
/// elements: written into with an owned value, or escaping as a bare
/// value, somewhere in their scope. Runs at the end of the desugar
/// pipeline for the siblings' reason: the census has to see the final
/// shape of every use of the binding.
pub fn analyze_let_owned_elems(ast: &mut Ast) {
    let names = alias::collect_names(ast);
    let mut found = Found::default();
    let stmts = ast.stmts.clone();
    low_walk_stmts(ast, &stmts, true, &names, &mut found);
    view_homes::mark_read_objects(ast, &mut found.homes);
    let (owned, view_slots) = alias::settle(ast, &names, &found.decls);
    ast.let_owned_elem_inits = owned;
    ast.view_slot_inits = view_slots;
    ast.view_home_exprs = found.homes;
}

#[derive(Default)]
struct Found {
    decls: Vec<Decl>,
    homes: HashSet<ExprId>,
}

fn low_walk_stmts(
    ast: &Ast,
    stmts: &[Stmt],
    top_level: bool,
    names: &AliasNames,
    found: &mut Found,
) {
    for (i, s) in stmts.iter().enumerate() {
        view_homes::mark_stmt_homes(ast, s, &mut found.homes);
        if let Stmt::LetDecl {
            name,
            init,
            type_ann,
            ..
        } = s
        {
            // The trailing statements are every place the binding
            // can be written — and a hoisted fn decl anywhere in the
            // block can reach it too. At top level every function
            // body can reach the binding, so the whole program is the
            // window.
            let window = |q: &Query<'_>| {
                if top_level {
                    stmts.iter().any(|t| stmt_writes(ast, t, q))
                } else {
                    stmts[i + 1..].iter().any(|t| stmt_writes(ast, t, q))
                        || stmts
                            .iter()
                            .any(|t| matches!(t, Stmt::FnDecl { .. }) && stmt_writes(ast, t, q))
                }
            };
            found.decls.push(Decl {
                name: name.clone(),
                init: *init,
                candidate: alias::is_candidate(ast, names, name, type_ann.as_deref(), *init),
                written: window(&Query {
                    x: name,
                    aliases: None,
                    nest: None,
                }),
                poisoned: window(&Query {
                    x: name,
                    aliases: Some(names),
                    nest: None,
                }),
            });
            // a container's element class is ruled on over the
            // container's window, and every occurrence of the
            // container must have been seen there
            if let Some(class) = names.containers.get(name) {
                let n = Nest {
                    c: name,
                    class,
                    seen: Default::default(),
                };
                let poisoned = window(&Query {
                    x: class,
                    aliases: Some(names),
                    nest: Some(&n),
                }) || !n.all_seen(ast);
                found.decls.push(Decl {
                    name: class.clone(),
                    init: *init,
                    candidate: true,
                    written: false,
                    poisoned,
                });
            }
        }
    }
    for s in stmts {
        low_recurse_into(ast, s, names, found);
    }
}

fn low_recurse_into(ast: &Ast, s: &Stmt, names: &AliasNames, found: &mut Found) {
    let walk = |inner: &[Stmt], found: &mut Found| low_walk_stmts(ast, inner, false, names, found);
    match s {
        Stmt::Block(inner) | Stmt::Multi(inner) => walk(inner, found),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            low_recurse_into(ast, then_branch, names, found);
            if let Some(eb) = else_branch {
                low_recurse_into(ast, eb, names, found);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            low_recurse_into(ast, body, names, found)
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                low_recurse_into(ast, i, names, found);
            }
            low_recurse_into(ast, body, names, found);
        }
        Stmt::ForOfSplitIter { body, .. } | Stmt::ForOf { body, .. } => {
            low_recurse_into(ast, body, names, found)
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                walk(&c.body, found);
            }
            if let Some(db) = default {
                walk(db, found);
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            walk(body, found);
            walk(catch_body, found);
            if let Some(fb) = finally_body {
                walk(fb, found);
            }
        }
        Stmt::FnDecl { body, .. } => walk(body, found),
        Stmt::ClassDecl { methods, .. } => {
            for m in methods {
                walk(&m.body, found);
            }
        }
        Stmt::ExportDecl {
            inner: Some(inner), ..
        } => low_recurse_into(ast, inner, names, found),
        Stmt::Labeled { body, .. } => low_recurse_into(ast, body, names, found),
        _ => {}
    }
}
