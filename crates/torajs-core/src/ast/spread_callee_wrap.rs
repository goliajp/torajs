//! Dynamic-spread callee wrap (RFC 20260815-fn-value-rest-spread,
//! knife 3 second half — the sp5 residue).
//!
//! A spread-carrying call whose bare-Ident callee names a top-level
//! FnDecl normally never reaches the runtime spread lane: the static
//! expanders own the direct-call shapes (`apply_rest_args` packs any
//! rest-callee site with enough required args; `apply_spread_args`
//! index-expands the single-trailing-spread fixed-arity shape). The
//! shapes they decline — a NON-trailing spread (`four(1, ...mid,
//! 4)`), multiple spreads, a non-Ident spread source against a
//! fixed-arity callee, a rest callee starved of its required prefix —
//! used to die loud at `box_to_any(FnSig)` in the runtime lane's
//! callee boxing (the rotation 372 recorded boundary).
//!
//! This pass rewrites exactly those declined callees to their
//! canonical closure value (`Closure { __forward_<name>, [] }`, the
//! fn-to-closure forwarder machinery), so the runtime spread kernel
//! receives a boxable callee and dispatches through the boxed dual
//! entry, which feeds the true runtime argc (a rest callee re-packs
//! its tail there — the knife-2 adapter).
//!
//! Ordering is load-bearing three ways:
//! - AFTER `desugar_arguments_object`: a callee that reads argument
//!   VALUES must NOT wrap (the forwarder relays only the declared
//!   params, so those values die in the relay — the ba9095b4
//!   lesson); the side-tables consulted here are filled by that pass.
//!   A LENGTH-only head-less callee does wrap since rotation 416: the
//!   relay carries its own runtime argc into the callee's hidden slot
//!   (RFC 20260815 blade 5), so the count survives the hop, and the
//!   static expanders decline those sites on purpose — leaving them
//!   unwrapped meant both passes stood aside and the bare fn name
//!   died loud in the runtime lane's callee boxing.
//! - AFTER `apply_default_args`: a defaulted-param callee must NOT
//!   wrap either (defaults are call-site-substituted by the static
//!   expanders; the dynamic lane has no default story), and the skip
//!   below keeps such shapes on the checker's loud reject.
//! - BEFORE `apply_rest_args` / `apply_spread_args`: the wrap gate
//!   mirrors their accept conditions, standing aside wherever a
//!   static expander will take the site (wrapping those hijacked the
//!   known-arity direct calls into the dynamic lane — the rotation
//!   372 six-red). A wrapped callee is a Closure literal, which both
//!   expanders skip, so no site is processed twice.
//!
//! Synthetic callees (`__new_*` factories, `__cm_*` super chains —
//! any `__` prefix) stay out entirely: the static expanders are the
//! semantic owners of their shapes, and an arity-0 derived-class
//! factory wrapped here would flip its loud reject into
//! silently-uninitialized fields.

use super::apply_args::peel_hidden_params;
use super::forwarders_object::{snapshot_fn_sigs, synthesize_forwarder_decls};
use super::{Ast, Expr, ExprId};
use std::collections::HashSet;

pub fn wrap_dynamic_spread_callees(ast: &mut Ast) {
    let (fn_sigs, existing_forwarders, _fn_type_params) = snapshot_fn_sigs(ast);
    if fn_sigs.is_empty() {
        return;
    }
    let mut targets: HashSet<String> = HashSet::new();
    let mut rewrites: Vec<(ExprId, String)> = Vec::new();
    for i in 0..ast.exprs.len() {
        let Expr::Call { callee, args } = &ast.exprs[i] else {
            continue;
        };
        let callee = *callee;
        let name = match ast.get_expr(callee) {
            Expr::Ident(n) if !n.starts_with("__") => n.clone(),
            _ => continue,
        };
        if !args
            .iter()
            .any(|a| matches!(ast.get_expr(*a), Expr::Spread { .. }))
        {
            continue;
        }
        let Some((params, _, _)) = fn_sigs.get(&name) else {
            continue;
        };
        // Argument VALUES still have no channel through the
        // forwarder's declared-param relay, so those callees keep the
        // loud reject. The head-less argc tier no longer does: since
        // the `__forward_` relay carries its OWN runtime argc into a
        // head-less callee's hidden slot (RFC 20260815 blade 5), a
        // spread site's true count reaches the body — the relay is
        // fed by the boxed adapter, which knows it.
        if ast.closure_argv_fns.contains(&name)
            || ast.headless_argv_touch_fns.contains(&name)
            || ast.closure_argc_locals.contains(&name)
            || ast.closure_argv_locals.contains(&name)
        {
            continue;
        }
        let user = peel_hidden_params(params);
        // Defaulted params are call-site substitutions the dynamic
        // lane cannot replay; such shapes keep the loud reject.
        if user.iter().any(|p| p.default.is_some()) {
            continue;
        }
        // …but only when it really will. `apply_spread_args` skips an
        // arguments-carrying callee on purpose (its index-read
        // expansion trims the list to the declared arity and the real
        // argc dies with the trimmed tail), so for those sites the
        // shape test below answers yes about a pass that is about to
        // decline. Left to itself that gap put a bare fn name on the
        // runtime spread lane, which cannot box it — a loud
        // "box_to_any element type FnSig" where the two passes each
        // assumed the other had it.
        if !ast.headless_argc_fns.contains(&name) && static_expander_takes(ast, args, user) {
            continue;
        }
        targets.insert(name.clone());
        rewrites.push((callee, name));
    }
    if rewrites.is_empty() {
        return;
    }
    let (new_decls, renames) =
        synthesize_forwarder_decls(ast, &targets, &fn_sigs, &existing_forwarders);
    for (eid, target) in rewrites {
        if let Some(forward_name) = renames.get(&target) {
            ast.exprs[eid.0 as usize] = Expr::Closure {
                fn_name: forward_name.clone(),
                captures: Vec::new(),
            };
        }
    }
    ast.stmts.extend(new_decls);
}

/// Mirror of the static expanders' accept conditions — `true` means
/// a later pass will consume this site, so the wrap stands aside.
fn static_expander_takes(ast: &Ast, args: &[ExprId], user: &[super::Param]) -> bool {
    if user.last().is_some_and(|p| p.is_rest) {
        // `apply_rest_args` packs any site with the required prefix
        // covered (its trailing args bundle into the rest array,
        // spreads included via the array-literal / Array.from arms).
        return args.len() >= user.len() - 1;
    }
    // `apply_spread_args` takes exactly one spread, at the last
    // position, over an Ident source, with the fixed prefix inside
    // the declared arity.
    let spread_count = args
        .iter()
        .filter(|a| matches!(ast.get_expr(**a), Expr::Spread { .. }))
        .count();
    if spread_count != 1 {
        return false;
    }
    let Some((&last, prefix)) = args.split_last() else {
        return false;
    };
    let Expr::Spread { expr } = ast.get_expr(last) else {
        return false;
    };
    matches!(ast.get_expr(*expr), Expr::Ident(_)) && prefix.len() <= user.len()
}
