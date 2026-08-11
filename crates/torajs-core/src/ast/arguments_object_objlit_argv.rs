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
        for (fname, feid) in fs {
            if let Expr::Closure { fn_name, .. } = ast.get_expr(*feid) {
                fields.insert((name.clone(), fname.clone()), fn_name.clone());
            }
        }
    }
    if fields.is_empty() {
        return (HashSet::new(), HashSet::new());
    }
    // Reference discipline over the whole arena.
    let mut ident_refs: HashMap<&str, usize> = HashMap::new();
    let mut closure_refs: HashMap<&str, usize> = HashMap::new();
    let mut member_names: HashSet<&str> = HashSet::new();
    for e in &ast.exprs {
        match e {
            Expr::Ident(n) => *ident_refs.entry(n).or_insert(0) += 1,
            Expr::Closure { fn_name, .. } => *closure_refs.entry(fn_name).or_insert(0) += 1,
            Expr::Member { name, .. } | Expr::OptChain { name, .. } => {
                member_names.insert(name);
            }
            _ => {}
        }
    }
    let mut value_fns: HashSet<String> = HashSet::new();
    let mut method_fns: HashSet<String> = HashSet::new();
    for ((_, fname), fn_name) in &fields {
        if excluded.contains(fn_name)
            || ident_refs.contains_key(fn_name.as_str())
            || closure_refs.get(fn_name.as_str()) != Some(&1)
            || member_names.contains(fname.as_str())
        {
            continue;
        }
        let Some((params, body)) = ast.stmts.iter().find_map(|s| match s {
            Stmt::FnDecl {
                name, params, body, ..
            } if name == fn_name => Some((params, body)),
            _ => None,
        }) else {
            continue;
        };
        if !body_has_non_length_arguments_touch(ast, body) && !body_has_arguments_length(ast, body)
        {
            continue;
        }
        if params.iter().any(|p| p.default.is_some() || p.is_rest) || params.len() > 5 {
            continue;
        }
        if ast.fnexpr_recv_fns.contains(fn_name) {
            method_fns.insert(fn_name.clone());
        } else {
            value_fns.insert(fn_name.clone());
        }
    }
    (value_fns, method_fns)
}
