//! A `this`-using function declaration's VALUE is its forwarder.
//!
//! `bind_this_param` gives a top-level `function` whose body says
//! `this` a hidden leading `__this` parameter and seeds `undefined`
//! into every DIRECT call site. Its own doc records the other half as
//! a known boundary: "a call through a value (`const g = F; g(1)`)
//! cannot be reached from here and is left alone deliberately …
//! rather than papering over it with a silently wrong arity".
//!
//! The arity was silently wrong anyway, because nothing else closed
//! the boundary either. Measured on `040ec68bc`, all exit 0:
//!
//! ```text
//! function t(a: any) { return "a=" + a + "/" + typeof (this as any) }
//! const h = t;  h(7)              tr a=undefined/number   bun a=7/undefined
//! function take(f: (x: any) => string) { return f(7) }
//!               take(t)           tr a=true/number        bun a=7/undefined
//! const h = t;  h.length          tr 2                    bun 1
//! const h = t;  h === t           tr false                bun true
//! ```
//!
//! One cause: the value of such a declaration is sometimes the raw
//! `FnAddr` (arity including the receiver) and sometimes the
//! `__forward_<name>` shim (user-visible arity, receiver pinned to
//! `undefined`), decided by which earlier pass happened to claim the
//! site. The array-literal, object-literal and marked-argument routes
//! got the shim; a plain `const h = t` fell to
//! `ssa_lower_stmt_let_decl_global::try_lower_fn_addr_let`, whose
//! `FnSig` slot holds the raw address, and an argument reaching a slot
//! no fixpoint marked did the same.
//!
//! This pass makes the value ONE thing: every value use of a promoted
//! declaration becomes `Expr::Closure { __forward_<name> }`. Identity
//! then falls out of the `__fncell_*` singleton mint
//! (`ssa_lower_closure_canonical`), the arity is the forwarder's
//! declared face, and the receiver seed is the same `undefined` the
//! direct-call rewrite supplies.
//!
//! **Not** value uses, and deliberately untouched:
//!
//! - a `Call` callee — `t(7)` keeps the direct lane `bind_this_param`
//!   already rewrote, so the hot path is unchanged (B-4);
//! - a `Member` / `Index` base — `t.call(r, 7)` answers the real
//!   receiver today, and the forwarder pins `undefined`, so routing it
//!   here would trade a correct answer for a wrong one. The property
//!   face reads the same cell either way (`fnprops_bind_cell`).
//!   `.bind` is the exception, and for the same reason read the other
//!   way: the desugar swallows no `.bind` at all, so nothing downstream
//!   turns it into a direct call — the member call survives and the
//!   runtime kernel calls whatever cell the base evaluated to;
//! - an `Assign` target — `t = …` is a write, and
//!   `widen_rebound_fn_decls` owns that shape.
//!
//! Declarations with no receiver parameter are not in
//! `this_param_fns` at all, so the plain named-fn direct lane
//! (`try_lower_fn_addr_let`) keeps every function that never mentions
//! `this` — which is the hot one.
//!
//! Pipeline position: immediately BEFORE
//! `tag_closure_arg_params`. The rewrite turns an argument site into
//! a closure cell, and the parameter it lands in has to be retagged to
//! the env-first repr by the same fixpoint run — a slot left spelled
//! `(x: any) => string` while the argument became a cell is a
//! calling-convention mismatch, and its symptom is a program that
//! prints nothing at all. Forwarder synthesizers that ran earlier have
//! already turned their sites into `Expr::Closure`, so this pass does
//! not see them.
//!
//! **One declaration, one shell** (RFC 20260914-namedfn-value-identity
//! blade 2). Every value site of a `this`-reading declaration takes
//! the receiver-FIRST shell, so the declaration has exactly one cell
//! (`__fncell_` is keyed by shell name) and therefore one identity,
//! one property bag, one `.prototype`. This pass used to stand aside
//! for a list of "callback" sites that a later pass re-minted with the
//! receiver-first face, which left a declaration reached both ways
//! with TWO cells: `const h: any = t; h === t` was false, `new
//! Proxy(t, {})` dropped the receiver, and `(h as any).prototype` and
//! `t.prototype` were different objects. The list is gone; so is the
//! second minting pass.
//!
//! The receiver-first face is not a cost here. Every lane that can
//! call such a cell reads `FLAG_CLOSURE_RECV_FIRST` — the any-lane
//! kernels, the typed closure call's runtime gate, the higher-order
//! array methods' `sig_skip`, and the promise callback faces — and a
//! declaration with no receiver parameter never reaches this pass at
//! all.

use std::collections::{HashMap, HashSet};

use super::{Ast, Expr, Param, Stmt};

/// Rewrite value uses of `bind_this_param`-promoted declarations to
/// their forwarder, synthesizing the forwarder when no earlier pass
/// already did.
pub fn promote_this_fn_values(ast: &mut Ast) {
    if ast.this_param_fns.is_empty() {
        return;
    }
    let mut sigs: HashMap<String, (Vec<Param>, Option<String>, crate::lexer::Span)> =
        HashMap::new();
    let mut have_forwarder: HashSet<String> = HashSet::new();
    for s in &ast.stmts {
        let Stmt::FnDecl {
            name,
            params,
            return_type,
            span,
            ..
        } = s
        else {
            continue;
        };
        have_forwarder.insert(name.clone());
        // The receiver parameter has to be THERE: a promoted name whose
        // declaration a later pass reshaped (a lift that gave it `__env`)
        // is no longer the shape this rewrite forwards to.
        if ast.this_param_fns.contains(name) && params.first().is_some_and(|p| p.name == "__this") {
            sigs.insert(name.clone(), (params.clone(), return_type.clone(), *span));
        }
    }
    // The site walk below is an arena sweep with no scope information,
    // so it cannot tell `return fn` inside `function g() { const fn = 1 }`
    // from a read of the declaration — and rewriting that one turns a
    // program that answers `1` into one that answers `[Function: fn]`.
    // A rebinding no block boundary separates from the declaration
    // (a parameter / catch / loop variable, a top-level redeclaration)
    // still refuses the whole name; a BLOCK hands back the arena slots
    // it owns instead, so the reads outside it keep their forwarder
    // (`namedfn_rebound_scope`, whose doc carries why the whole-name
    // verdict is not free).
    let mut block_scoped: HashSet<usize> = HashSet::new();
    sigs.retain(|n, _| {
        if super::namedfn_rebound_scope::rebound_beyond_blocks(&ast.stmts, n) {
            return false;
        }
        block_scoped.extend(
            super::namedfn_rebound_scope::block_scoped_expr_ids(ast, n)
                .into_iter()
                .map(|e| e.0 as usize),
        );
        true
    });
    if sigs.is_empty() {
        return;
    }

    // Every arena slot that holds an ExprId in a NON-value position.
    // The complement of this set, among `Ident`s naming a promoted
    // declaration, is exactly the value uses.
    let mut non_value: HashSet<usize> = HashSet::new();
    for e in &ast.exprs {
        match e {
            Expr::Call { callee, .. } | Expr::NewDynamic { callee, .. } => {
                non_value.insert(callee.0 as usize);
            }
            // `.bind` is the one property whose base IS a value use:
            // `desugar_function_prototype_methods` never swallows it
            // (every `.bind` rides the runtime kernel), so the member
            // call survives to lowering and the kernel calls the cell
            // it is handed. A plain `__forward_` shell pins the
            // receiver to `undefined`, which is exactly the answer
            // `f.bind({_v: 10})(5)` must not give. `.call` / `.apply`
            // keep the exclusion: this pass runs BEFORE the desugar,
            // and the forms it swallows become direct calls whose
            // receiver channel is already right.
            Expr::Member { obj, name } if name != "bind" => {
                non_value.insert(obj.0 as usize);
            }
            Expr::Index { obj, .. } => {
                non_value.insert(obj.0 as usize);
            }
            Expr::Assign { target, .. } => {
                non_value.insert(target.0 as usize);
            }
            _ => {}
        }
    }
    let mut sites: Vec<(usize, String)> = Vec::new();
    for (i, e) in ast.exprs.iter().enumerate() {
        if let Expr::Ident(n) = e
            && sigs.contains_key(n)
            && !non_value.contains(&i)
            && !block_scoped.contains(&i)
        {
            sites.push((i, n.clone()));
        }
    }
    if sites.is_empty() {
        return;
    }

    let mut new_decls: Vec<Stmt> = Vec::new();
    for (_, target) in &sites {
        let forward_name = format!("__fwdrecv_{target}");
        if have_forwarder.contains(&forward_name) {
            continue;
        }
        have_forwarder.insert(forward_name.clone());
        let (params, return_type, target_span) = sigs.get(target).unwrap().clone();
        new_decls.push(super::forwarder_synth::synth_forwarder_decl(
            ast,
            &forward_name,
            target,
            &params,
            return_type,
            target_span,
            crate::ast::ShellFace::ReceiverFirst,
        ));
    }
    for (i, target) in sites {
        ast.exprs[i] = Expr::Closure {
            fn_name: format!("__fwdrecv_{target}"),
            captures: Vec::new(),
        };
    }
    ast.stmts.extend(new_decls);
}
