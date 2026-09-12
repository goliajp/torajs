//! Annex B §B.3.3.3 at module top — a `function f` the module declares
//! AND a block of the module declares.
//!
//! §16.1.7 step 16 puts the module-top declaration's value into the
//! global binding `f` before the first statement runs; §B.3.3.3 makes
//! the block's evaluation a `SetMutableBinding` on that SAME binding.
//! So `f()` before the block is the module-top function, and after it
//! the block's.
//!
//! The lift's module-top walk used to hold such a declaration in
//! `shadowed` while it was still ahead, answer `Legacy` for the block,
//! and redirect every outer `f` to the BLOCK function — the module-top
//! declaration became dead code (`globalThis.f === f` went false with
//! it). The one reason recorded for that was `widen_rebound_fn_decls`
//! declining a slot to a name read before its declaration, so the
//! block's write had nowhere to land.
//!
//! This is the module-top half of what pass 1 already does for a
//! function body (`LiftCtx::body_level_var_lift`): the declaration
//! keeps its body under a mangled name — a module-top declaration is
//! hoisted, its position carries nothing — and `var f: any =
//! <mangled>` at the HEAD of the module holds the value from the first
//! statement on. The block's write then updates it, and `f` becomes a
//! written `var`, which is the global-object property with the
//! attributes CreateGlobalFunctionBinding(D = false) gives
//! (`{writable, enumerable, not configurable}`, §9.1.1.4.18). The
//! mangled name is a synthesized dunder, so `script_global_fns` gives
//! it no property of its own.
//!
//! The same form serves a `var f = e` anywhere in the module over a
//! `function f` (§16.1.7 step 8 / 10: one binding, the function's
//! value first, then whatever the `var`'s initializer stores when it
//! runs) — a user's own `var f = 1`, or the `var f = <block binding>`
//! the capturing lane leaves behind for a block function that reads a
//! module-top `let` (`split_block_binding`, which the block-nested
//! scan above cannot see: by then the declaration is a `let`). The var
//! hoist used to keep a prelude slot for that name, and the slot
//! shadowed the function until the `var` ran. An init-less `var f;`
//! re-declares the function's binding and mints nothing there, so it
//! is not a reason to take the declaration here either.

use super::annexb_fn_var::LiftCtx;
use super::annexb_names::block_nested_fn_names;
use super::{Expr, ExprId, Stmt};
use std::collections::HashSet;

/// Rename in place every eligible module-top declaration and mint its
/// head `var`. Answers the hoisted decls (for the head of `top`) and
/// the names, which seed the module-top walk's `declared` so the
/// block's declaration takes the write form.
///
/// `script_goal`: a `var` over a `function` is one binding only under
/// the Script goal (§16.1.7 step 8); a module names it twice, and the
/// checker's duplicate-declaration reject must still see both.
pub(super) fn lift_top_fns_over_block_fns(
    top: &mut [Stmt],
    exprs: &[Expr],
    script_goal: bool,
    ctx: &mut LiftCtx,
) -> (Vec<Stmt>, HashSet<String>) {
    let nested = block_nested_fn_names(top);
    let var_init = if script_goal {
        var_names_with_init(top, exprs)
    } else {
        HashSet::new()
    };
    let mut hoisted = Vec::new();
    let mut names = HashSet::new();
    for stmt in top.iter_mut() {
        let Stmt::FnDecl { name, span, .. } = stmt else {
            continue;
        };
        if name.starts_with("__") {
            continue;
        }
        let block_fn = nested.contains(name) && ctx.applies(name, *span);
        if !block_fn && !var_init.contains(name) {
            continue;
        }
        let mangled = format!("__nested___top_{name}_{}", ctx.counter);
        ctx.counter += 1;
        ctx.source_names.push((mangled.clone(), name.clone()));
        // The slot and the write as two statements (see the mint): the
        // one form the var hoist keeps as an `any` slot whether or not
        // a block of the module declares the name again.
        hoisted.extend(ctx.mint_var_slot_and_write(name, &mangled));
        names.insert(name.clone());
        *name = mangled;
    }
    (hoisted, names)
}

/// The names the module's `var` sites declare WITH an initializer, at
/// any depth the hoist walks (`count_var_names`: blocks and branches,
/// not function bodies).
fn var_names_with_init(top: &[Stmt], exprs: &[Expr]) -> HashSet<String> {
    let mut counts: std::collections::HashMap<String, Vec<ExprId>> =
        std::collections::HashMap::new();
    super::var_hoist::count_var_names(top, &mut counts);
    counts
        .into_iter()
        .filter(|(_, sites)| {
            sites
                .iter()
                .any(|e| !matches!(exprs[e.0 as usize], Expr::Uninit))
        })
        .map(|(n, _)| n)
        .collect()
}
