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
//! **The rewrite moves up only as far as the first use.** A function
//! declaration is initialized before any statement of its body runs
//! (§10.2.11), and the only thing that buys is using the name before the
//! declaration's text. So a routed `let` stays where the declaration was
//! unless an earlier statement of the same body reaches it — naming it,
//! or calling a peer that names it — and then moves to just above the
//! first such statement ([`early_use`]). A captured binding the move
//! leaves below the closure becomes a forward capture, the lane
//! `check_hoist_closure_lets` and the lowering's forward-capture boxes
//! already serve. A block declaration §B.3.3 splits stays in place:
//! Annex B writes its var binding where the text sits, so one reached
//! early is not split.
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
//!    must route — that free variable is the outer local it cannot see
//!    (unless only its defaults read it and every call can pad them,
//!    see [`pad`]).
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

use std::collections::HashSet;

use super::annexb_block_binding::split_block_binding;
use super::free_vars::free_vars_of_fn;
use super::scope_chain::{list_names, not_rebound};
use super::{Ast, Expr, ExprId, Stmt};

mod case_block;
mod early_use;
mod pad;
mod rewrite;
mod walk;
pub(super) use early_use::hoist_above_first_use;
use early_use::{EarlyUse, early_use, raise_early};
use rewrite::rewrite_in_place;
use walk::{Scopes, walk_children};

/// Compiler-minted declarations (lifted arrows, class methods, generator
/// state machines, an earlier nested lift) are never user nesting.
pub(super) fn is_synthetic(name: &str) -> bool {
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
    // The arena scan cannot see which body an arrow sits in, so the
    // frames around each one are read off the tree first.
    let chains = super::scope_chain::chains_by_arrow(ast, &[&top]);
    let n = ast.exprs.len();
    for i in 0..n {
        if !matches!(ast.exprs[i], Expr::ArrowFn { .. }) {
            continue;
        }
        let mut body = match &mut ast.exprs[i] {
            Expr::ArrowFn { body, .. } => std::mem::take(body),
            _ => unreachable!("matched ArrowFn above"),
        };
        let mut scope = Scopes::of_arrow(ast, &chains, ExprId(i as u32), &body);
        walk_list(ast, &mut body, &globals, &mut scope, false, &mut counter);
        match &mut ast.exprs[i] {
            Expr::ArrowFn { body: b, .. } => *b = body,
            _ => unreachable!("matched ArrowFn above"),
        }
    }
    // The module's own statement list is not a routing site — its
    // `function` declarations ARE the globals. Only descend.
    let mut scope = Scopes::default();
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
    stmts: &mut [Stmt],
    globals: &[String],
    scope: &mut Scopes,
    in_block: bool,
    counter: &mut u32,
) {
    let depth = scope.mark();
    // A list's declarations bind for its whole extent.
    list_names(stmts, &mut scope.shadow);
    for s in stmts.iter_mut() {
        walk_children(ast, s, globals, scope, in_block, counter);
        // Visible to the declarations that follow, and to every routing
        // decision this list makes.
        if let Stmt::LetDecl { name, .. } | Stmt::UsingDecl { name, .. } = s {
            scope.annexb.push(name.clone());
        }
    }
    let (routed, early) = route_list(ast, stmts, globals, scope, in_block);
    // Annex B §B.3.3 — a declaration nested in a BLOCK has a second,
    // var-scoped binding as well, and the rewrite only made the block
    // one. See `annexb_block_binding` for what the split does.
    if in_block {
        for r in &routed {
            // A name an enclosing scope already binds is §B.3.3.1 step
            // 1.a.ii's early error — the extension does not apply and
            // the reference belongs to that owner.
            if r.early || scope.annexb.contains(&r.name) {
                continue;
            }
            split_block_binding(ast, stmts, r.idx, &r.name, r.span, counter);
        }
    }
    if let Some(early) = &early {
        raise_early(stmts, early);
    }
    scope.reset(depth);
}

/// One declaration the router rewrote into a `let` in place.
pub(super) struct Routed {
    pub(super) idx: usize,
    pub(super) name: String,
    pub(super) span: crate::lexer::Span,
    /// What it reads from outside, its peers included.
    pub(super) free: Vec<String>,
    /// A statement above it reaches it, so it moves up whole and is not
    /// split (`early_use`).
    pub(super) early: bool,
}

/// One statement list: decide which of its `function` declarations route,
/// then rewrite those in place.
fn route_list(
    ast: &mut Ast,
    stmts: &mut [Stmt],
    globals: &[String],
    scope: &Scopes,
    in_block: bool,
) -> (Vec<Routed>, Option<EarlyUse>) {
    let sites: Vec<usize> = stmts
        .iter()
        .enumerate()
        .filter_map(|(i, s)| match s {
            Stmt::FnDecl { name, .. } if !is_synthetic(name) => Some(i),
            _ => None,
        })
        .collect();
    if sites.is_empty() {
        return (Vec::new(), None);
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
    let prebound: Vec<Vec<String>> = sites
        .iter()
        .map(|i| site_prebound(&stmts[*i], globals, &scope.shadow))
        .collect();
    let free: Vec<Vec<String>> = sites
        .iter()
        .zip(&prebound)
        .map(|(i, pre)| match &stmts[*i] {
            Stmt::FnDecl { params, body, .. } => free_vars_of_fn(ast, pre, &[], params, body),
            _ => unreachable!("site is a FnDecl"),
        })
        .collect();
    let mut routed: Vec<bool> = (0..sites.len())
        .map(|k| {
            free[k].iter().any(|n| !peers.contains(n.as_str()))
                && (in_block || !pad::pad_reads_scope(ast, stmts, sites[k], &prebound[k], &peers))
        })
        .collect();
    if !routed.iter().any(|r| *r) {
        return (Vec::new(), None);
    }
    grow_to_components(&names, &free, &mut routed);
    for (k, i) in sites.iter().enumerate() {
        if routed[k] && !can_rewrite(&stmts[*i], &free[k]) {
            return (Vec::new(), None);
        }
    }
    for (k, i) in sites.iter().enumerate() {
        if routed[k] {
            rewrite_in_place(ast, &mut stmts[*i]);
        }
    }
    let routed_sites: Vec<usize> = (0..sites.len())
        .filter(|k| routed[*k])
        .map(|k| sites[k])
        .collect();
    // Settled before the split: a binding a statement above can reach,
    // even through a peer it calls, has to be live there, so it moves
    // up whole instead of splitting (`early_use`).
    let early = early_use(ast, stmts, &routed_sites);
    let out = (0..sites.len())
        .filter(|k| routed[*k])
        .map(|k| Routed {
            idx: sites[k],
            name: names[k].clone(),
            span: spans[k],
            free: free[k].clone(),
            early: early.as_ref().is_some_and(|e| e.sites.contains(&sites[k])),
        })
        .collect();
    (out, early)
}

/// What a declaration's frame binds before its free variables are read:
/// its params, and every top-level fn name still in scope. One a frame
/// in `shadow` rebinds is NOT — the reference belongs to the local and
/// has to be captured (RFC 20260828).
fn site_prebound(s: &Stmt, globals: &[String], shadow: &[String]) -> Vec<String> {
    let Stmt::FnDecl { params, .. } = s else {
        unreachable!("site is a FnDecl");
    };
    let mut prebound: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
    prebound.extend(not_rebound(globals, shadow));
    prebound
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
