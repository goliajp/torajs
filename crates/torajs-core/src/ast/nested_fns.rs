//! Nested-fn hoisting pass — chunk 358, extracted from ast.rs.
//!
//! Pub entry `desugar_nested_fns` implements ES Annex B §B.3.3
//! (FunctionDeclarations in IfStatement Blocks). Walks every top-
//! level FnDecl body + module-top-level non-FnDecl stmts, finds
//! nested `function f() {...}` decls inside any block-shape stmt,
//! lifts them to top-level with mangled names
//! `__nested_<parent>_<name>_<uid>`, rewrites references. The
//! private helpers (`collect_nested_fns_in_stmt/to_lift`,
//! `rewrite_idents_in_body/stmt/expr` in the idents sibling) do the
//! tree walk — both passes share ONE stmt collector since rotation
//! 269 (the pass-1-only sibling lacked the bare-FnDecl-branch arm).

use super::annexb_fn_var::{AnnexB, LiftCtx, LiftMode};
use super::annexb_names::{block_nested_fn_names, own_fn_names, param_names};
use super::nested_fns_catch::apply_catch_renames;
use super::nested_fns_idents::{rewrite_idents_in_body, rewrite_idents_in_stmt};
use super::nested_fns_walk::descend_into_children;
use super::{Ast, Stmt};
use std::collections::{HashMap, HashSet};

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
            ctx.catch_renames.clear();
            ctx.reset_scopes();
            let mut renames: HashMap<String, String> = HashMap::new();
            let mut branch_scoped: HashSet<String> = HashSet::new();
            let mut lifted: Vec<Stmt> = Vec::new();
            collect_nested_fns_to_lift(
                body,
                &parent,
                &mut renames,
                &mut branch_scoped,
                LiftMode {
                    in_fn_body: true,
                    nested: false,
                },
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
                if !outer_map.is_empty() {
                    rewrite_idents_in_body(ast, body, &outer_map, true);
                }
                for lf in lifted.iter_mut() {
                    if let Stmt::FnDecl { body: lb, .. } = lf {
                        rewrite_idents_in_body(ast, lb, &renames, true);
                    }
                }
            }
            // After the mangling, so the references left under a
            // renamed catch parameter's original name are the ones that
            // meant the parameter.
            apply_catch_renames(ast, body, &ctx.catch_renames);
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
    ctx.declared = HashSet::new();
    ctx.shadowed = top_declared;
    ctx.params = HashSet::new();
    ctx.catch_renames.clear();
    ctx.reset_scopes();
    let top_mark = ctx.enter_scope(&top);
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
    ctx.leave_scope(top_mark);
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
        for lf in top_lifted.iter_mut() {
            if let Stmt::FnDecl { body: lb, .. } = lf {
                rewrite_idents_in_body(ast, lb, &top_renames, true);
            }
        }
    }
    apply_catch_renames(ast, &mut top, &ctx.catch_renames);
    new_top.extend(top_lifted);
    top.extend(new_top);
    ast.stmts = top;
}

/// P3.4-followup-A — recursive helper that descends into a single stmt
/// looking for nested FnDecls (block/if/while/for/try/switch
/// children). When found, lift to `lifted` with mangled name and
/// drop from the original site (replaced with a no-op Block).
pub(super) fn collect_nested_fns_in_stmt(
    stmt: &mut Stmt,
    parent_name: &str,
    renames: &mut HashMap<String, String>,
    branch_scoped: &mut HashSet<String>,
    mode: LiftMode,
    lifted: &mut Vec<Stmt>,
    ctx: &mut LiftCtx,
) {
    // Bare-FnDecl-as-statement: `if (cond) function f() {}` parses
    // the then-branch as Stmt::FnDecl directly (no enclosing Block).
    // Same lift-and-mangle as the Block case but in-place.
    if let Stmt::FnDecl { name, span, .. } = stmt {
        if !name.starts_with("__closure_")
            && !name.starts_with("__cm_")
            && !name.starts_with("__sm_")
            && !name.starts_with("__nested_")
        {
            let mangled = format!("__nested_{parent_name}_{name}_{}", ctx.counter);
            ctx.counter += 1;
            renames.insert(name.clone(), mangled.clone());
            // §B.3.3 in SLOPPY code: the outer name is a `var` binding
            // of its own, written where the declaration sits — so the
            // outer references must NOT be redirected to the mangled
            // block binding, and the slot the declaration vacates holds
            // that write. Strict code has no var binding: TS semantics
            // INSIDE a fn body (strict_branch) keep the block-scoped
            // reject (bun answers ReferenceError there), and strict
            // MODULE-TOP keeps the historical leak-to-parent rewrite.
            // The strict split is an S5.7 boundary-decision entry.
            // A declaration reached in this position is the branch of
            // an `if` / a loop / a label — a block by construction,
            // whatever the caller's own depth was.
            let verdict = ctx.annexb(name, *span, mode.descend());
            // A BARE-branch declaration (`if (c) function f() {}`) is
            // §B.3.2's shape, not §14.2.10's, and it keeps the answer
            // it had: there is no block here for a binding of its own
            // to live in. `AnnexB::Strict` therefore reads as `Legacy`
            // on this path.
            //
            // It also cannot be decided from the program's goal alone.
            // An INDIRECT eval's code is global SLOPPY code whatever
            // the calling program was (§19.2.1.1 step 3), so
            // `(0, eval)("if (true) function ib(){}")` inside a module
            // wants Annex B — and the eval's own goal is not something
            // this pass can see: `desugar_eval` blanks every spliced
            // declaration's span to the same `(0, 0)` sentinel every
            // synthesized declaration carries, so there is nothing to
            // ask. Same missing marker as the §B.3.3.3 parameterNames
            // item; both wait on the same design.
            if !matches!(verdict, AnnexB::Legacy | AnnexB::Strict) || mode.in_fn_body {
                branch_scoped.insert(name.clone());
            }
            let slot = match verdict {
                AnnexB::Apply => ctx.annexb_var_decl(name, &mangled),
                AnnexB::Skip | AnnexB::Strict | AnnexB::Legacy => Stmt::Block(Vec::new()),
            };
            // Take the FnDecl out of the slot, push the renamed decl
            // into `lifted`.
            let taken = std::mem::replace(stmt, slot);
            if let Stmt::FnDecl {
                type_params,
                params,
                return_type,
                body,
                is_generator,
                span,
                ..
            } = taken
            {
                lifted.push(Stmt::FnDecl {
                    name: mangled,
                    type_params,
                    params,
                    return_type,
                    body,
                    is_generator,
                    // lift keeps the user's declaration text (B1b)
                    span,
                });
            }
            return;
        }
    }
    descend_into_children(stmt, parent_name, renames, branch_scoped, mode, lifted, ctx);
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
    let mark = ctx.enter_scope(&drained);
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
                renames.insert(name.clone(), mangled.clone());
                // Same §B.3.3 split as the bare-branch arm above: in
                // sloppy code the vacated slot becomes the var write and
                // outer references stay on the un-mangled name.
                if !mode.nested && ctx.body_level_var_lift(name, fn_decl_span(&s)) {
                    branch_scoped.insert(name.clone());
                    let decl = ctx.mint_var_decl(name, &mangled);
                    hoisted.push(decl);
                }
                match ctx.annexb(name, fn_decl_span(&s), mode) {
                    AnnexB::Apply => {
                        branch_scoped.insert(name.clone());
                        let decl = ctx.annexb_var_decl(name, &mangled);
                        new_body.push(decl);
                    }
                    // Another owner holds the name outside the block,
                    // so outer references belong to it and must not
                    // follow the lift.
                    AnnexB::Skip => {
                        branch_scoped.insert(name.clone());
                    }
                    // Nothing outside the block holds the name at all,
                    // so the binding is this list's — minted at its
                    // HEAD, because a function declaration is hoisted
                    // within its block. `branch_scoped` keeps the
                    // enclosing body's references off the lifted name;
                    // this binding is what the block's own references
                    // resolve to.
                    AnnexB::Strict => {
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
    ctx.leave_scope(mark);
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
