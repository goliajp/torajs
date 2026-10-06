//! Rotation 365 — boxed-only object-literal method argv: the
//! `valueOf: function () { args = arguments; … }` idiom the t262
//! Date-setter family (`arg-to-number` cases) hands to a builtin
//! protocol. The field closure has NO visible call site at all —
//! zero member reads, zero Ident references — so neither the
//! static-argv face (argc votes need sites) nor the knife-4c alias
//! seed (needs a binding) can admit it, and the body stayed
//! KeepLoud. But the shape needs no proof beyond reference
//! discipline: the field value lives in a dynobj cell, and the ONLY
//! way any call reaches it is the any-lane member dispatch (a
//! builtin protocol like OrdinaryToPrimitive, or a user any-typed
//! call), which enters through the closure cell's boxed adapter —
//! the verified face that feeds REAL argc/argv. Same safety
//! argument as knife 4a's zero-Ident rule: no site can name the fn,
//! so no lane can reach it with the old signature.
//!
//! Admission gates, all mechanical:
//! - the field init is a lifted closure whose body touches
//!   `arguments` (any form — length included: Real-mode length is
//!   the argv face's own rewrite);
//! - zero `Ident(fn_name)` arena references and exactly ONE
//!   `Closure { fn_name }` reference (the field position) — an
//!   alias, forwarder body, or wrap candidate would each mint a
//!   second reference reaching lanes this walk cannot vouch for;
//! - no member reference spells the field name off the object
//!   binding (`o.valueOf` anywhere — call or read — would be a
//!   typed-dispatch site calling the OLD signature; by-name over
//!   the whole arena, conservative direction = refuse);
//! - no default / rest params, and the user param count fits the
//!   boxed-adapter budget (knife 4a's MAX_BOXED_PARAMS defense);
//! - not shadow/bare-assign excluded (the shared exclusion set).
//!
//! The admitted fn splits by receiver promotion: a body the
//! fnexpr-this pass gave a `__this` head param rides the
//! method-argv tier (`[__this, __torajs_argv, …]`, the knife-4a
//! shape); a this-free body rides the value tier
//! (`[__torajs_argv, …]`, the P11 shape). Both adapter head shapes
//! are already synthesized and audited.

//! RFC 20260915 blade 4 — the `next` / `return` / `throw` trio off a
//! `__`-spelled (compiler-minted) binding is benign like the one off a
//! generator instance: the injected for-of close helpers' `__it` and
//! a machine's `__gfit_N` reach the field through the any lane, never
//! a typed-dispatch site, and injected text shares no namespace with
//! the program (`gen-dstr-rtrn-close-args-001`).

use super::arguments_object_walkers::{
    body_has_arguments_length, body_has_non_length_arguments_touch,
};
use super::{Ast, Expr, Stmt};

pub(super) fn collect_objlit_boxed_only_argv(
    ast: &Ast,
    excluded: &std::collections::HashSet<String>,
) -> (
    std::collections::HashSet<String>,
    std::collections::HashSet<String>,
) {
    use std::collections::{HashMap, HashSet};
    // (object binding, field) → lifted closure fn name.
    let mut fields: HashMap<(String, String), String> = HashMap::new();
    for s in &ast.stmts {
        let Stmt::LetDecl { name, init, .. } = s else {
            continue;
        };
        let Expr::ObjectLit { fields: fs } = ast.get_expr(*init) else {
            continue;
        };
        for (fname, feid) in fs.iter().filter_map(|(f, e)| f.as_str().map(|f| (f, e))) {
            if let Expr::Closure { fn_name, .. } = ast.get_expr(*feid) {
                fields.insert((name.clone(), fname.to_string()), fn_name.clone());
            }
        }
    }
    // Reference discipline over the whole arena. (No early return on
    // an empty field map — the store arm below seeds independently.)
    //
    // Rotation 372 — a member reference standing as the callee of a
    // SPREAD-carrying call is NOT a typed-dispatch site: the spread
    // lane (`ssa_lower_call_spread`) boxes the receiver and enters
    // the any-lane member dispatch, which reaches the field closure
    // through its boxed adapter — the exact channel this face is
    // built on. Such a reference doesn't refuse admission; any OTHER
    // member spelling of the field name still does (it could be a
    // typed direct call carrying the OLD signature).
    let mut spread_call_callees: HashSet<u32> = HashSet::new();
    for e in &ast.exprs {
        if let Expr::Call { callee, args } | Expr::OptCall { callee, args } = e
            && args
                .iter()
                .any(|a| matches!(ast.get_expr(*a), Expr::Spread { .. }))
        {
            spread_call_callees.insert(callee.0);
        }
    }
    // Rotation 455 — a `.next()` / `.return()` / `.throw()` spelled
    // off a GENERATOR INSTANCE binding is the §27.5.1 prototype
    // surface, never a reference into a user object literal — the
    // t262 rtrn-close iterator duo (`{ next: fn, return: fn }`
    // consumed only through the runtime close) was refused admission
    // purely because `it.return(...)` shares the field's spelling.
    // A binding qualifies when it HAS a value source and every source
    // (top-level init, every arena assignment) is a call to a
    // registered generator factory; anything else stays a threat.
    let is_gen_call = |eid: super::ExprId| {
        matches!(ast.get_expr(eid), Expr::Call { callee, .. }
            if matches!(ast.get_expr(*callee), Expr::Ident(g)
                if ast.generator_factory_classes.contains_key(g)))
    };
    let mut gen_sources: HashMap<&str, (bool, bool)> = HashMap::new();
    for s in &ast.stmts {
        if let Stmt::LetDecl { name, init, .. } = s
            && !matches!(ast.get_expr(*init), Expr::Uninit)
        {
            let e = gen_sources.entry(name.as_str()).or_insert((true, false));
            e.1 = true;
            if !is_gen_call(*init) {
                e.0 = false;
            }
        }
    }
    for e in &ast.exprs {
        if let Expr::Assign { target, value } = e
            && let Expr::Ident(n) = ast.get_expr(*target)
        {
            let en = gen_sources.entry(n.as_str()).or_insert((true, false));
            en.1 = true;
            if !is_gen_call(*value) {
                en.0 = false;
            }
        }
    }
    let gen_instances: HashSet<&str> = gen_sources
        .iter()
        .filter(|(_, (all_gen, any))| *all_gen && *any)
        .map(|(n, _)| *n)
        .collect();
    let (ident_refs, closure_refs, member_names) =
        scan_arena_refs(ast, &spread_call_callees, &gen_instances);
    let mut value_fns: HashSet<String> = HashSet::new();
    let mut method_fns: HashSet<String> = HashSet::new();
    for ((_, fname), fn_name) in &fields {
        if member_names.contains(fname.as_str()) {
            continue;
        }
        admit_boxed_only(
            ast,
            fn_name,
            excluded,
            &ident_refs,
            &closure_refs,
            &mut value_fns,
            &mut method_fns,
        );
    }
    // Rotation 365 store arm — the same boxed-only argument applies
    // to an ANONYMOUS fn-expr assigned straight into a boxed-face
    // store position (`spy[Symbol.toPrimitive] = function () { … }`,
    // the t262 to-primitive spy idiom): the target is any-lane-only
    // consumption by construction (the escape-store profile's B2
    // roots), so every call enters the closure cell's boxed adapter.
    // The reference discipline is the same — the store's value slot
    // is the ONE Closure reference, and no Ident can name the fn.
    let props_recvs = super::fnexpr_this_recvs_props::collect_props_receiver_binding_names(
        &ast.stmts, &ast.exprs,
    );
    let expando_recvs = super::fnexpr_this_expando::ExpandoRecvs::scan(&ast.stmts, &ast.exprs);
    for e in &ast.exprs {
        let Expr::Assign { target, value } = e else {
            continue;
        };
        let Expr::Closure { fn_name, .. } = ast.get_expr(*value) else {
            continue;
        };
        if !super::arguments_object_escape_store::boxed_face_store_target(
            ast,
            *target,
            &props_recvs,
            &expando_recvs,
        ) {
            continue;
        }
        admit_boxed_only(
            ast,
            fn_name,
            excluded,
            &ident_refs,
            &closure_refs,
            &mut value_fns,
            &mut method_fns,
        );
    }
    // The call-argument arms (sibling module doc).
    super::arguments_object_objlit_argv_calls::admit_call_arg_faces(
        ast,
        excluded,
        &ident_refs,
        &closure_refs,
        &member_names,
        &mut value_fns,
        &mut method_fns,
    );
    // The return-escape arm (r454) — per owning function since 642-08.
    super::arguments_object_objlit_argv_return::admit_return_escapes(
        ast,
        excluded,
        &ident_refs,
        &closure_refs,
        &mut value_fns,
        &mut method_fns,
    );
    (value_fns, method_fns)
}

/// Reference discipline over the whole arena: Ident and Closure
/// reference counts per name, and every member spelling that could
/// be a typed-dispatch site. A `.next` / `.return` / `.throw` off a
/// generator instance or a compiler-minted binding is benign (module
/// doc). Rotation 652 — so is ANY member off a binding the program
/// only ever declares `: any`: the checker types that receiver Any,
/// so `o.m(...)` is the any-lane member dispatch that enters the
/// field closure's boxed adapter — the face's own channel
/// (`objlit_nominal_anylane`'s (a) leg gives such a literal its
/// `__this: any` faces for the same reason). Without it the `let it:
/// any = { m() { …arguments… } }; it.m(1); it.m(2, 3)` shape had no
/// face at all: the static objlit face needs ONE argc vote, and this
/// face refused on the member spelling.
fn scan_arena_refs<'a>(
    ast: &'a Ast,
    spread_call_callees: &std::collections::HashSet<u32>,
    gen_instances: &std::collections::HashSet<&str>,
) -> (
    std::collections::HashMap<&'a str, usize>,
    std::collections::HashMap<&'a str, usize>,
    std::collections::HashSet<&'a str>,
) {
    use std::collections::{HashMap, HashSet};
    let any_only = any_only_toplevel_bindings(ast);
    let mut ident_refs: HashMap<&str, usize> = HashMap::new();
    let mut closure_refs: HashMap<&str, usize> = HashMap::new();
    let mut member_names: HashSet<&str> = HashSet::new();
    for (i, e) in ast.exprs.iter().enumerate() {
        match e {
            Expr::Ident(n) => *ident_refs.entry(n).or_insert(0) += 1,
            Expr::Closure { fn_name, .. } => *closure_refs.entry(fn_name).or_insert(0) += 1,
            Expr::Member { obj, name } | Expr::OptChain { obj, name }
                if !spread_call_callees.contains(&(i as u32)) =>
            {
                let benign = (matches!(name.as_str(), "next" | "return" | "throw")
                    && matches!(ast.get_expr(*obj), Expr::Ident(n)
                            if gen_instances.contains(n.as_str()) || n.starts_with("__")))
                    || matches!(ast.get_expr(*obj), Expr::Ident(n)
                            if any_only.contains(n.as_str()));
                if !benign {
                    member_names.insert(name);
                }
            }
            _ => {}
        }
    }
    (ident_refs, closure_refs, member_names)
}

/// Names whose every top-level declaration is spelled `: any` and
/// that no fn declares as a param or local — a member call off such
/// a name is any-lane dispatch wherever it appears.
fn any_only_toplevel_bindings(ast: &Ast) -> std::collections::HashSet<&str> {
    use std::collections::HashSet;
    let mut any_decl: HashSet<&str> = HashSet::new();
    let mut other_decl: HashSet<&str> = HashSet::new();
    let mut fn_local: HashSet<String> = HashSet::new();
    for s in &ast.stmts {
        match s {
            Stmt::LetDecl { name, type_ann, .. } => {
                if type_ann.as_deref() == Some("any") {
                    any_decl.insert(name);
                } else {
                    other_decl.insert(name);
                }
            }
            Stmt::FnDecl { params, body, .. } => {
                for p in params {
                    fn_local.insert(p.name.clone());
                }
                crate::ast_collect_bindings::collect_local_binding_names(body, &mut fn_local);
            }
            _ => {}
        }
    }
    any_decl
        .into_iter()
        .filter(|n| !other_decl.contains(n) && !fn_local.contains(*n))
        .collect()
}

/// The shared per-fn admission gate (module doc): reference
/// discipline + arguments touch + param budget, then the head-shape
/// split by `__this` promotion.
pub(super) fn admit_boxed_only(
    ast: &Ast,
    fn_name: &str,
    excluded: &std::collections::HashSet<String>,
    ident_refs: &std::collections::HashMap<&str, usize>,
    closure_refs: &std::collections::HashMap<&str, usize>,
    value_fns: &mut std::collections::HashSet<String>,
    method_fns: &mut std::collections::HashSet<String>,
) {
    if excluded.contains(fn_name)
        || ident_refs.contains_key(fn_name)
        || closure_refs.get(fn_name) != Some(&1)
    {
        return;
    }
    let Some((params, body)) = ast.stmts.iter().find_map(|s| match s {
        Stmt::FnDecl {
            name, params, body, ..
        } if name == fn_name => Some((params, body)),
        _ => None,
    }) else {
        return;
    };
    if !body_has_non_length_arguments_touch(ast, body) && !body_has_arguments_length(ast, body) {
        return;
    }
    if params.iter().any(|p| p.default.is_some() || p.is_rest) || params.len() > 5 {
        return;
    }
    if ast.fnexpr_recv_fns.contains(fn_name) {
        method_fns.insert(fn_name.to_string());
    } else {
        value_fns.insert(fn_name.to_string());
    }
}
