//! The fourth `__forward_` site — a fn name bound to a variable and
//! then used as a VALUE.
//!
//! Three passes already relay a bare top-level fn name into its
//! canonical closure value when the name escapes: `synthesize_
//! forwarders` (a `return f` in a closure-typed position),
//! `synthesize_fn_to_closure_forwarders` (an ObjectLit field store)
//! and `wrap_dynamic_spread_callees` (a spread call's callee). Each
//! was written for the site that happened to break, and each reads
//! the escape off the EXPRESSION it sits in.
//!
//! `let f = g` is the site none of them cover. The binding itself is
//! not an escape — `let f = g; f(1)` stays a direct FnSig call, and
//! keeping it one is the whole point of the FnSig lane. What escapes
//! is the LOCAL: the moment `f` appears anywhere but a callee slot,
//! its `Type::FnSig` value reaches a boxing point that has no runtime
//! representation to box, and lowering refuses loud. Seven distinct
//! call sites spell that same refusal today (`ssa_lower_call_arg_conv`
//! / `_call_spread` / `_call_promise_keyed` / `_binop_inner` /
//! `_call_array_from` / `_ternary` / `_member`), which is what a
//! missing relay looks like from the inside: every consumer grew its
//! own copy of the same reject.
//!
//! So the gate is on the local, not on the initializer: wrap when
//! some arena `Ident(<local>)` sits outside a call/new callee slot,
//! leave the binding alone otherwise. `Expr::New` names its target as
//! a string and never mints an `Ident`, so only `NewDynamic` needs
//! naming here; counting its callee as a call slot keeps
//! `new f()` on the direct lane it needs.
//!
//! Pipeline: right after `synthesize_fn_to_closure_forwarders`, which
//! is where the other relays finish. `arguments_object_headless_argv`
//! states the invariant this closes — "value escapes are already
//! relayed through `__forward_` shims by the time this pass runs" —
//! and a `let`-bound escape was the one that wasn't.
//!
//! An ANNOTATED binding is left alone: `let f: () => number = g`
//! spells `__fn(...)`, which is exactly the FnSig repr, and swapping
//! in a Closure would put a closure cell in a slot typed for a
//! direct-dispatch signature.
//!
//! A target whose body touches `arguments` is wrapped whether or not
//! the local escapes. Its signature grows a hidden argc slot (and an
//! argv pointer on the value face) that the FnSig lane cannot see:
//! `const c = E.one; c(true)` on `static one(a: boolean) { …
//! arguments.length }` fed `true` into the count slot and read `a`
//! off nothing. The relay carries the true count; when every use of
//! the local is a direct call, `arguments_object_devirt` folds the
//! alias back into direct calls of the target, so those sites keep
//! the direct lane with the slot filled — the shape `var c = E.one`
//! already took through its declare-then-assign fold.

use super::forwarders_object::{snapshot_fn_sigs, synthesize_forwarder_decls};
use super::{Ast, Expr, ExprId, Param, Stmt};
use std::collections::{HashMap, HashSet};

pub fn wrap_escaping_fn_ident_bindings(ast: &mut Ast) {
    let (fn_sigs, existing_forwarders, _fn_type_params) = snapshot_fn_sigs(ast);
    if fn_sigs.is_empty() {
        return;
    }
    let mut targets: HashSet<String> = HashSet::new();
    let mut rewrites: Vec<(ExprId, String)> = Vec::new();
    for (eid, _, target) in escaping_fn_aliases(ast, &|g| fn_sigs.contains_key(g)) {
        targets.insert(target.clone());
        rewrites.push((eid, target));
    }
    // a function stored into a binding that holds cells (see the sibling)
    let cell_inits: HashSet<ExprId> = rewrites.iter().map(|(eid, _)| *eid).collect();
    let is_fn = |g: &str| fn_sigs.contains_key(g);
    let (wraps, shells_only) =
        super::fn_assign_rhs_wrap::closure_slot_store_leaves(ast, &is_fn, &cell_inits);
    for (eid, target) in wraps {
        targets.insert(target.clone());
        rewrites.push((eid, target));
    }
    targets.extend(shells_only);
    if targets.is_empty() {
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

/// The `let local = target` bindings [`wrap_escaping_fn_ident_bindings`]
/// relays, as (init, local, target), with `is_fn` naming the plain
/// function declarations. The closure-param tag pass, which runs
/// first, asks the same question to know which locals end up closure
/// cells: before its retag every fn-typed parameter still spells
/// `__fn(`, so a local handed only to those stays a bare pointer.
pub fn escaping_fn_aliases(
    ast: &Ast,
    is_fn: &dyn Fn(&str) -> bool,
) -> Vec<(ExprId, String, String)> {
    let mut cands: Vec<(ExprId, String, String)> = Vec::new();
    super::ns_alias::walk_stmts(&ast.stmts, &mut |s| {
        let Stmt::LetDecl {
            name,
            type_ann: None,
            init,
            ..
        } = s
        else {
            return;
        };
        let Expr::Ident(g) = ast.get_expr(*init) else {
            return;
        };
        // `let g = g` re-binds the name this pass would rewrite
        // through; leave that shadow alone.
        if g == name || !is_fn(g) || !relayable_target(g) {
            return;
        }
        cands.push((*init, name.clone(), g.clone()));
    });
    let mut out = Vec::new();
    let mut escaping: HashSet<String> = HashSet::new();
    let mut settled: HashSet<String> = HashSet::new();
    let fnsig_args = if cands.is_empty() {
        HashSet::new()
    } else {
        fnsig_param_args(ast)
    };
    for (eid, local, target) in cands {
        if settled.insert(local.clone())
            && (local_escapes(ast, &local, &fnsig_args) || target_touches_arguments(ast, &target))
        {
            escaping.insert(local.clone());
        }
        if escaping.contains(&local) {
            out.push((eid, local, target));
        }
    }
    out
}

/// Which synthetic names may be relayed. `desugar_classes` rewrites
/// `C.m` in place to a flat `__sm_C__m` Ident, so a static method
/// bound to a variable arrives here already spelled synthetic — and
/// it is an ordinary top-level fn, safe to relay. The other synthetic
/// FnDecls are not: `__new_C` factories and `__cm_` super chains are
/// owned by the static expanders (an arity-0 derived-class factory
/// relayed here would turn its loud reject into silently
/// uninitialized fields — the `wrap_dynamic_spread_callees` account),
/// and a static FIELD read (`__sf_C__x`) is a global, never in
/// `fn_sigs`. Accessors never reach this test at all: `C.p` becomes a
/// CALL to `__sm_C__p_get`, not a bare Ident.
///
/// A module's function declaration can land under a synthetic spelling
/// too — the re-export alias `__reex_*`, the deconflict mangle
/// `__m<k>_*` — and a later request binds its own spelling to it
/// (`const __reex_b = __reex_a`); both are the user's function.
pub(super) fn relayable_target(g: &str) -> bool {
    !g.starts_with("__")
        || g.starts_with("__sm_")
        || g.starts_with("__reex_")
        || g.strip_prefix("__m").is_some_and(|r| {
            let digits = r.bytes().take_while(u8::is_ascii_digit).count();
            digits > 0 && r.as_bytes().get(digits) == Some(&b'_')
        })
}

/// Whether the top-level fn `target`'s body reads `arguments` in any
/// form — the same test `arguments_object_devirt` gates its fold on,
/// so a wrapped alias whose uses are all direct calls comes back out
/// as direct calls.
fn target_touches_arguments(ast: &Ast, target: &str) -> bool {
    use super::arguments_object_walkers::{
        body_has_arguments_length, body_has_non_length_arguments_touch,
    };
    ast.stmts.iter().any(|s| {
        matches!(s, Stmt::FnDecl { name, body, .. }
            if name == target
                && (body_has_arguments_length(ast, body)
                    || body_has_non_length_arguments_touch(ast, body)))
    })
}

/// Whether some arena `Ident(name)` sits outside a callee slot.
/// Conservative by construction: an unrelated binding sharing the
/// name only costs the wrap (mirroring
/// `arguments_object_headless_argv::name_only_called_directly`,
/// which answers the same question about a fn name).
///
/// A spread call's callee slot is an escape: the runtime spread lane
/// boxes its callee as a value, which a bare FnSig cannot be
/// (`const h = E.plain; h(1, ...arr)` refused loud there).
///
/// An argument to a declared function's bare-pointer parameter
/// ([`fnsig_param_args`]) is not one either: the call hands the FnSig
/// over as is, exactly as `app(g)` would. Wrapping there put a
/// closure cell into that `__fn(` slot, and the callee branched into
/// the cell's header.
fn local_escapes(ast: &Ast, name: &str, fnsig_args: &HashSet<u32>) -> bool {
    let mut occurrences: HashSet<u32> = HashSet::new();
    for (i, e) in ast.exprs.iter().enumerate() {
        if matches!(e, Expr::Ident(n) if n == name) {
            occurrences.insert(i as u32);
        }
    }
    if occurrences.is_empty() {
        return false;
    }
    let mut callee_slots: HashSet<u32> = HashSet::new();
    for e in &ast.exprs {
        let callee = match e {
            Expr::Call { callee, args }
                if !args
                    .iter()
                    .any(|a| matches!(ast.get_expr(*a), Expr::Spread { .. })) =>
            {
                *callee
            }
            Expr::NewDynamic { callee, .. } => *callee,
            _ => continue,
        };
        if occurrences.contains(&callee.0) {
            callee_slots.insert(callee.0);
        }
    }
    callee_slots.extend(fnsig_args.intersection(&occurrences));
    occurrences.difference(&callee_slots).next().is_some()
}

/// The arguments of direct calls that land in a declared function's
/// `__fn(` parameter, which still names the bare-pointer lane here
/// (`tag_closure_arg_params` has retagged every parameter a closure
/// reaches to `__cls(`). The callee's name must not be rebound
/// anywhere, and the declaration must have no hidden parameter that
/// would shift the argument positions.
fn fnsig_param_args(ast: &Ast) -> HashSet<u32> {
    let mut decls: HashMap<&str, &[Param]> = HashMap::new();
    let mut rebound: HashSet<String> = HashSet::new();
    super::ns_alias::walk_stmts(&ast.stmts, &mut |s| match s {
        Stmt::LetDecl { name, .. } => {
            rebound.insert(name.clone());
        }
        Stmt::FnDecl { params, .. } => rebound.extend(params.iter().map(|p| p.name.clone())),
        _ => {}
    });
    for s in &ast.stmts {
        if let Stmt::FnDecl { name, params, .. } = s
            && !params.iter().any(|p| p.name.starts_with("__"))
        {
            decls.insert(name, params);
        }
    }
    let mut out = HashSet::new();
    for e in &ast.exprs {
        let Expr::Call { callee, args } = e else {
            continue;
        };
        let Expr::Ident(f) = ast.get_expr(*callee) else {
            continue;
        };
        let Some(params) = decls.get(f.as_str()) else {
            continue;
        };
        if rebound.contains(f)
            || args
                .iter()
                .any(|a| matches!(ast.get_expr(*a), Expr::Spread { .. }))
        {
            continue;
        }
        for (p, a) in params.iter().zip(args) {
            if !p.is_rest && crate::ast_closure_param_tag::is_fnsig_ann(&p.type_ann) {
                out.insert(a.0);
            }
        }
    }
    out
}
