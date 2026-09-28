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

use super::forwarders_object::{snapshot_fn_sigs, synthesize_forwarder_decls};
use super::{Ast, Expr, ExprId, Stmt};
use std::collections::HashSet;

pub fn wrap_escaping_fn_ident_bindings(ast: &mut Ast) {
    let (fn_sigs, existing_forwarders, _fn_type_params) = snapshot_fn_sigs(ast);
    if fn_sigs.is_empty() {
        return;
    }
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
        if g == name || !fn_sigs.contains_key(g) || !relayable_target(g) {
            return;
        }
        cands.push((*init, name.clone(), g.clone()));
    });
    let mut targets: HashSet<String> = HashSet::new();
    let mut rewrites: Vec<(ExprId, String)> = Vec::new();
    let mut escaping: HashSet<String> = HashSet::new();
    let mut settled: HashSet<String> = HashSet::new();
    for (eid, local, target) in cands {
        if settled.insert(local.clone()) && local_escapes(ast, &local) {
            escaping.insert(local.clone());
        }
        if !escaping.contains(&local) {
            continue;
        }
        targets.insert(target.clone());
        rewrites.push((eid, target));
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
fn relayable_target(g: &str) -> bool {
    !g.starts_with("__")
        || g.starts_with("__sm_")
        || g.starts_with("__reex_")
        || g.strip_prefix("__m").is_some_and(|r| {
            let digits = r.bytes().take_while(u8::is_ascii_digit).count();
            digits > 0 && r.as_bytes().get(digits) == Some(&b'_')
        })
}

/// Whether some arena `Ident(name)` sits outside a callee slot.
/// Conservative by construction: an unrelated binding sharing the
/// name only costs the wrap (mirroring
/// `arguments_object_headless_argv::name_only_called_directly`,
/// which answers the same question about a fn name).
fn local_escapes(ast: &Ast, name: &str) -> bool {
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
            Expr::Call { callee, .. } | Expr::NewDynamic { callee, .. } => *callee,
            _ => continue,
        };
        if occurrences.contains(&callee.0) {
            callee_slots.insert(callee.0);
        }
    }
    occurrences.difference(&callee_slots).next().is_some()
}
