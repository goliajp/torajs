//! RFC 20260708 closure argc/argv tier collectors — split out of
//! `arguments_object.rs` (file-size limit) when the argv-face tier
//! landed. Three units:
//!
//! - `collect_value_argc` — length-only real-argc closure VALUE
//!   seed + HOF mono-track admit (RFC 20260708-closure-argc-abi).
//! - `collect_value_argv` — full-arguments tier seed (RFC
//!   20260708-closure-argv-face).
//! - `safe_binding_chain` — the shared direct-call-or-alias binding
//!   safety walk both tiers gate on.

use super::arguments_object_walkers::{
    body_has_arguments_length, body_has_non_length_arguments_touch,
    body_has_unsafe_return_arguments,
};
use super::{Ast, Expr, ExprId, Stmt};

/// RFC 20260708-closure-argc-abi chunk 1 — collect the closure
/// VALUE form seed: length-only lifted closures whose value is
/// consumed exclusively through a safe binding chain. Returns
/// `(value_real_argc fns, argc-local binding names)` — see the
/// gate rationale in the body comments.
pub(super) fn collect_value_argc(
    ast: &Ast,
    env_fns: &std::collections::HashSet<String>,
    iife_real_argc: &std::collections::HashSet<String>,
    excluded: &std::collections::HashSet<String>,
) -> (
    std::collections::HashSet<String>,
    std::collections::HashSet<String>,
) {
    use std::collections::{HashMap, HashSet};
    // RFC 20260708-closure-argc-abi chunk 1 — closure VALUE form: a
    // lifted closure that reads `arguments.length` and is NOT an IIFE
    // has no static call site, so the real argc travels at runtime
    // (the ssa_lower closure-local call arm prepends
    // ConstI64(user-arg count) for bindings in `argc_locals`). Two
    // gates keep this zero-silent-wrong:
    //
    // 1. Only the length-only tier qualifies: a body that also reads
    //    `arguments[dynamic]` / spreads `...arguments` needs the arg
    //    VALUES (the argv face, recorded follow-up) and stays
    //    KeepLoud — admitting a beyond-arity call whose values are
    //    unreachable would be silent-wrong.
    // 2. Every binding the closure value can reach must be
    //    direct-call-or-alias only (`f(...)` / `const g = f`). Any
    //    other use — passed as an argument, stored in a container,
    //    returned, reassigned — kills the whole chain back to the
    //    fn, which then stays KeepLoud (checker rejects `arguments`
    //    loudly, today's behavior): a call-emit site that doesn't
    //    know about the argc slot would feed the callee garbage.
    //    The HOF/param-infection face is the RFC's chunk 2.
    let mut value_real_argc: HashSet<String> = HashSet::new();
    // binding name → source lifted-closure fn name (aliases share
    // the source fn).
    let argc_candidates: HashMap<String, String>;
    for s in &ast.stmts {
        if let Stmt::FnDecl { name, body, .. } = s {
            if env_fns.contains(name)
                && !iife_real_argc.contains(name)
                && !excluded.contains(name)
                && body_has_arguments_length(ast, body)
                && !body_has_non_length_arguments_touch(ast, body)
            {
                value_real_argc.insert(name.clone());
            }
        }
    }
    // RFC chunk 2 (mono track) — a length-only closure passed
    // DIRECTLY as an argument to a fn whose receiving param is
    // unannotated: that param goes through implicit generics (__T),
    // and the monomorphizer instantiates the slot as `__clsargc(`
    // (see ssa_lower_generics_mono_shapes), whose param-call arm
    // prepends the runtime argc. Two gates: the receiving param must
    // be unannotated (mono track — a typed param never sees the
    // argc-shaped signature), and the receiving body must consume it
    // by DIRECT CALL only (any other use — alias, re-pass, store —
    // reaches sites that don't know the argc slot).
    let mut hof_arg_fns: HashSet<String> = HashSet::new();
    // The receiving-fn gate below resolves the callee name `g` with
    // a top-level FnDecl `.any()` — name-only. A call site whose `g`
    // is really a LOCAL binding (param / fn-body let) shadowing a
    // same-named top-level fn would pass the gate while the actual
    // receiver knows nothing about the argc slot → ABI mismatch
    // (probe: SIGBUS). Guard: a callee name bound in any fn-local
    // position anywhere is not provably the top-level fn — skip
    // (over-kill only loses the optimization, never wrong).
    let mut fn_local_names: HashSet<String> = HashSet::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl { params, body, .. } = s {
            for p in params {
                fn_local_names.insert(p.name.clone());
            }
            crate::ast_collect_bindings::collect_local_binding_names(body, &mut fn_local_names);
        }
    }
    for e in &ast.exprs {
        let Expr::Call { callee, args } = e else {
            continue;
        };
        let Expr::Ident(g) = ast.get_expr(*callee) else {
            continue;
        };
        if fn_local_names.contains(g) {
            continue;
        }
        for (pi, a) in args.iter().enumerate() {
            let Expr::Closure { fn_name, .. } = ast.get_expr(*a) else {
                continue;
            };
            if !value_real_argc.contains(fn_name) {
                continue;
            }
            // RFC 20260708-variadic — a variadic-annotated param
            // (`(...args: E[]) => R`) also admits: its call lane is
            // the boxed dual entry whose adapter feeds the REAL
            // argc, and the checker's rest-tail assignability keeps
            // the value inside variadic-typed slots (a static
            // declared-pair site can never receive it), so no
            // body-use gate is needed for that track.
            let ok = ast.stmts.iter().any(|s| {
                matches!(s, Stmt::FnDecl { name, params, body, .. }
                    if name == g
                        && ((params.get(pi).is_some_and(|p| p.type_ann.is_none())
                            && param_uses_are_direct_calls(ast, body, &params[pi].name))
                            || params.get(pi).is_some_and(|p| p
                                .type_ann
                                .as_deref()
                                .is_some_and(|a| a.contains("__rest(")))))
            });
            if ok {
                hof_arg_fns.insert(fn_name.clone());
            }
        }
    }

    // Direct bindings + alias closure + kill fixpoint — the shared
    // safety walk (also used by the argv-face tier below).
    argc_candidates = safe_binding_chain(ast, |fn_name| value_real_argc.contains(fn_name));
    // Only fns whose closure value is consumed EXCLUSIVELY through a
    // surviving binding chain get the argc injection — plus the
    // RFC-chunk-2 mono-track form below. A closure stored in a
    // container, returned, etc. must NOT be reshaped: its call
    // sites don't know about the argc slot and would feed the body
    // garbage.
    let mut injected: HashSet<String> = argc_candidates.values().cloned().collect();
    injected.extend(hof_arg_fns);
    let argc_locals: HashSet<String> = argc_candidates.keys().cloned().collect();
    (injected, argc_locals)
}

/// RFC 20260708-closure-argv-face — collect the full-arguments tier:
/// lifted closures whose body touches `arguments` in any non-length
/// form. RFC 20260801 knife 2 admitted the bare-escape shapes
/// (return / alias / call arg / member) too: the rewriter swaps the
/// bare `arguments` to the materialized `__torajs_arguments` array,
/// which every consumer then treats as an ordinary `any[]` (a bare
/// root return takes the moved mark — see
/// `consume_all_idents_in_return`). Only the pass-through
/// `return arguments[i]` shape stays KeepLoud (the elem box would
/// leave borrowing the array's stake — UAF once the array
/// scope-drops). The value must survive the same
/// direct-call-or-alias binding walk as the argc tier: every call
/// then rides the boxed dual entry (the checker mints the type
/// rest-tail), whose adapter feeds real argc + argv. Returns
/// `(argv fns, binding names)`.
pub(super) fn collect_value_argv(
    ast: &Ast,
    env_fns: &std::collections::HashSet<String>,
    iife_real_argc: &std::collections::HashSet<String>,
    excluded: &std::collections::HashSet<String>,
) -> (
    std::collections::HashSet<String>,
    std::collections::HashSet<String>,
) {
    use std::collections::HashSet;
    let mut full: HashSet<String> = HashSet::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl { name, body, .. } = s
            && env_fns.contains(name)
            && !iife_real_argc.contains(name)
            && !excluded.contains(name)
            && body_has_non_length_arguments_touch(ast, body)
            && !body_has_unsafe_return_arguments(ast, body)
        {
            full.insert(name.clone());
        }
    }
    if full.is_empty() {
        return (HashSet::new(), HashSet::new());
    }
    let candidates = safe_binding_chain(ast, |fn_name| full.contains(fn_name));
    let injected: HashSet<String> = candidates.values().cloned().collect();
    let locals: HashSet<String> = candidates.keys().cloned().collect();
    (injected, locals)
}

/// Shared binding safety walk (argc + argv tiers): seed direct
/// bindings (`const f = <closure literal>` whose fn passes `seed`),
/// grow aliases (`const g = f`) to a fixpoint, then kill every
/// chain containing a binding used anywhere other than a Call
/// callee or a still-candidate alias init. A killed chain removes
/// the whole source fn (its call sites don't know the reshaped
/// signature).
fn safe_binding_chain(
    ast: &Ast,
    seed: impl Fn(&str) -> bool,
) -> std::collections::HashMap<String, String> {
    use std::collections::{HashMap, HashSet};
    let mut candidates: HashMap<String, String> = HashMap::new();
    for s in &ast.stmts {
        if let Stmt::LetDecl { name, init, .. } = s
            && let Expr::Closure { fn_name, .. } = ast.get_expr(*init)
            && seed(fn_name)
        {
            candidates.insert(name.clone(), fn_name.clone());
        }
    }
    // Knife 4c (RFC 20260801-arguments-method-face) — `var ref =
    // obj.method`: a member read of a top-level object-literal
    // binding whose field holds a seeded closure joins the chain
    // too (the test262 meth-args template's escape form). Only the
    // ALIAS binding enters the walk — the object binding itself is
    // untouched, and its member CALLS stay safe because the argv
    // face's rest-tail checker type routes a reshaped field through
    // the boxed variadic lane rather than the static widen.
    let mut objlit_fields: HashMap<(&str, &str), &str> = HashMap::new();
    for s in &ast.stmts {
        if let Stmt::LetDecl { name, init, .. } = s
            && let Expr::ObjectLit { fields } = ast.get_expr(*init)
        {
            for (fname, feid) in fields {
                if let Expr::Closure { fn_name, .. } = ast.get_expr(*feid) {
                    objlit_fields.insert((name.as_str(), fname.as_str()), fn_name.as_str());
                }
            }
        }
    }
    // The escape usually arrives var-hoisted: `var ref = obj.m` is
    // `let ref: any (Uninit)` + a top-level `ref = obj.m` Assign.
    // Seed both spellings; the Assign's target Ident is whitelisted
    // for the kill walk below (it is the binding's own definition,
    // not a value use — any OTHER assignment to the alias still
    // kills the chain).
    let mut assign_legal: HashSet<u32> = HashSet::new();
    for s in &ast.stmts {
        let (bname, member_eid, target_eid) = match s {
            Stmt::LetDecl { name, init, .. } => (name.clone(), *init, None),
            Stmt::Expr(eid) => {
                let Expr::Assign { target, value } = ast.get_expr(*eid) else {
                    continue;
                };
                let Expr::Ident(name) = ast.get_expr(*target) else {
                    continue;
                };
                (name.clone(), *value, Some(target.0))
            }
            _ => continue,
        };
        if let Expr::Member { obj, name: m } = ast.get_expr(member_eid)
            && let Expr::Ident(o) = ast.get_expr(*obj)
            && let Some(fn_name) = objlit_fields.get(&(o.as_str(), m.as_str()))
            && seed(fn_name)
            && !candidates.contains_key(&bname)
        {
            candidates.insert(bname, fn_name.to_string());
            if let Some(t) = target_eid {
                assign_legal.insert(t);
            }
        }
    }
    loop {
        let mut grew = false;
        for s in &ast.stmts {
            if let Stmt::LetDecl { name, init, .. } = s
                && let Expr::Ident(src) = ast.get_expr(*init)
                && candidates.contains_key(src)
                && !candidates.contains_key(name)
            {
                candidates.insert(name.clone(), candidates[src].clone());
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    // RFC 20260808 knife 2 — the kill walk's by-name arena scan must
    // not count a fn-local PARAM shadow's uses (the t262 harness
    // declares a `func` param whose body uses killed every binding
    // the bind family actually names `func`). Resolution is per fn —
    // `toplevel_binding_refs` cannot arbitrate here: its Closure arm
    // counts capture names unfiltered (the r290 over-approximation),
    // so the harness param captured by its own inner closure poisons
    // the whole set. A closure CAPTURE of the name outside a
    // shadowing body is a real reference the walk can't see through —
    // that still kills the chain.
    //
    // Knife 7 — the exemption arms ONLY for a binding that stands in
    // `.bind`-receiver position somewhere: those calls reach the
    // body through the bind kernel's boxed entry (real argc/argv),
    // the verified face. A shadow-colliding binding whose only uses
    // are DIRECT calls stays on the old kill: admitting it routes
    // the calls down the fn-indirect lane on the DECLARED signature
    // while the body grows the argc/argv slots — a 5-param body
    // entered through a 2-param sig read its argv param off garbage
    // (the 27 new SIGSEGVs of the every/filter/map `-c-i-25` t262
    // family). Real fix = the var-hoisted binding's rest-tail mint
    // (L3b); until then the un-exempted shape keeps its loud reject.
    let shadow_owned: std::collections::HashMap<String, Vec<bool>> = candidates
        .keys()
        .map(|b| {
            let has_bind_receiver_use = ast.exprs.iter().any(|e| {
                matches!(e, Expr::Member { obj, name } if name == "bind"
                    && matches!(ast.get_expr(*obj), Expr::Ident(n) if n == b))
            });
            let owned = if has_bind_receiver_use {
                super::fnexpr_bind_this::shadowed_use_eids(ast, b)
            } else {
                vec![false; ast.exprs.len()]
            };
            (b.clone(), owned)
        })
        .collect();
    // RFC 20260808 escape-store profile — the face-position roots
    // the fnexpr-this store arm admits (B2, rotation 337): a store
    // into `<any>.k` / `<any>[k]` / `X.prototype.k` /
    // `<any|array>.constructor[k]` is boxed-only consumption — a
    // builtin reaches the stored value through the factory adapter
    // or the any-lane call path, both of which enter the boxed dual
    // entry with REAL argc/argv. Computed once for the kill walk's
    // escape-store exemption below.
    let props_recvs =
        super::fnexpr_this_recvs::collect_props_receiver_binding_names(&ast.stmts, &ast.exprs);
    let boxed_face_store = |target: ExprId| -> bool {
        super::arguments_object_escape_store::boxed_face_store_target(ast, target, &props_recvs)
    };
    loop {
        let mut killed: HashSet<String> = HashSet::new();
        for b in candidates.keys() {
            let sh = &shadow_owned[b];
            if ast.exprs.iter().enumerate().any(|(i, e)| {
                matches!(e, Expr::Closure { captures, .. }
                    if captures.iter().any(|c| c == b))
                    && !sh[i]
            }) {
                killed.insert(b.clone());
                continue;
            }
            let b_eids: HashSet<u32> = (0..ast.exprs.len() as u32)
                .filter(|&i| {
                    matches!(&ast.exprs[i as usize], Expr::Ident(n) if n == b) && !sh[i as usize]
                })
                .collect();
            let mut legal: HashSet<u32> = HashSet::new();
            for e in &ast.exprs {
                if let Expr::Call { callee, .. } = e
                    && b_eids.contains(&callee.0)
                {
                    legal.insert(callee.0);
                }
            }
            for s in &ast.stmts {
                if let Stmt::LetDecl { name, init, .. } = s
                    && b_eids.contains(&init.0)
                    && candidates.contains_key(name)
                {
                    legal.insert(init.0);
                }
            }
            // Knife 4c — the seeding `ref = obj.m` Assign's own
            // target Ident is the binding's definition, not a use.
            for &t in assign_legal.intersection(&b_eids) {
                legal.insert(t);
            }
            // RFC 20260808 escape-store — the binding stored into a
            // boxed-face position (see `boxed_face_store` above) is
            // not a kill: every path from that slot back to the body
            // (species construct, builtin callback, any-lane member
            // call) enters the boxed dual entry, which feeds the
            // reshaped signature its REAL argc/argv. Any OTHER use
            // shape still kills.
            for e in &ast.exprs {
                if let Expr::Assign { target, value } = e
                    && b_eids.contains(&value.0)
                    && boxed_face_store(*target)
                {
                    legal.insert(value.0);
                }
            }
            // RFC 20260808 knife 2 — `b.bind(…)` as a Call callee:
            // the bind desugar routes an arguments-touching target to
            // the runtime kernel (member call kept), whose bound cell
            // dispatches through the boxed dual entry — real
            // argc/argv against the reshaped rest-tail signature, so
            // the receiver position is NOT a kill. A `.bind` member
            // read outside a call (a stored `b.bind` value) still
            // kills the chain.
            for (mi, e) in ast.exprs.iter().enumerate() {
                if let Expr::Member { obj, name } = e
                    && name == "bind"
                    && b_eids.contains(&obj.0)
                    && ast
                        .exprs
                        .iter()
                        .any(|c| matches!(c, Expr::Call { callee, .. } if callee.0 == mi as u32))
                {
                    legal.insert(obj.0);
                }
            }
            if b_eids.difference(&legal).next().is_some() {
                killed.insert(b.clone());
            }
        }
        if killed.is_empty() {
            break;
        }
        let killed_fns: HashSet<String> = candidates
            .iter()
            .filter(|(b, _)| killed.contains(*b))
            .map(|(_, f)| f.clone())
            .collect();
        candidates.retain(|_, f| !killed_fns.contains(f));
    }
    candidates
}

/// RFC 20260708-closure-argc-abi chunk 2 — every arena occurrence of
/// `Ident(pname)` must be a Call callee. Conservative: same-named
/// idents anywhere in the program count, so an unrelated binding
/// with the param's name fails the gate (stays loud, never silent).
fn param_uses_are_direct_calls(ast: &Ast, _body: &[Stmt], pname: &str) -> bool {
    use std::collections::HashSet;
    let p_eids: HashSet<u32> = (0..ast.exprs.len() as u32)
        .filter(|&i| matches!(&ast.exprs[i as usize], Expr::Ident(n) if n == pname))
        .collect();
    let mut legal: HashSet<u32> = HashSet::new();
    for e in &ast.exprs {
        if let Expr::Call { callee, .. } = e
            && p_eids.contains(&callee.0)
        {
            legal.insert(callee.0);
        }
    }
    p_eids.difference(&legal).next().is_none()
}
