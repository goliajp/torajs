//! The object-literal branch of the static-argv face — the same
//! question as its `__cm_` sibling, asked of a different kind of
//! evidence.
//!
//! A class method's call sites are arena `Ident`s naming the mangled
//! fn, so the collector next door scans for that name. An
//! object-literal method has no such name at its call sites: it
//! lifts to a `__closure_N` FnDecl stored in a field, and every call
//! reads `o.m(...)`. This module maps `(binding, field) → closure`
//! from the declarations and votes argc over the member calls
//! instead, with the admission bounds that mapping needs (any bare
//! value use of `o` hands the methods to sites we cannot see).
//!
//! Split from `arguments_object_static_argv` in rotation 616, which
//! had grown to 659 lines.

use super::arguments_object_walkers::body_has_non_length_arguments_touch;
use super::{Ast, Expr, Stmt};

/// RFC 20260801 objlit branch (rotation 272) — object-literal
/// methods join the static-argv face. A method lifts to a
/// `__closure_N` FnDecl stored in the ObjectLit field; its call
/// sites are `o.m(...)` member calls, invisible to the Ident scan,
/// so this collector maps `(binding, field) → closure fn` from the
/// object-literal declarations and votes argc over the member call
/// sites instead. Admission requires the binding itself to stay
/// tame: any bare value use of `o` (aliasing hands the methods to
/// call sites we cannot see), any rebind of `o`, any write through
/// `o.<field>`, or a fn-local shadow of the name kills every method
/// face of that object. A member READ of the method (`var ref =
/// o.m`) stays legal without voting — the escape call pads missing
/// trailing params with undefined, wrong-at-parity with the KeepLoud
/// baseline (same account as the forwarder relay above).
pub(super) fn collect_objlit_method_static_argv(
    ast: &Ast,
    env_fns: &std::collections::HashSet<String>,
) -> (
    std::collections::HashMap<String, usize>,
    std::collections::HashSet<String>,
) {
    use std::collections::{HashMap, HashSet};
    // (binding, field) → lifted closure fn name, for top-level
    // object-literal lets whose method closures are env fns with an
    // arguments touch.
    let mut methods: HashMap<(String, String), String> = HashMap::new();
    for s in &ast.stmts {
        let Stmt::LetDecl { name, init, .. } = s else {
            continue;
        };
        let Expr::ObjectLit { fields } = ast.get_expr(*init) else {
            continue;
        };
        for (fname, feid) in fields {
            if let Expr::Closure { fn_name, .. } = ast.get_expr(*feid)
                && env_fns.contains(fn_name)
                && let Some(fname) = fname.as_str()
            {
                methods.insert((name.clone(), fname.to_string()), fn_name.clone());
            }
        }
    }
    if methods.is_empty() {
        return (HashMap::new(), HashSet::new());
    }
    let obj_names: HashSet<&String> = methods.keys().map(|(o, _)| o).collect();
    let mut fn_local_names: HashSet<String> = HashSet::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl { params, body, .. } = s {
            for p in params {
                fn_local_names.insert(p.name.clone());
            }
            crate::ast_collect_bindings::collect_local_binding_names(body, &mut fn_local_names);
        }
    }
    // Reference discipline over the whole arena: classify every
    // `Ident(o)` / `Member { Ident(o), f }` use.
    let mut member_base: HashSet<u32> = HashSet::new();
    let mut call_callee_member: HashSet<u32> = HashSet::new();
    let mut killed: HashSet<String> = HashSet::new();
    for e in &ast.exprs {
        match e {
            Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => {
                member_base.insert(obj.0);
            }
            Expr::Call { callee, .. } | Expr::OptCall { callee, .. } => {
                call_callee_member.insert(callee.0);
            }
            Expr::Assign { target, .. } => match ast.get_expr(*target) {
                // `o = ...` rebind / `o.f = ...` field write.
                Expr::Ident(n) if obj_names.contains(n) => {
                    killed.insert(n.clone());
                }
                Expr::Member { obj, .. } | Expr::Index { obj, .. } => {
                    if let Expr::Ident(n) = ast.get_expr(*obj)
                        && obj_names.contains(n)
                    {
                        killed.insert(n.clone());
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
    for (i, e) in ast.exprs.iter().enumerate() {
        if let Expr::Ident(n) = e
            && obj_names.contains(n)
            && !member_base.contains(&(i as u32))
        {
            // Bare value use — the object aliases away.
            killed.insert(n.clone());
        }
    }
    // Vote argc over the member call sites. A member read that is
    // NOT a call callee is a value escape — the face's fold would
    // answer the direct sites' argc to a call it can't see (knife
    // 4c caught exactly that: `var ref = obj.m; ref(a, b)` next to
    // a 1-arg direct call answered length 1). Such a field's fn
    // leaves the static face; the knife-4c argv-face seed picks the
    // alias up and delivers the TRUE per-call argc instead.
    let mut escaped_fns: HashSet<String> = HashSet::new();
    let mut argcs: HashMap<String, HashSet<usize>> = HashMap::new();
    for (i, e) in ast.exprs.iter().enumerate() {
        let Expr::Member { obj, name: fname } = e else {
            continue;
        };
        let Expr::Ident(oname) = ast.get_expr(*obj) else {
            continue;
        };
        let Some(fn_name) = methods.get(&(oname.clone(), fname.clone())) else {
            continue;
        };
        if !call_callee_member.contains(&(i as u32)) {
            escaped_fns.insert(fn_name.clone());
            continue;
        }
        // Find the call that owns this callee to count its args.
        for e2 in &ast.exprs {
            if let Expr::Call { callee, args } | Expr::OptCall { callee, args } = e2
                && callee.0 == i as u32
            {
                if args
                    .iter()
                    .any(|a| matches!(ast.get_expr(*a), Expr::Spread { .. }))
                {
                    killed.insert(oname.clone());
                } else {
                    argcs.entry(fn_name.clone()).or_default().insert(args.len());
                }
            }
        }
    }
    let mut out: HashMap<String, usize> = HashMap::new();
    for ((oname, _), fn_name) in &methods {
        if killed.contains(oname) || fn_local_names.contains(oname) {
            continue;
        }
        let Some(votes) = argcs.get(fn_name) else {
            continue;
        };
        if votes.len() != 1 {
            continue;
        }
        let n = *votes.iter().next().unwrap();
        let touches = ast.stmts.iter().any(|s| {
            matches!(s, Stmt::FnDecl { name, body, .. }
                if name == fn_name
                    && (body_has_non_length_arguments_touch(ast, body)
                        || super::arguments_object_walkers::body_has_arguments_length(ast, body)))
        });
        if touches {
            out.insert(fn_name.clone(), n);
        }
    }
    (out, escaped_fns)
}
