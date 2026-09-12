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

use super::Stmt;
use super::annexb_fn_var::LiftCtx;
use super::annexb_names::block_nested_fn_names;
use std::collections::HashSet;

/// Rename in place every eligible module-top declaration and mint its
/// head `var`. Answers the hoisted decls (for the head of `top`) and
/// the names, which seed the module-top walk's `declared` so the
/// block's declaration takes the write form.
pub(super) fn lift_top_fns_over_block_fns(
    top: &mut [Stmt],
    ctx: &mut LiftCtx,
) -> (Vec<Stmt>, HashSet<String>) {
    let nested = block_nested_fn_names(top);
    let mut hoisted = Vec::new();
    let mut names = HashSet::new();
    for stmt in top.iter_mut() {
        let Stmt::FnDecl { name, span, .. } = stmt else {
            continue;
        };
        if name.starts_with("__") || !nested.contains(name) || !ctx.applies(name, *span) {
            continue;
        }
        let mangled = format!("__nested___top_{name}_{}", ctx.counter);
        ctx.counter += 1;
        ctx.source_names.push((mangled.clone(), name.clone()));
        hoisted.push(ctx.mint_var_decl(name, &mangled));
        names.insert(name.clone());
        *name = mangled;
    }
    (hoisted, names)
}
