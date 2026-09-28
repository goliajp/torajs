//! Namespace statics handed over as CALLBACKS where no annotated slot
//! names their type: a builtin array HOF's callback, a promise
//! handler. The dispatcher cell (RFC 20260719) serves the boxed lane
//! only — its `fn_addr` is a loud TypeError — so a typed consumer of
//! such a site (`xs.map(Math.sqrt)` calling through `CallIndirect`,
//! `.then` running its handler) threw, or read the throw's stand-in
//! as the handler's result. Each site now takes a forwarder shell
//! spelled from the member's OWN signature — the checker's namespace
//! table, the one truth for what the member takes and answers — so
//! the consumer sees an ordinary lifted closure:
//! `__forward_nsv_Math_sqrt(__env, __nsa0: number): number { return
//! Math.sqrt(__nsa0) }`, whose body compiles through the typed
//! kernel. A member the table types with a rest parameter has no
//! fixed shell and keeps the cell. A site an annotated slot receives
//! is the sibling `wrap_ns_static_values`, which spells the shell
//! from the slot and runs first.
//!
//! Only a callback site: the caller calls the value and nothing else.
//! A value stored somewhere — an object literal's field, an array's
//! element — is compared as well (`{ value: Promise.try }` in a
//! property-descriptor check is `=== Promise.try`), and a shell is
//! not the member; those sites keep the cell.

use std::collections::HashSet;

use crate::ast::{Ast, Expr, ExprId, Param, Stmt};
use crate::check::Type;

/// How many arguments the lowered loop or the promise kernel hands a
/// fn argument of this member method — the builtin array HOFs
/// (§23.1.3: value, index, array; the comparator's two; the
/// reducer's four) and the promise handlers. `None` for any other
/// method: its fn argument is a plain value.
fn callback_arity(name: &str) -> Option<usize> {
    Some(match name {
        "map" | "forEach" | "filter" | "find" | "findLast" | "findIndex" | "findLastIndex"
        | "some" | "every" | "flatMap" => 3,
        "sort" | "toSorted" => 2,
        "reduce" | "reduceRight" => 4,
        "then" | "catch" => 1,
        "finally" => 0,
        _ => return None,
    })
}

pub fn wrap_ns_static_value_sites(ast: &mut Ast) {
    // (site, arity the site's caller passes)
    let mut sites: Vec<(ExprId, usize)> = Vec::new();
    for e in ast.exprs.iter() {
        let (candidates, arity): (Vec<ExprId>, usize) = match e {
            Expr::Call { callee, args } => match ast.get_expr(*callee) {
                Expr::Member { name, .. } => match callback_arity(name) {
                    Some(n) => (args.clone(), n),
                    None => continue,
                },
                _ => continue,
            },
            _ => continue,
        };
        sites.extend(
            candidates
                .into_iter()
                .filter(|a| ns_static_member(ast, *a).is_some())
                .map(|a| (a, arity)),
        );
    }
    if sites.is_empty() {
        return;
    }
    let mut existing: HashSet<String> = ast
        .stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::FnDecl { name, .. } if name.starts_with("__forward_nsv_") => Some(name.clone()),
            _ => None,
        })
        .collect();
    for (eid, arity) in sites {
        let Some((ns, m)) = ns_static_member(ast, eid) else {
            continue;
        };
        let Some((own_anns, ret_ann)) = own_signature(&ns, &m) else {
            continue;
        };
        // The shell takes what its caller passes: the member's own
        // parameters first, `any` for the positions past them. A
        // fixed-arity member ignores the extra arguments (§10.3.1),
        // so the shell forwards its own parameters only; a variadic
        // one reads them all (`[9, 4].map(Math.max)` is
        // `Math.max(9, 0, [9, 4])`, NaN) — the table's `length` is
        // the spec length and cannot tell the two apart.
        let param_anns: Vec<String> = (0..arity)
            .map(|i| {
                own_anns
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| "any".to_string())
            })
            .collect();
        let variadic =
            ns == "console" || (ns == "Math" && matches!(m.as_str(), "max" | "min" | "hypot"));
        let forwarded = if variadic {
            arity
        } else {
            arity.min(own_anns.len())
        };
        let forward_name = format!("__forward_nsv_{ns}_{m}_{arity}");
        if existing.insert(forward_name.clone()) {
            synth_shell(ast, &forward_name, &ns, &m, &param_anns, forwarded, ret_ann);
        }
        ast.exprs[eid.0 as usize] = Expr::Closure {
            fn_name: forward_name,
            captures: Vec::new(),
        };
    }
}

/// `(ns, member)` when `eid` reads a namespace static the intern
/// table knows.
fn ns_static_member(ast: &Ast, eid: ExprId) -> Option<(String, String)> {
    if let Expr::Member { obj, name } = ast.get_expr(eid)
        && let Expr::Ident(ns) = ast.get_expr(*obj)
        && torajs_rc::ns_static_id(ns, name) != torajs_rc::NS_STATIC_UNKNOWN
    {
        Some((ns.clone(), name.clone()))
    } else {
        None
    }
}

/// The member's parameter and return annotations, from the checker's
/// namespace table; `None` for a member the table does not type as a
/// fixed-arity function.
fn own_signature(ns: &str, m: &str) -> Option<(Vec<String>, String)> {
    let ns_static: &'static str = crate::check_type_of_ident::NS_OBJECT_IDENTS
        .iter()
        .copied()
        .find(|s| *s == ns)?;
    let Some(Ok(Type::Function(params, ret))) =
        crate::check_type_of_member_namespace::try_match(&Type::Object(ns_static), m)
    else {
        return None;
    };
    if params.iter().any(|p| matches!(p, Type::Rest(_))) {
        return None;
    }
    Some((
        params.iter().map(crate::check::type_to_ann).collect(),
        crate::check::type_to_ann(&ret),
    ))
}

fn synth_shell(
    ast: &mut Ast,
    forward_name: &str,
    ns: &str,
    m: &str,
    param_anns: &[String],
    forwarded: usize,
    ret_ann: String,
) {
    let mut params: Vec<Param> = Vec::with_capacity(param_anns.len() + 1);
    params.push(Param {
        name: "__env".into(),
        type_ann: Some("__env()".to_string()),
        default: None,
        is_rest: false,
    });
    let mut args: Vec<ExprId> = Vec::with_capacity(param_anns.len());
    for (i, ann) in param_anns.iter().enumerate() {
        let pname = format!("__nsa{i}");
        params.push(Param {
            name: pname.clone(),
            type_ann: Some(ann.clone()),
            default: None,
            is_rest: false,
        });
        if i < forwarded {
            args.push(ast.add_expr(Expr::Ident(pname)));
        }
    }
    let ns_eid = ast.add_expr(Expr::Ident(ns.to_string()));
    let callee = ast.add_expr(Expr::Member {
        obj: ns_eid,
        name: m.to_string(),
    });
    let call = ast.add_expr(Expr::Call { callee, args });
    ast.stmts.push(Stmt::FnDecl {
        name: forward_name.to_string(),
        type_params: Vec::new(),
        params,
        return_type: Some(ret_ann),
        body: vec![Stmt::Return(Some(call))],
        is_generator: false,
        // no user source to point at: toString of a wrapped builtin
        // answers the empty span
        span: crate::lexer::Span { start: 0, end: 0 },
    });
}
