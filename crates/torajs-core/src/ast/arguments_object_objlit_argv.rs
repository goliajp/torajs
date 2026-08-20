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
    let mut ident_refs: HashMap<&str, usize> = HashMap::new();
    let mut closure_refs: HashMap<&str, usize> = HashMap::new();
    let mut member_names: HashSet<&str> = HashSet::new();
    for (i, e) in ast.exprs.iter().enumerate() {
        match e {
            Expr::Ident(n) => *ident_refs.entry(n).or_insert(0) += 1,
            Expr::Closure { fn_name, .. } => *closure_refs.entry(fn_name).or_insert(0) += 1,
            Expr::Member { name, .. } | Expr::OptChain { name, .. } => {
                if !spread_call_callees.contains(&(i as u32)) {
                    member_names.insert(name);
                }
            }
            _ => {}
        }
    }
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
    let props_recvs =
        super::fnexpr_this_recvs::collect_props_receiver_binding_names(&ast.stmts, &ast.exprs);
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
    // Rotation 372 apply arm — an INLINE fn-expr as the `.apply`
    // callee whose LITERAL argArray carries a `...spread`
    // (`(function () { …arguments… }).apply(null, [1, ...src])`,
    // the t262 expressions/array family): the fn-value apply route
    // hands exactly this shape to the spread lane's bare kernel,
    // which invokes the closure cell's boxed adapter — real
    // argc/argv, the face's own channel. Inline means the Closure
    // expr is the fn's only reference (the shared gate re-checks).
    for e in &ast.exprs {
        let Expr::Call { callee, args } = e else {
            continue;
        };
        let Expr::Member { obj, name } = ast.get_expr(*callee) else {
            continue;
        };
        if name != "apply" || args.len() != 2 {
            continue;
        }
        let Expr::Closure { fn_name, .. } = ast.get_expr(*obj) else {
            continue;
        };
        let spread_arg_array = matches!(ast.get_expr(args[1]), Expr::Array(els)
            if els.iter().any(|el| matches!(ast.get_expr(*el), Expr::Spread { .. })));
        if !spread_arg_array {
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
    // r454 return arm — a fn-expr escaping through a RETURN statement
    // (`return function () { …arguments… }`, or `const g = <fn-expr>;
    // return g;`): the caller receives an opaque closure VALUE, so —
    // exactly like the field and store arms — every call reaches the
    // body through the closure cell's boxed adapter (the enclosing
    // fn's inferred return sig publishes the rest-tail spelling, see
    // preinfer_closure_sigs). The alias form additionally requires
    // the binding's ONLY ident reference to be the return operand:
    // any other use could be a direct call carrying the old
    // signature.
    let mut lets: Vec<(&str, super::ExprId)> = Vec::new();
    super::arguments_object_chain::collect_letdecls_recursive(&ast.stmts, &mut lets);
    let mut returns: Vec<super::ExprId> = Vec::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl { body, .. } = s {
            collect_return_operands(body, &mut returns);
        }
    }
    for r in returns {
        let fn_name = match ast.get_expr(r) {
            Expr::Closure { fn_name, .. } => fn_name.clone(),
            Expr::Ident(b) => {
                if ident_refs.get(b.as_str()) != Some(&1) {
                    continue;
                }
                let mut init_fn: Option<String> = None;
                let mut refused = false;
                for (n, init) in &lets {
                    if *n == b.as_str() {
                        match (init_fn.is_none(), ast.get_expr(*init)) {
                            (true, Expr::Closure { fn_name, .. }) => {
                                init_fn = Some(fn_name.clone());
                            }
                            _ => {
                                refused = true;
                                break;
                            }
                        }
                    }
                }
                match (refused, init_fn) {
                    (false, Some(f)) => f,
                    _ => continue,
                }
            }
            _ => continue,
        };
        admit_boxed_only(
            ast,
            &fn_name,
            excluded,
            &ident_refs,
            &closure_refs,
            &mut value_fns,
            &mut method_fns,
        );
    }
    (value_fns, method_fns)
}

/// Every `return <expr>` operand reachable from `body`, nested
/// control flow included. Nested FnDecls are walked too — their
/// returns escape a closure value the same way.
fn collect_return_operands(body: &[Stmt], out: &mut Vec<super::ExprId>) {
    for s in body {
        match s {
            Stmt::Return(Some(e)) => out.push(*e),
            Stmt::FnDecl { body, .. } => collect_return_operands(body, out),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_return_operands(core::slice::from_ref(then_branch), out);
                if let Some(e) = else_branch {
                    collect_return_operands(core::slice::from_ref(e), out);
                }
            }
            Stmt::While { body, .. }
            | Stmt::DoWhile { body, .. }
            | Stmt::Labeled { body, .. }
            | Stmt::ForOf { body, .. }
            | Stmt::ForOfSplitIter { body, .. } => {
                collect_return_operands(core::slice::from_ref(body), out)
            }
            Stmt::For { body, .. } => collect_return_operands(core::slice::from_ref(body), out),
            Stmt::Block(b) | Stmt::Multi(b) => collect_return_operands(b, out),
            Stmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                collect_return_operands(body, out);
                collect_return_operands(catch_body, out);
                if let Some(f) = finally_body {
                    collect_return_operands(f, out);
                }
            }
            Stmt::Switch { cases, default, .. } => {
                for c in cases {
                    collect_return_operands(&c.body, out);
                }
                if let Some(d) = default {
                    collect_return_operands(d, out);
                }
            }
            _ => {}
        }
    }
}

/// The shared per-fn admission gate (module doc): reference
/// discipline + arguments touch + param budget, then the head-shape
/// split by `__this` promotion.
fn admit_boxed_only(
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
