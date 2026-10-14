//! RFC 20260912-global-var-cell knife 3 — a script's top-level
//! `function` declaration is an own property of the global object.
//!
//! §16.1.7 step 16: every function in `functionsToInitialize` gets
//! `CreateGlobalFunctionBinding(fn, fo, false)` — a `{writable,
//! enumerable, NOT configurable}` data property whose value is the
//! function object, present before the first statement runs, and the
//! SAME binding the bare name reads (`verifyProperty(this, "f", …)`
//! writes a string through the object and reads it back).
//!
//! The binding a compiled program keeps for a declaration is a direct
//! call symbol, not a slot, so the object cannot point a `GlobalCell`
//! at it. What it can point at is the function's VALUE: every value
//! use of a named declaration already answers one singleton cell
//! (`ssa_lower_closure_canonical` / the naked accessor-face mint —
//! RFC 20260905), so a hidden `any` slot holding that value, bound
//! under the function's name, is the property. The slot is minted in
//! the exact shape a hoisted `var` takes (an `any` prelude binding
//! plus an assignment), because that is the route every function
//! shape — generic, generator, async, default, rest — was measured to
//! carry with identity intact (rotation 637 probes `q1` / `q2`).
//!
//! The value is the declaration's `__forward_<name>` shim — the one
//! every other value route relays a bare declaration name through
//! (`wrap_escaping_fn_ident_bindings` for a bound alias,
//! `promote_this_fn_values` for a `this`-reading body, the ObjectLit /
//! return / argument synthesizers for theirs), so `globalThis.f === f`
//! holds on every route; for a `this`-reading declaration it is the
//! receiver-first `__fwdrecv_<name>` face. Minted here when no earlier pass needed it
//! (this pass runs after them all); a bare `Ident(f)` would reach the
//! store as a `FnSig` with nothing to box (`box_to_any element type
//! FnSig … not supported`, the reject every un-relayed value use meets).
//!
//! Not the property's storage, and registered as residue: a bare
//! call `f()` stays the direct lane, so `globalThis.f = other; f()`
//! still calls the declaration — the same shape `delete this.x` has
//! for a `var` (the static slot outlives the property). A declaration
//! `widen_rebound_fn_decls` turned into an `any` binding (it is
//! assigned somewhere) is no longer a `FnDecl` here and takes no
//! property.

use super::{Ast, Expr, Stmt};

/// The hidden slot's prefix; `emit_script_global_var_bind` strips it
/// to recover the property name.
pub const SCRIPT_GLOBAL_FN_SLOT_PREFIX: &str = "__gfn_";

/// Every top-level `FnDecl` name, synthesized ones included — the
/// exclusion list for the var hoist (`var f;` re-declares the
/// function's binding and mints nothing).
pub(super) fn toplevel_fn_decl_names(top: &[Stmt]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    top.iter()
        .filter_map(|s| match s {
            Stmt::FnDecl { name, .. } if seen.insert(name.clone()) => Some(name.clone()),
            _ => None,
        })
        .collect()
}

/// Mint the hidden slot + bind record for each user-written top-level
/// function declaration. Pipeline position: after `desugar_var_hoist`
/// (the `var` prelude is in place; the function prelude goes in front
/// of it — §16.1.7 instantiates functions in step 16, vars in step 17)
/// and after `desugar_arguments_object` — a forwarder minted BEFORE
/// the argv injection is one the injection then finds and keeps, and
/// its face lacks the `__torajs_argv` the target now takes (mapped
/// `arguments` answered stale values; `arguments` inside a function
/// expression became a ReferenceError). Minted after, the relay peels
/// the injected param and its own boxed entry packs the argv.
pub fn synthesize_script_global_fns(ast: &mut Ast) {
    if !ast.sloppy_script_goal {
        return;
    }
    let names: Vec<String> = toplevel_fn_decl_names(&ast.stmts)
        .into_iter()
        .filter(|n| !super::is_synthesized_dunder(ast, n) && !ast.class_parents.contains_key(n))
        .collect();
    if names.is_empty() {
        return;
    }
    // The LAST declaration of a name is the binding's (§16.1.7 step
    // 10.a.ii), and the one `fn_table` resolves a call to. The shared
    // synthesizer erases a generic target's own type variables to
    // `any` (402-01) — a forwarder is a plain declaration with no type
    // params to bind them, and the test262 harness is all
    // implicitly-generic functions.
    let mut fn_sigs: std::collections::HashMap<
        String,
        (Vec<super::Param>, Option<String>, crate::lexer::Span),
    > = std::collections::HashMap::new();
    let mut existing_forwarders: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl {
            name,
            params,
            return_type,
            span,
            ..
        } = s
        {
            if super::is_shell(name) {
                existing_forwarders.insert(name.clone());
            }
            fn_sigs.insert(name.clone(), (params.clone(), return_type.clone(), *span));
        }
    }
    let mut new_decls = receiver_first_shells(ast, &names, &fn_sigs, &mut existing_forwarders);
    let targets: std::collections::HashSet<String> = names.iter().cloned().collect();
    let (plain_decls, renames) = super::forwarders_object::synthesize_forwarder_decls(
        ast,
        &targets,
        &fn_sigs,
        &existing_forwarders,
    );
    new_decls.extend(plain_decls);
    let mut prelude: Vec<Stmt> = Vec::with_capacity(names.len() * 2);
    // Knife 4-3: a function declared by eval / Script text binds its
    // property where the text ran — the slot's assignment goes right
    // after the (spliced-in) declaration, and the let-decl lane binds
    // on that write. The slot itself still opens in the prelude.
    let mut at_site: Vec<(String, Stmt)> = Vec::new();
    for name in names {
        let slot = format!("{SCRIPT_GLOBAL_FN_SLOT_PREFIX}{name}");
        let value = ast.add_expr(Expr::Closure {
            fn_name: renames[&name].clone(),
            captures: Vec::new(),
        });
        let uninit = ast.add_expr(Expr::Uninit);
        prelude.push(Stmt::LetDecl {
            mutable: true,
            name: slot.clone(),
            type_ann: Some("any".to_string()),
            init: uninit,
            is_var: false,
        });
        let target = ast.add_expr(Expr::Ident(slot.clone()));
        let assign = ast.add_expr(Expr::Assign { target, value });
        let flavor = ast
            .eval_fn_names
            .get(&name)
            .copied()
            .unwrap_or(super::var_hoist::ScriptGlobalVar::Function);
        if flavor == super::var_hoist::ScriptGlobalVar::Function {
            prelude.push(Stmt::Expr(assign));
        } else {
            at_site.push((name, Stmt::Expr(assign)));
        }
        ast.script_global_var_names.insert(slot, flavor);
    }
    for (name, assign) in at_site {
        let pos = ast
            .stmts
            .iter()
            .rposition(|s| matches!(s, Stmt::FnDecl { name: n, .. } if *n == name))
            .expect("a text-declared function is a top-level declaration");
        ast.stmts.insert(pos + 1, assign);
    }
    // After the synthesized head — the hoisted `var` prelude (inert
    // `Uninit` inits), the class globals and the `__cprologue()` call
    // that runs their registrations: a generator factory's cell
    // defines `.prototype` from its `__Gen_<f>` class proto at mint,
    // and before registration that table slot holds a different
    // object than the instances get (`Object.getPrototypeOf(g()) ===
    // g.prototype` was false). Before the first real statement, so
    // the property exists when user code first runs. Registered
    // residue: §16.1.7 instantiates functions (step 16) before vars
    // (step 17), so `Object.keys(globalThis)` lists a script's vars
    // ahead of its functions here.
    let at = prelude_insert_index(ast);
    let tail = ast.stmts.split_off(at);
    ast.stmts.extend(prelude);
    ast.stmts.extend(tail);
    ast.stmts.extend(new_decls);
}

/// The receiver-first shell of each `this`-reading declaration that
/// has no shell yet. The slot's value is what every read the program
/// routes to the binding sees — a method call, `.call`, `new` — so it
/// must carry the receiver; the plain face pins it to `undefined`.
/// `promote_this_fn_values` mints this same shell for a value use of
/// the bare name, so either way the declaration keeps one cell.
fn receiver_first_shells(
    ast: &mut Ast,
    names: &[String],
    fn_sigs: &FnSigs,
    existing: &mut std::collections::HashSet<String>,
) -> Vec<Stmt> {
    let mut out = Vec::new();
    for name in names {
        let (params, return_type, span) = &fn_sigs[name];
        let shells = super::shell_names(name);
        if !ast.this_param_fns.contains(name)
            || params.first().is_none_or(|p| p.name != "__this")
            || shells.iter().any(|n| existing.contains(n))
        {
            continue;
        }
        let [recv, _] = shells;
        out.push(super::forwarder_synth::synth_forwarder_decl(
            ast,
            &recv,
            name,
            params,
            return_type.clone(),
            *span,
            super::ShellFace::ReceiverFirst,
        ));
        existing.insert(recv);
    }
    out
}

type FnSigs =
    std::collections::HashMap<String, (Vec<super::Param>, Option<String>, crate::lexer::Span)>;

/// The first statement that is neither a declaration nor a
/// compiler-minted head statement: an uninitialized binding (the
/// hoisted `var` prelude) or a dunder-named one, or a bare call of a
/// dunder name (`__cprologue()`, which runs the class registrations,
/// or a `__torajs_*` intrinsic).
fn prelude_insert_index(ast: &Ast) -> usize {
    ast.stmts
        .iter()
        .position(|s| match s {
            Stmt::FnDecl { .. } | Stmt::TypeDecl { .. } | Stmt::ClassDecl { .. } => false,
            Stmt::LetDecl { name, init, .. } => {
                !super::is_synthesized_dunder(ast, name)
                    && !matches!(ast.get_expr(*init), Expr::Uninit)
            }
            Stmt::Expr(e) => !matches!(
                ast.get_expr(*e),
                Expr::Call { callee, .. }
                    if matches!(ast.get_expr(*callee), Expr::Ident(n) if super::is_synthesized_dunder(ast, n))
            ),
            _ => true,
        })
        .unwrap_or(ast.stmts.len())
}
