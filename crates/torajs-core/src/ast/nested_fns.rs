//! Nested-fn hoisting pass — chunk 358, extracted from ast.rs.
//!
//! Pub entry `desugar_nested_fns` implements ES Annex B §B.3.3
//! (FunctionDeclarations in IfStatement Blocks). Walks every top-
//! level FnDecl body + module-top-level non-FnDecl stmts, finds
//! nested `function f() {...}` decls inside any block-shape stmt,
//! lifts them to top-level with mangled names
//! `__nested_<parent>_<name>_<uid>`, rewrites references. The
//! private helpers (`collect_nested_fns_in_stmt/to_lift`, and the
//! scope-aware `rename_in_owner`) do the tree walk — both passes
//! share ONE stmt collector since rotation 269 (the pass-1-only
//! sibling lacked the bare-FnDecl-branch arm).

use super::annexb_fn_var::{AnnexB, LiftCtx, LiftMode};
use super::annexb_names::{block_nested_fn_names, own_fn_names, param_names};
use super::desugar_async_nested_fns::rename::rename_in_owner;
use super::nested_fns_catch::apply_after_lift;
use super::nested_fns_idents::rewrite_idents_in_stmt;
use super::{Ast, Stmt};
use std::collections::{HashMap, HashSet};

mod bare_branch;
mod body_lift;

pub(super) use bare_branch::collect_nested_fns_in_stmt;
pub(super) use body_lift::collect_fn_body_lifts;

/// P3.4 — block-scoped / nested function declaration hoisting per
/// ES spec Annex B §B.3.3 (FunctionDeclarations in IfStatement
/// Blocks). Walks every top-level FnDecl body, finds nested
/// `function f() {...}` declarations inside any block-shape stmt,
/// lifts them to top-level with mangled names
/// `__nested_<parent>_<name>_<uid>`. References to the original
/// name within the parent body get rewritten to the mangled name.
///
/// Captures: this pass only handles the no-capture case. If the
/// nested fn body references parent locals, ssa_lower will fail at
/// the lifted FnDecl's body lower (lifted = top-level scope, can't
/// see parent locals). Real closure-capturing nested fns need the
/// same treatment as arrow fns (lift_arrow_fns adds an `__env`
/// first param + Closure shape) — substrate followup if test262
/// surfaces it.
pub fn desugar_nested_fns(ast: &mut Ast) {
    // r283 — fixpoint over the single-round pass: a lifted FnDecl can
    // itself contain a nested FnDecl (`function a() { function b() {
    // function c() {} } }` — the t262 async-case tail's `assertions`
    // / `$DONE` pair, 120-case cluster). Each round only walks
    // top-level decls, so freshly-lifted bodies get their own round;
    // a round that lifts nothing is the (idempotent) fixed point.
    loop {
        let before = ast.stmts.len();
        desugar_nested_fns_once(ast);
        if ast.stmts.len() == before {
            break;
        }
    }
}

fn desugar_nested_fns_once(ast: &mut Ast) {
    let mut top = std::mem::take(&mut ast.stmts);
    let mut new_top: Vec<Stmt> = Vec::new();
    let mut ctx = LiftCtx::new(ast);
    // §B.3.3.3 — a module-top declaration a block of the module also
    // declares keeps its body under a mangled name and hands its value
    // to a head `var` (`nested_fns_top`); the walk below then treats
    // the name as declared and the block's declaration takes the write
    // form. Before the walk, because the block may precede the
    // declaration.
    let (top_hoisted, top_hoisted_names) = super::nested_fns_top::lift_top_fns_over_block_fns(
        &mut top,
        ast,
        ast.sloppy_script_goal,
        &ast.source_dunder_idents,
        &mut ctx,
    );
    // The module scope's own function declarations, read before the
    // walk starts mutating their bodies.
    let top_declared = own_fn_names(&top);
    // Pass 1 — top-level FnDecl bodies. Walk nested fns inside parent
    // FnDecl bodies (the original P3.4 scope).
    for stmt in top.iter_mut() {
        if let Stmt::FnDecl {
            name: parent_name,
            body,
            params,
            ..
        } = stmt
        {
            let parent = parent_name.clone();
            // A function body's own declarations do not survive this
            // pass — it lifts and renames them — so by default none of
            // them can take the §B.3.3 write. The exception is a name
            // the body ALSO declares inside a block: there the write has
            // to land somewhere, so the body-level declaration becomes a
            // hoisted var of its own and the write updates it.
            // Parameters bind either way, and §B.3.3.1 step 1.a.iii
            // skips the name when they do.
            let own = own_fn_names(body);
            let nested_names = block_nested_fn_names(body);
            ctx.declared = own.intersection(&nested_names).cloned().collect();
            ctx.shadowed = own.difference(&nested_names).cloned().collect();
            ctx.params = param_names(params);
            ctx.var_shared = super::nested_fns_top::var_shared_fn_names(ast, body);
            ctx.catch_renames.clear();
            ctx.lex.clear();
            let mut renames: HashMap<String, String> = HashMap::new();
            let mut branch_scoped: HashSet<String> = HashSet::new();
            let mut lifted: Vec<Stmt> = Vec::new();
            body_lift::collect_fn_body_lifts(
                &ast.exprs,
                body,
                &parent,
                &mut renames,
                &mut branch_scoped,
                &mut lifted,
                &mut ctx,
            );
            ctx.flush(ast);
            if !renames.is_empty() {
                // A bare-branch decl (`if (c) function f() {}`) is
                // block-scoped in TS semantics: outer references keep
                // the ORIGINAL name (they resolve nowhere and ride the
                // catchable-ReferenceError undeclared-read lane, which
                // is exactly what bun answers). Only the lifted body
                // itself sees the mangled name (self-recursion).
                let outer_map: HashMap<String, String> = renames
                    .iter()
                    .filter(|(k, _)| !branch_scoped.contains(*k))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                // By scope: a block, loop head, catch clause or
                // nested function that rebinds a lifted name keeps its
                // own references.
                if !outer_map.is_empty() {
                    rename_in_owner(ast, body, &outer_map);
                }
                // Each lifted declaration is a function scope of its
                // own, whose parameters and locals hide the names too.
                rename_in_owner(ast, &mut lifted, &renames);
            }
            // After the mangling, so the references left under a
            // renamed catch parameter's original name are the ones that
            // meant the parameter.
            apply_after_lift(ast, body, &mut ctx);
            new_top.extend(lifted);
        }
    }
    // P3.4-followup-A — module-top-level blocks. `{ function f() {} }`
    // at module scope (outside any FnDecl) puts a FnDecl inside a
    // Stmt::Block, which the synthesized `main` body walks. Without
    // this pass, ssa_lower's lower_top_stmt catch-all panicked on
    // every such case (annexB function-statement hoisting tests).
    // Same shape as Pass 1 but the "parent" is the synthetic "__top"
    // namespace for mangling. Also handles If / While / DoWhile /
    // For / ForOf / ForIn / Try / Switch nested at module top.
    let parent = "__top".to_string();
    // A module-top `function f` survives the pass and can take the
    // write — but only once the walk has passed it. One still ahead
    // gets hoisted over anything written before it, and
    // `widen_rebound_fn_decls` declines to give a slot to a name read
    // before its declaration, so the write would have nowhere to land.
    ctx.declared = top_hoisted_names;
    ctx.shadowed = top_declared;
    ctx.params = HashSet::new();
    ctx.var_shared.clear();
    ctx.catch_renames.clear();
    ctx.lex.clear();
    let top_mark = ctx.lex.enter_scope(&top);
    let mut top_renames: HashMap<String, String> = HashMap::new();
    let mut top_branch_scoped: HashSet<String> = HashSet::new();
    let mut top_lifted: Vec<Stmt> = Vec::new();
    for stmt in top.iter_mut() {
        match stmt {
            Stmt::FnDecl { name, .. } => {
                // Handled by pass 1; the walk has now passed it.
                ctx.shadowed.remove(name);
                ctx.declared.insert(name.clone());
            }
            other => {
                collect_nested_fns_in_stmt(
                    other,
                    &parent,
                    &mut top_renames,
                    &mut top_branch_scoped,
                    LiftMode {
                        in_fn_body: false,
                        nested: false,
                    },
                    &mut top_lifted,
                    &mut ctx,
                );
            }
        }
    }
    ctx.lex.leave(top_mark);
    ctx.flush(ast);
    if !top_renames.is_empty() {
        // Rewrite ident references in the entire top-level (excluding
        // the lifted decls themselves; those get rewritten below).
        // Branch-scoped names stay un-rewritten outside — see pass 1.
        let outer_map: HashMap<String, String> = top_renames
            .iter()
            .filter(|(k, _)| !top_branch_scoped.contains(*k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        if !outer_map.is_empty() {
            for stmt in top.iter_mut() {
                rewrite_idents_in_stmt(ast, stmt, &outer_map, false);
            }
        }
        rename_in_owner(ast, &mut top_lifted, &top_renames);
    }
    apply_after_lift(ast, &mut top, &mut ctx);
    new_top.extend(top_lifted);
    // Pass 3 — function-expression bodies, which are neither of the two
    // scopes above. Runs while those bodies are still whole: after
    // `lift_arrow_fns` each one is a top-level FnDecl and pass 1 does
    // reach it, but by then the body's other function values have been
    // lifted out too and the rename cannot follow them.
    super::nested_fns_fnexpr::lift_in_fn_expr_bodies(ast, &mut ctx, &mut new_top);
    // At the HEAD: a module-top function is initialized before the
    // first statement runs (§16.1.7 step 16).
    top.splice(0..0, top_hoisted);
    top.extend(new_top);
    ast.stmts = top;
}

/// The declaration's own source range, or the `(0, 0)` sentinel for a
/// statement that is not a `FnDecl` (the callers only ask about one).
fn fn_decl_span(stmt: &Stmt) -> crate::lexer::Span {
    match stmt {
        Stmt::FnDecl { span, .. } => *span,
        _ => crate::lexer::Span { start: 0, end: 0 },
    }
}

pub(super) fn collect_nested_fns_to_lift(
    body: &mut Vec<Stmt>,
    parent_name: &str,
    renames: &mut HashMap<String, String>,
    branch_scoped: &mut HashSet<String>,
    mode: LiftMode,
    lifted: &mut Vec<Stmt>,
    ctx: &mut LiftCtx,
) {
    let drained = std::mem::take(body);
    let mark = ctx.lex.enter_scope(&drained);
    if mode.nested {
        ctx.lex.enter_block_fns(&drained, mark);
    }
    let mut new_body: Vec<Stmt> = Vec::with_capacity(drained.len());
    // Writes for the body-level declarations that keep a binding — see
    // `body_level_var_lift`. They go at the HEAD of the list because a
    // function declaration is initialized on scope entry, before any
    // statement of the body runs.
    let mut hoisted: Vec<Stmt> = Vec::new();
    // Block bindings for the strict-mode lifts of THIS list, keyed by
    // name so a legal re-declaration replaces rather than repeats.
    let mut strict_block: Vec<(String, String)> = Vec::new();
    for s in drained {
        match s {
            Stmt::FnDecl { ref name, .. }
                if !name.starts_with("__closure_")
                    && !name.starts_with("__cm_")
                    && !name.starts_with("__sm_")
                    && !name.starts_with("__nested_") =>
            {
                let mangled = format!("__nested_{parent_name}_{name}_{}", ctx.counter);
                ctx.counter += 1;
                ctx.source_names.push((mangled.clone(), name.clone()));
                renames.insert(name.clone(), mangled.clone());
                // Same §B.3.3 split as the bare-branch arm above: in
                // sloppy code the vacated slot becomes the var write and
                // outer references stay on the un-mangled name.
                if !mode.nested && ctx.body_level_var_lift(name, fn_decl_span(&s)) {
                    branch_scoped.insert(name.clone());
                    let decl = ctx.mint_var_decl(name, &mangled);
                    match ctx.from_sloppy_eval(name, fn_decl_span(&s)) {
                        true => new_body.push(decl),
                        false => hoisted.push(decl),
                    }
                }
                match ctx.annexb(name, fn_decl_span(&s), mode) {
                    AnnexB::Apply => {
                        branch_scoped.insert(name.clone());
                        let decl = ctx.annexb_var_decl(name, &mangled);
                        new_body.push(decl);
                    }
                    // No var binding — another owner holds the name
                    // outside the block (Skip) or the code is strict —
                    // so the binding is this list's, minted at its
                    // HEAD because a function declaration is hoisted
                    // within its block. `branch_scoped` keeps the
                    // enclosing body's references off the lifted name;
                    // this binding is what the block's own references
                    // resolve to.
                    AnnexB::Skip | AnnexB::Strict => {
                        branch_scoped.insert(name.clone());
                        // ONE binding per name, and the last
                        // declaration wins: two plain `function f` in
                        // one block is legal (§B.3.3.4 — bun accepts,
                        // and `block-redecl-legal-001` measures it), so
                        // minting one binding each would answer
                        // "redeclaration of `f`" for a legal program.
                        match strict_block.iter_mut().find(|(n, _)| n == name) {
                            Some(slot) => slot.1 = mangled.clone(),
                            None => strict_block.push((name.clone(), mangled.clone())),
                        }
                    }
                    AnnexB::Legacy => {}
                }
                let new_decl = match s {
                    Stmt::FnDecl {
                        type_params,
                        params,
                        return_type,
                        body: fbody,
                        is_generator,
                        span,
                        ..
                    } => Stmt::FnDecl {
                        name: mangled,
                        type_params,
                        params,
                        return_type,
                        body: fbody,
                        is_generator,
                        // lift keeps the user's declaration text (B1b)
                        span,
                    },
                    _ => unreachable!(),
                };
                lifted.push(new_decl);
                // Original site: drop entirely.
            }
            mut other => {
                // RFC 20260801 (rotation 269) — pass 1 now walks the
                // same collector as the module-top pass: the old
                // sibling walk (`collect_nested_fns_in_other`,
                // deleted) lacked the bare-FnDecl-branch arm, so a
                // braceless `if (c) function f() {}` INSIDE a fn
                // body (annexB B.3.2's most common shape, usually
                // under an IIFE) reached the lowering catch-all
                // panic while the identical shape at module top
                // lifted fine.
                collect_nested_fns_in_stmt(
                    &mut other,
                    parent_name,
                    renames,
                    branch_scoped,
                    mode,
                    lifted,
                    ctx,
                );
                new_body.push(other);
            }
        }
    }
    ctx.lex.leave(mark);
    // At the HEAD, because a function declaration is hoisted within
    // its block: a call written above it has to resolve.
    let minted: Vec<Stmt> = strict_block
        .iter()
        .map(|(n, m)| ctx.mint_block_decl(n, m))
        .collect();
    hoisted.splice(0..0, minted);
    if hoisted.is_empty() {
        *body = new_body;
    } else {
        hoisted.extend(new_body);
        *body = hoisted;
    }
}
