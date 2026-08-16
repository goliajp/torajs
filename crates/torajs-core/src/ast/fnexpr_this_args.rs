//! Rotation 345 — the boxed-consumption POSITION classifiers shared
//! by knife-2 promotion (`fnexpr_this_routed`) and the
//! arguments-object faces (`arguments_object_escape_store`): the
//! construct-channel callee whitelist, equality / value-op operand
//! sets, and the explicit-`any` / proven-generic param argument
//! positions with their greatest-fixpoint safety proof. Split from
//! `fnexpr_this_routed.rs` under the 500-line file rule.

use super::{Expr, ExprId, Stmt};
// The never-calls VALUE-position classifiers moved to the valueops
// sibling at the 500-line cap (rotation 410); the re-export keeps
// `fnexpr_this_shapes`'s import path intact.
pub(super) use super::fnexpr_this_args_valueops::{
    define_property_target_idents, eq_operand_idents, export_default_idents,
    instanceof_name_idents, typeof_operand_idents,
};

/// The construct-channel callee whitelist backing the fourth
/// receiver-safe use shape: `Reflect.construct` and the
/// `Array.from` / `Array.fromAsync` statics re-dispatched through
/// `.call` / `.apply` (their ns-static cells are recv-first, so the
/// thisArg rides argv[0] into the kernel's Construct split).
fn construct_channel_callee(exprs: &[Expr], callee: ExprId) -> bool {
    match &exprs[callee.0 as usize] {
        // Rotation 410 — the value-shaped-parent synthetics: the
        // super dispatch reaches a closure only through
        // `invoke_with_this` (receiver-honoring), and the heritage
        // gate never invokes its argument at all (§15.7.14 step 5 is
        // a header read).
        Expr::Ident(n) if n == "__torajs_super_value" || n == "__torajs_heritage_check" => true,
        Expr::Member { obj, name } if name == "construct" => {
            matches!(&exprs[obj.0 as usize], Expr::Ident(n) if n == "Reflect")
        }
        Expr::Member { obj, name } if name == "call" || name == "apply" => {
            matches!(&exprs[obj.0 as usize], Expr::Member { obj: ns, name: m }
                if (m == "from" || m == "fromAsync")
                    && matches!(&exprs[ns.0 as usize], Expr::Ident(n) if n == "Array"))
        }
        _ => false,
    }
}

/// RFC 20260808-construct-channel B6 刀 2 — an argument handed to a
/// CONSTRUCT-CHANNEL builtin (`Reflect.construct(C, …)`,
/// `Array.from.call(C, …)` / `.apply`, fromAsync same) is the
/// fourth receiver-safe use shape: those builtins reach the closure
/// only through receiver-honoring channels (construct →
/// `invoke_with_this`; the recv-first ns-static cell prepends the
/// `.call` thisArg), so no receiver-unaware call path exists.
/// Callee whitelist, not "any builtin argument" — B-4
/// narrow-surface: each admitted callee's runtime path is audited.
/// Shared with the arguments-object kill walk (rotation 345): the
/// same channels deliver REAL argc/argv through the boxed dual
/// entry, so such an argument position must not evict the fn from
/// the argv face either.
pub(super) fn construct_channel_arg_idents(exprs: &[Expr]) -> std::collections::HashSet<ExprId> {
    let mut out: std::collections::HashSet<ExprId> = exprs
        .iter()
        .filter_map(|e| match e {
            Expr::Call { callee, args } if construct_channel_callee(exprs, *callee) => Some(
                args.iter()
                    .copied()
                    .filter(|a| matches!(&exprs[a.0 as usize], Expr::Ident(_))),
            ),
            _ => None,
        })
        .flatten()
        .collect();
    // Rotation 345 knife 5 — `new C()` (the NewDynamic callee) rides
    // the same channel: `__torajs_anyv_construct` → the plain-fn
    // kernel invokes through `invoke_with_this` with the allocated
    // `this`, which shifts argv on FLAG_CLOSURE_RECV_FIRST — and its
    // boxed dual entry carries real argc/argv for the arguments face.
    out.extend(exprs.iter().filter_map(|e| match e {
        Expr::NewDynamic { callee, .. } if matches!(&exprs[callee.0 as usize], Expr::Ident(_)) => {
            Some(*callee)
        }
        _ => None,
    }));
    out
}

/// The sixth receiver-safe use shape (RFC 20260808-construct-channel
/// B2 knife, rotation 345): an Ident standing as an argument to a
/// program-local FnDecl whose matching param is EXPLICITLY `any`,
/// or a generic type-param slot proven call-free (2b —
/// `safe_generic_param_names` below). An `any` value crosses into
/// the any lane as a boxed cell, and every any-lane call path
/// honors the receiver channel (`__torajs_any_call` /
/// `invoke_with_this` shift argv on FLAG_CLOSURE_RECV_FIRST), so no
/// receiver-unaware call can reach the promoted closure. The callee
/// must be the program's only FnDecl of that name (a duplicate
/// poisons the entry), and a call carrying a spread admits nothing
/// (positions shift).
pub(super) fn any_param_arg_idents(
    stmts: &[Stmt],
    exprs: &[Expr],
) -> std::collections::HashSet<ExprId> {
    let mut fn_params: std::collections::HashMap<&str, Option<FnSig>> =
        std::collections::HashMap::new();
    collect_fn_decl_params(stmts, &mut fn_params);
    let safe_generics = safe_generic_param_names(stmts, exprs, &fn_params);
    let mut out = std::collections::HashSet::new();
    for e in exprs {
        let Expr::Call { callee, args } = e else {
            continue;
        };
        let Expr::Ident(fname) = &exprs[callee.0 as usize] else {
            continue;
        };
        let Some(Some((type_params, params))) = fn_params.get(fname.as_str()) else {
            continue;
        };
        if args
            .iter()
            .any(|a| matches!(&exprs[a.0 as usize], Expr::Spread { .. }))
        {
            continue;
        }
        for (i, a) in args.iter().enumerate() {
            if matches!(&exprs[a.0 as usize], Expr::Ident(_))
                && let Some(p) = params.get(i)
                && !p.is_rest
                && p.type_ann.as_deref().is_some_and(|ann| {
                    ann == "any"
                        || (type_params.iter().any(|t| t == ann)
                            && safe_generics.contains(p.name.as_str()))
                })
            {
                out.insert(*a);
            }
        }
    }
    out
}

/// Rotation 375 — the INLINE-literal twin of
/// [`any_param_arg_idents`]: a marked fn-expr standing directly in
/// an explicitly-`any` (or proven-safe generic) param slot of a
/// program-local FnDecl call promotes under the same
/// greatest-fixpoint proof — the value crosses into the flag-aware
/// any lane at the call boundary, and an inline literal has zero
/// aliases by construction (stronger than the Ident shape, which
/// must also pass the routed parity walk). This is what admits the
/// `new Promise(function () { …this… })` executor: the
/// promise-new desugar has already rewritten it to
/// `__promise_from_executor(<fn-expr>)` whose `__ex` param is
/// explicit `any`, and the helper body's `__ex(...)` rides
/// `__torajs_any_call` → `invoke_with_this` (§27.2.3.1 step 9's
/// Call(executor, undefined, «…»)).
pub(super) fn collect_any_param_literal_faces(
    stmts: &[Stmt],
    exprs: &[Expr],
    fn_expr_exprs: &std::collections::HashSet<ExprId>,
    patches: &mut Vec<super::fnexpr_this_faces::FacePatch>,
) {
    let mut fn_params: std::collections::HashMap<&str, Option<FnSig>> =
        std::collections::HashMap::new();
    collect_fn_decl_params(stmts, &mut fn_params);
    let safe_generics = safe_generic_param_names(stmts, exprs, &fn_params);
    for e in exprs {
        let Expr::Call { callee, args } = e else {
            continue;
        };
        let Expr::Ident(fname) = &exprs[callee.0 as usize] else {
            continue;
        };
        let Some(Some((type_params, params))) = fn_params.get(fname.as_str()) else {
            continue;
        };
        if args
            .iter()
            .any(|a| matches!(&exprs[a.0 as usize], Expr::Spread { .. }))
        {
            continue;
        }
        for (i, a) in args.iter().enumerate() {
            if let Some(p) = params.get(i)
                && !p.is_rest
                && p.type_ann.as_deref().is_some_and(|ann| {
                    ann == "any"
                        || (type_params.iter().any(|t| t == ann)
                            && safe_generics.contains(p.name.as_str()))
                })
            {
                super::fnexpr_this_faces::collect_face(stmts, exprs, *a, fn_expr_exprs, patches);
            }
        }
    }
}

/// FnDecl type-params + params by name over the whole program (fn
/// bodies and blocks recurse). A second decl of the same name
/// poisons the entry to `None` — a by-name call cannot tell which
/// one runs.
type FnSig<'a> = (&'a [String], &'a [super::Param]);

fn collect_fn_decl_params<'a>(
    stmts: &'a [Stmt],
    out: &mut std::collections::HashMap<&'a str, Option<FnSig<'a>>>,
) {
    for s in stmts {
        match s {
            Stmt::FnDecl {
                name,
                type_params,
                params,
                body,
                ..
            } => {
                out.entry(name.as_str())
                    .and_modify(|e| *e = None)
                    .or_insert(Some((type_params.as_slice(), params.as_slice())));
                collect_fn_decl_params(body, out);
            }
            Stmt::Block(inner) | Stmt::Multi(inner) => collect_fn_decl_params(inner, out),
            _ => {}
        }
    }
}

/// 2b — a param whose annotation IS one of its fn's type params
/// (`same<T>(a: T, b: T)`) admits an argument only when the param
/// NAME provably never enters a call lane: monomorphization may bind
/// the slot to a typed fn signature whose CallIndirect bypasses the
/// receiver flag. The proof is a greatest-fixpoint refutation over
/// every `Ident(name)` use in the program (a cross-fn by-name blur —
/// strictly conservative, since a hit in ANY fn refutes): a use is
/// harmless when it stands as an equality operand, as the init of an
/// explicitly-`any` LetDecl (the value crosses into the flag-aware
/// any lane), or as an argument to another safe param (explicit
/// `any`, or a generic still in the safe set — the recursive
/// `sameValue → sameValueCheck` spelling). Anything else refutes.
fn safe_generic_param_names<'a>(
    stmts: &'a [Stmt],
    exprs: &[Expr],
    fns: &std::collections::HashMap<&'a str, Option<FnSig<'a>>>,
) -> std::collections::HashSet<&'a str> {
    let mut safe: std::collections::HashSet<&'a str> = fns
        .values()
        .flatten()
        .flat_map(|(tps, params)| {
            params.iter().filter(|p| {
                !p.is_rest
                    && p.type_ann
                        .as_deref()
                        .is_some_and(|a| tps.iter().any(|t| t == a))
            })
        })
        .map(|p| p.name.as_str())
        .collect();
    let eq_sites = eq_operand_idents(exprs);
    let mut any_let_inits: std::collections::HashSet<ExprId> = std::collections::HashSet::new();
    collect_any_letdecl_inits(stmts, &mut any_let_inits);
    // `!x` (ToBoolean — no call, no prototype walk) and `typeof x`
    // are pure value positions; the same-NAME blur folds every fn's
    // params together (`__t262_assert(actual: boolean)` says
    // `!actual`, which must not refute sameValue's `actual`).
    let value_op_sites: std::collections::HashSet<ExprId> = exprs
        .iter()
        .filter_map(|e| match e {
            Expr::Unary {
                op: super::UnaryOp::Not,
                expr,
            }
            | Expr::TypeOf { expr } => Some(*expr),
            _ => None,
        })
        .collect();
    // arg site → the (callee, index) it feeds; poisoned/spread calls
    // classify their args as refuting by NOT entering this map.
    let mut arg_sites: std::collections::HashMap<ExprId, (&str, usize)> =
        std::collections::HashMap::new();
    for e in exprs {
        if let Expr::Call { callee, args } = e
            && let Expr::Ident(fname) = &exprs[callee.0 as usize]
            && matches!(fns.get(fname.as_str()), Some(Some(_)))
            && !args
                .iter()
                .any(|a| matches!(&exprs[a.0 as usize], Expr::Spread { .. }))
        {
            for (i, a) in args.iter().enumerate() {
                arg_sites.insert(*a, (fname.as_str(), i));
            }
        }
    }
    loop {
        let mut refuted: Vec<&str> = Vec::new();
        for (i, e) in exprs.iter().enumerate() {
            let Expr::Ident(n) = e else { continue };
            let Some(&name) = safe.get(n.as_str()) else {
                continue;
            };
            let eid = ExprId(i as u32);
            if eq_sites.contains(&eid)
                || any_let_inits.contains(&eid)
                || value_op_sites.contains(&eid)
            {
                continue;
            }
            let ok = arg_sites.get(&eid).is_some_and(|(g, j)| {
                fns.get(g).and_then(|s| *s).is_some_and(|(tps, params)| {
                    params.get(*j).is_some_and(|p| {
                        !p.is_rest
                            && p.type_ann.as_deref().is_some_and(|a| {
                                a == "any"
                                    || (tps.iter().any(|t| t == a)
                                        && safe.contains(p.name.as_str()))
                            })
                    })
                })
            });
            if !ok {
                refuted.push(name);
            }
        }
        if refuted.is_empty() {
            return safe;
        }
        for n in refuted {
            safe.remove(n);
        }
    }
}

/// Every LetDecl init ExprId whose declared annotation is exactly
/// `any` — the crossing point into the flag-aware any lane. The
/// walk must reach EVERY stmt shape that owns a body: a decl the
/// walk misses classifies its init as a refuting use (`const e:
/// any = expected` inside an `if` branch is the sameValueCheck
/// spelling that caught the FnDecl/Block-only first cut).
fn collect_any_letdecl_inits(stmts: &[Stmt], out: &mut std::collections::HashSet<ExprId>) {
    let one = |s: &Stmt, out: &mut std::collections::HashSet<ExprId>| {
        collect_any_letdecl_inits(std::slice::from_ref(s), out)
    };
    for s in stmts {
        match s {
            Stmt::LetDecl { type_ann, init, .. } => {
                if type_ann.as_deref() == Some("any") {
                    out.insert(*init);
                }
            }
            Stmt::FnDecl { body, .. } => collect_any_letdecl_inits(body, out),
            Stmt::Block(inner) | Stmt::Multi(inner) => collect_any_letdecl_inits(inner, out),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                one(then_branch, out);
                if let Some(e) = else_branch {
                    one(e, out);
                }
            }
            Stmt::While { body, .. }
            | Stmt::DoWhile { body, .. }
            | Stmt::Labeled { body, .. }
            | Stmt::ForOf { body, .. }
            | Stmt::ForOfSplitIter { body, .. } => one(body, out),
            Stmt::For { init, body, .. } => {
                if let Some(i) = init {
                    one(i, out);
                }
                one(body, out);
            }
            Stmt::Switch { cases, default, .. } => {
                for c in cases {
                    collect_any_letdecl_inits(&c.body, out);
                }
                if let Some(d) = default {
                    collect_any_letdecl_inits(d, out);
                }
            }
            Stmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                collect_any_letdecl_inits(body, out);
                collect_any_letdecl_inits(catch_body, out);
                if let Some(f) = finally_body {
                    collect_any_letdecl_inits(f, out);
                }
            }
            _ => {}
        }
    }
}
