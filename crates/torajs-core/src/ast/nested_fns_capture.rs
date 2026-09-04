//! A nested `function` declaration that reads something from the
//! function around it.
//!
//! ```text
//! function main(): void {
//!   const o = { v: 3 };
//!   function g(): number { return o.v; }   // bun: 3, tr: unknown identifier `o`
//!   console.log(g());
//! }
//! ```
//!
//! [`super::nested_fns`] lifts EVERY nested declaration to top level, and
//! a top-level body is checked in top-level scope — so it can see no
//! outer local. Its own doc has said for a long time what the fix is:
//! the capturing ones want what arrow fns get, an `__env` first param and
//! a `Closure` shape. This pass is that fix, and nothing more: it splits
//! the capturing declarations off and rewrites them into
//! `let <name> = <function expression>`, after which [`super::lift_arrow_fns`]
//! gives them the env exactly as it gives one to an arrow. Whatever this
//! pass leaves behind reaches `nested_fns` unchanged and lifts as before,
//! so every non-capturing declaration keeps its current codegen.
//!
//! **The rewrite stays where the declaration was.** A function
//! declaration is hoisted and initialized before any statement runs, so
//! the textbook rewrite belongs at the top of the enclosing body — but
//! the only thing hoisting buys is *using the name before the
//! declaration*, and a declaration with no captures never leaves the
//! old lane, where hoisting already works. What is left is the
//! declaration that both captures and is used early, and that one is a
//! type error today. In place it stays one: the name simply is not in
//! scope yet. Moving to the top instead would demand a box for every
//! captured binding declared later — a decision about when bindings get
//! boxed, which is a separate blade (RFC 20260725-nested-fn-capture §4).
//!
//! Recursion needs nothing new. Rewritten, a self-recursive declaration
//! is a closure naming the binding it initializes, and a mutually
//! recursive pair is one closure capturing a peer declared later —
//! the two lanes rotation 212 built.
//!
//! ## What routes
//!
//! Per statement list, over its direct `function` declarations:
//!
//! 1. A declaration with a free variable that is not one of its peers
//!    must route — that free variable is the outer local it cannot see.
//! 2. Peers that reference each other route together. A routed
//!    declaration becomes a local `let`, which a peer left on the
//!    lifting lane could not name from top-level scope; both directions
//!    of the reference edge therefore pull, and routing follows the
//!    connected component.
//! 3. A component containing anything this pass cannot faithfully
//!    rewrite — a generator, a generic, or a body naming `arguments`
//!    (a declaration owns that binding, an arrow does not) — routes
//!    nothing. The list keeps today's behavior, error and all.
//!
//! Bodies route before the lists that hold them, so a declaration two
//! levels down is already an arrow when its parent's free variables are
//! computed and the capture propagates outward on its own.

use std::collections::{HashMap, HashSet};

use super::annexb_fn_var::annexb_applies_at;
use super::free_vars::free_vars_of_body;
use super::nested_fns_idents::rewrite_idents_in_body;
use super::{Ast, Expr, Stmt};

/// Compiler-minted declarations (lifted arrows, class methods, generator
/// state machines, an earlier nested lift) are never user nesting.
fn is_synthetic(name: &str) -> bool {
    name.starts_with("__closure_")
        || name.starts_with("__cm_")
        || name.starts_with("__sm_")
        || name.starts_with("__nested_")
}

pub fn desugar_capturing_nested_fns(ast: &mut Ast) {
    let mut top = std::mem::take(&mut ast.stmts);
    let globals: Vec<String> = top
        .iter()
        .filter_map(|s| match s {
            Stmt::FnDecl { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    // Function-expression / arrow bodies live in the expression arena,
    // not the statement tree — a declaration nested in one is invisible
    // to the descent below, and `nested_fns` would lift it to top level
    // where its captures (the closure's params and locals) resolve to
    // nothing. The arena holds every such body flat, inner before outer
    // (the parser emits inner expressions first), so a fixed-length
    // low→high scan routes each body exactly once; bodies minted by the
    // rewrite itself were routed while taken out and need no revisit.
    // Unique across the pass — it names the block bindings §B.3.3 needs
    // to keep apart from the var bindings they are written into.
    let mut counter: u32 = 0;
    let n = ast.exprs.len();
    for i in 0..n {
        if !matches!(ast.exprs[i], Expr::ArrowFn { .. }) {
            continue;
        }
        let mut body = match &mut ast.exprs[i] {
            Expr::ArrowFn { body, .. } => std::mem::take(body),
            _ => unreachable!("matched ArrowFn above"),
        };
        walk_list(
            ast,
            &mut body,
            &globals,
            &mut Vec::new(),
            false,
            &mut counter,
        );
        match &mut ast.exprs[i] {
            Expr::ArrowFn { body: b, .. } => *b = body,
            _ => unreachable!("matched ArrowFn above"),
        }
    }
    // The module's own statement list is not a routing site — its
    // `function` declarations ARE the globals. Only descend.
    let mut scope: Vec<String> = Vec::new();
    for s in top.iter_mut() {
        walk_children(ast, s, &globals, &mut scope, false, &mut counter);
    }
    ast.stmts = top;
}

/// Route `stmts` after everything nested inside it has routed.
///
/// `scope` carries the bindings the enclosing scopes introduce, so
/// `route_list` can tell a top-level fn name that is still in scope from
/// one a local binding has rebound (RFC 20260828 knife 3).
fn walk_list(
    ast: &mut Ast,
    stmts: &mut Vec<Stmt>,
    globals: &[String],
    scope: &mut Vec<String>,
    in_block: bool,
    counter: &mut u32,
) {
    let depth = scope.len();
    for s in stmts.iter_mut() {
        walk_children(ast, s, globals, scope, in_block, counter);
        // Visible to the declarations that follow, and to every routing
        // decision this list makes.
        if let Stmt::LetDecl { name, .. } | Stmt::UsingDecl { name, .. } = s {
            scope.push(name.clone());
        }
    }
    route_list(ast, stmts, globals, scope, in_block, counter);
    scope.truncate(depth);
}

/// Descend into every statement list `s` owns. A `FnDecl` body is a
/// scope of its own and routes as one; the block-shaped statements carry
/// lists that route as blocks do.
fn walk_children(
    ast: &mut Ast,
    s: &mut Stmt,
    globals: &[String],
    scope: &mut Vec<String>,
    in_block: bool,
    counter: &mut u32,
) {
    match s {
        Stmt::FnDecl { params, body, .. } => {
            let depth = scope.len();
            scope.extend(params.iter().map(|p| p.name.clone()));
            walk_list(ast, body, globals, scope, false, counter);
            scope.truncate(depth);
        }
        // A `Block` opens one; `Multi` is a compiler-minted sequence
        // sharing the surrounding scope, so it is whatever its parent is.
        Stmt::Block(b) => walk_list(ast, b, globals, scope, true, counter),
        Stmt::Multi(b) => walk_list(ast, b, globals, scope, in_block, counter),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            walk_children(ast, then_branch, globals, scope, true, counter);
            if let Some(eb) = else_branch.as_deref_mut() {
                walk_children(ast, eb, globals, scope, true, counter);
            }
        }
        Stmt::ForOf { var_name, body, .. } => {
            let depth = scope.len();
            scope.push(var_name.clone());
            walk_children(ast, body, globals, scope, true, counter);
            scope.truncate(depth);
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::For { body, .. }
        | Stmt::ForOfSplitIter { body, .. }
        | Stmt::Labeled { body, .. } => walk_children(ast, body, globals, scope, true, counter),
        Stmt::Try {
            body,
            catch_param,
            catch_body,
            finally_body,
            ..
        } => {
            walk_list(ast, body, globals, scope, true, counter);
            let depth = scope.len();
            if let Some(cp) = catch_param {
                scope.push(cp.clone());
            }
            walk_list(ast, catch_body, globals, scope, true, counter);
            scope.truncate(depth);
            if let Some(fb) = finally_body {
                walk_list(ast, fb, globals, scope, true, counter);
            }
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases.iter_mut() {
                walk_list(ast, &mut c.body, globals, scope, true, counter);
            }
            if let Some(d) = default {
                walk_list(ast, d, globals, scope, true, counter);
            }
        }
        _ => {}
    }
}

/// One statement list: decide which of its `function` declarations route,
/// then rewrite those in place.
fn route_list(
    ast: &mut Ast,
    stmts: &mut [Stmt],
    globals: &[String],
    scope: &[String],
    in_block: bool,
    counter: &mut u32,
) {
    let sites: Vec<usize> = stmts
        .iter()
        .enumerate()
        .filter_map(|(i, s)| match s {
            Stmt::FnDecl { name, .. } if !is_synthetic(name) => Some(i),
            _ => None,
        })
        .collect();
    if sites.is_empty() {
        return;
    }
    let names: Vec<String> = sites
        .iter()
        .map(|i| match &stmts[*i] {
            Stmt::FnDecl { name, .. } => name.clone(),
            _ => unreachable!("site is a FnDecl"),
        })
        .collect();
    // Read before the rewrite consumes the declaration: §B.3.3 asks
    // whether the declaration's own text is strict.
    let spans: Vec<crate::lexer::Span> = sites
        .iter()
        .map(|i| match &stmts[*i] {
            Stmt::FnDecl { span, .. } => *span,
            _ => unreachable!("site is a FnDecl"),
        })
        .collect();
    let peers: HashSet<&str> = names.iter().map(|n| n.as_str()).collect();
    // Free variables with the peers deliberately NOT pre-bound: the same
    // walk has to answer both questions — what the declaration reads from
    // outside (peers excluded) and which peers it names (peers alone).
    let free: Vec<Vec<String>> = sites
        .iter()
        .map(|i| match &stmts[*i] {
            Stmt::FnDecl { params, body, .. } => {
                let mut prebound: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
                // A top-level fn name an enclosing scope has rebound is
                // NOT in scope here — the reference belongs to the local
                // and has to be captured (RFC 20260828).
                prebound.extend(
                    globals
                        .iter()
                        .filter(|g| !scope.iter().any(|b| b == *g))
                        .cloned(),
                );
                free_vars_of_body(ast, &prebound, body)
            }
            _ => unreachable!("site is a FnDecl"),
        })
        .collect();
    let mut routed: Vec<bool> = free
        .iter()
        .map(|f| f.iter().any(|n| !peers.contains(n.as_str())))
        .collect();
    if !routed.iter().any(|r| *r) {
        return;
    }
    grow_to_components(&names, &free, &mut routed);
    for (k, i) in sites.iter().enumerate() {
        if routed[k] && !can_rewrite(&stmts[*i], &free[k]) {
            return;
        }
    }
    for (k, i) in sites.iter().enumerate() {
        if routed[k] {
            rewrite_in_place(ast, &mut stmts[*i]);
        }
    }
    // Annex B §B.3.3 — a declaration nested in a BLOCK has TWO bindings
    // in sloppy code: the block one this pass just minted, and a
    // var-scoped one on the enclosing function or script, written where
    // the declaration sits. Both are spelled the same name, so the block
    // one has to be renamed for the write to reach the var binding at
    // all — the same move the lifting lane makes when it mangles. What
    // is inside the block and meant the declaration follows the rename;
    // what is outside means the var binding and is left alone.
    //
    // The block binding is `mutable`: §B.3.3 makes it an ordinary
    // let-like binding, and test262's `block-scoping` family assigns to
    // it (`f = 123` inside the body) and then checks that the var
    // binding still holds the function.
    if !in_block {
        return;
    }
    for (k, i) in sites.iter().enumerate() {
        // A name an enclosing scope already binds is §B.3.3.1 step
        // 1.a.ii's early error — the extension does not apply and the
        // reference belongs to that owner.
        if !routed[k] || scope.iter().any(|b| *b == names[k]) {
            continue;
        }
        if !annexb_applies_at(ast, spans[k]) {
            continue;
        }
        let mangled = format!("__blkfn_{}_{}", names[k], counter);
        *counter += 1;
        let map: HashMap<String, String> = HashMap::from([(names[k].clone(), mangled.clone())]);
        rewrite_idents_in_body(ast, stmts, &map, true);
        let var_init = ast.add_expr(Expr::Ident(mangled.clone()));
        ast.set_expr_span(var_init, spans[k]);
        let taken = std::mem::replace(&mut stmts[*i], Stmt::Block(Vec::new()));
        let Stmt::LetDecl {
            type_ann,
            init,
            is_var,
            ..
        } = taken
        else {
            unreachable!("the rewrite above left a LetDecl");
        };
        stmts[*i] = Stmt::Multi(vec![
            Stmt::LetDecl {
                mutable: true,
                name: mangled,
                type_ann,
                init,
                is_var,
            },
            Stmt::LetDecl {
                mutable: true,
                name: names[k].clone(),
                type_ann: None,
                init: var_init,
                is_var: true,
            },
        ]);
    }
}

/// Pull every peer joined to a routed one by a reference edge, in either
/// direction, until the routed set is closed under the relation.
fn grow_to_components(names: &[String], free: &[Vec<String>], routed: &mut [bool]) {
    loop {
        let mut changed = false;
        for a in 0..names.len() {
            for b in 0..names.len() {
                if a == b || routed[a] == routed[b] {
                    continue;
                }
                let edge = free[a].iter().any(|n| *n == names[b])
                    || free[b].iter().any(|n| *n == names[a]);
                if edge {
                    routed[a] = true;
                    routed[b] = true;
                    changed = true;
                }
            }
        }
        if !changed {
            return;
        }
    }
}

/// An arrow has no type parameters, is never a generator, and does not
/// own `arguments`. A declaration wanting any of those keeps its
/// declaration form — today's answer beats a quietly different one.
fn can_rewrite(s: &Stmt, free: &[String]) -> bool {
    let Stmt::FnDecl {
        type_params,
        is_generator,
        ..
    } = s
    else {
        unreachable!("site is a FnDecl");
    };
    !*is_generator && type_params.is_empty() && !free.iter().any(|n| n == "arguments")
}

/// `function f(p): R { … }` → `let f = function (p): R { … }`, in the
/// slot the declaration occupied. Registering the expression as a
/// function expression (rather than leaving it to read as an arrow) is
/// what gives the lifted closure its `.prototype`, which a declaration
/// has; the span carries over so the lifted decl still points at the
/// user's text.
fn rewrite_in_place(ast: &mut Ast, slot: &mut Stmt) {
    let taken = std::mem::replace(slot, Stmt::Block(Vec::new()));
    let Stmt::FnDecl {
        name,
        params,
        return_type,
        body,
        span,
        ..
    } = taken
    else {
        unreachable!("site is a FnDecl");
    };
    let eid = ast.add_expr(Expr::ArrowFn {
        params,
        return_type,
        body,
    });
    ast.set_expr_span(eid, span);
    ast.fn_expr_exprs.insert(eid);
    *slot = Stmt::LetDecl {
        mutable: false,
        name,
        type_ann: None,
        init: eid,
        is_var: false,
    };
}
