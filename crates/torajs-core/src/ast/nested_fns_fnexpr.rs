//! A nested `function` declaration whose enclosing scope is a function
//! EXPRESSION body, not a top-level `FnDecl` body.
//!
//! ```text
//! (function () {
//!   function F() { return 7 }
//!   const t = () => F();     // bun: 7, tr: unknown identifier `F`
//!   console.log(t());
//! })()
//! ```
//!
//! [`super::nested_fns`] has two passes: one over top-level `FnDecl`
//! bodies, one over module-top-level statements. A function expression's
//! body is neither, so nothing collects the declarations inside it —
//! until `lift_arrow_fns` turns that body into a top-level `FnDecl` of
//! its own, and pass 1 picks it up on the pipeline's second call.
//!
//! By then it is too late for the rewrite. The lift has also moved every
//! other function value in that body out to top level, so `t` above is a
//! separate declaration and the rename of `F` — which walks the parent
//! body — cannot reach the reference inside it. The declaration is
//! renamed, the reader is not, and the read goes to a name nothing
//! declares. Where the name collides with a builtin the failure is
//! silent instead: the read finds the builtin.
//!
//! This pass is the missing scope. It runs on the pipeline's FIRST call,
//! while the body is still whole, and does per function-expression body
//! exactly what pass 1 does per top-level `FnDecl` body. `t` is a
//! `LetDecl` init sitting in that body at the time, so the rename
//! reaches it.
//!
//! Only declarations that do not capture reach here:
//! `desugar_capturing_nested_fns` runs earlier and rewrites a capturing
//! one into `let <name> = <function expression>`, which is the closure
//! lane and never lifts. What is left is what pass 1 already lifts
//! safely out of a top-level body — the lift to top level is sound for
//! the same reason there.

use super::annexb_fn_var::{LiftCtx, LiftMode};
use super::annexb_names::{block_nested_fn_names, own_fn_names, param_names};
use super::desugar_async_nested_fns::rename::rename_in_owner;
use super::nested_fns::collect_nested_fns_to_lift;
use super::nested_fns_catch::apply_after_lift;
use super::{Ast, Expr, Stmt};
use std::collections::{HashMap, HashSet};

/// Walk every function-expression body in the arena, lifting the nested
/// declarations inside it. Appends what it lifts to `new_top`; the
/// caller adds that to the module's statement list.
pub(super) fn lift_in_fn_expr_bodies(ast: &mut Ast, ctx: &mut LiftCtx, new_top: &mut Vec<Stmt>) {
    // A flush appends to the arena, so the bound is read once: what the
    // flush mints is an Annex B write, never a declaration, and the
    // fixpoint round would walk it anyway.
    let arena_len = ast.exprs.len();
    for i in 0..arena_len {
        let (params, mut body) = match &mut ast.exprs[i] {
            Expr::ArrowFn { params, body, .. } => {
                let own = own_fn_names(body);
                let nested = block_nested_fn_names(body);
                if own.is_empty() && nested.is_empty() {
                    continue;
                }
                (params.clone(), std::mem::take(body))
            }
            _ => continue,
        };
        // The mangle needs a parent name and a function expression has
        // none. The arena index is one: stable within the round, unique
        // across bodies, and not a spelling any source can collide with.
        let parent = format!("__fnexpr{i}");
        lift_one_body(ast, ctx, &parent, &params, &mut body, new_top);
        if let Expr::ArrowFn { body: slot, .. } = &mut ast.exprs[i] {
            *slot = body;
        }
    }
}

/// Pass 1's body, against a function expression's body instead of a
/// top-level `FnDecl`'s. Kept in step with it deliberately — the two
/// answer the same question about two spellings of the same scope.
fn lift_one_body(
    ast: &mut Ast,
    ctx: &mut LiftCtx,
    parent: &str,
    params: &[super::Param],
    body: &mut Vec<Stmt>,
    new_top: &mut Vec<Stmt>,
) {
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
    // the declarations the lift mints at the head go below the
    // directive prologue, which has to stay the body's head
    let prologue_len = super::sloppy_this_prologue::directive_prologue_len(body, &ast.exprs);
    let prologue: Vec<Stmt> = body.drain(..prologue_len).collect();
    collect_nested_fns_to_lift(
        body,
        parent,
        &mut renames,
        &mut branch_scoped,
        LiftMode {
            in_fn_body: true,
            nested: false,
        },
        &mut lifted,
        ctx,
    );
    body.splice(0..0, prologue);
    ctx.flush(ast);
    if !renames.is_empty() {
        // Bare-branch declarations keep the original name outside the
        // lifted body, same as pass 1 — see its comment for why.
        let outer_map: HashMap<String, String> = renames
            .iter()
            .filter(|(k, _)| !branch_scoped.contains(*k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        if !outer_map.is_empty() {
            rename_in_owner(ast, body, &outer_map);
        }
        rename_in_owner(ast, &mut lifted, &renames);
    }
    apply_after_lift(ast, body, ctx);
    new_top.extend(lifted);
}
