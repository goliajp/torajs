//! The `<name> as any` callback slot of an array HOF call, split out
//! of [`super`] at the file-size line.

use crate::ast::fnexpr_this_names::peel_as;
use crate::ast::fnexpr_this_recvs::{collect_any_binding_names, collect_arraylit_binding_names};
use crate::ast::{Expr, ExprId, Stmt};

/// The `<name> as any` callback slot of an array HOF call whose
/// receiver is any-lane-certain (401-03).
///
/// This is the escaping proof family (the explicit-`any` argument /
/// any-boundary return shapes): the cell crosses into the any lane
/// and stays there, because every any-lane call path shifts argv on
/// FLAG_CLOSURE_RECV_FIRST. Three links, each carried by a syntactic
/// certainty:
///
/// 1. the `as any` shell makes the checker see `Any` in the slot, so
///    the call routes through the any-lane HOF wedge
///    (`check_type_of_call_hof_any_cb`) on a typed receiver and
///    the any-method route on an `any` one — never the typed inline
///    loop, whose argv does not shift;
/// 2. the receiver is an inline array literal or a binding the
///    knife-4 certainty collectors prove only ever holds one
///    (`collect_arraylit_binding_names` / an `: any` binding) — a
///    user object with its own `map` method decides its own call
///    protocol and stays out;
/// 3. the method set mirrors the wedge's — the ones whose runtime
///    kernels (`__torajs_arr_any_map` and friends, `find_loop`'s
///    backward modes, the flatMap walk) seed argv[0] off the header
///    flag.
///
/// The BARE spelling (`xs.map(cb, t)`) is deliberately not here: it
/// types at the closure's signature and rides the typed knife-4
/// lanes, which thread the receiver through `sig_skip` instead.
pub(super) fn hof_any_cb_arg_idents(
    stmts: &[Stmt],
    exprs: &[Expr],
) -> std::collections::HashSet<ExprId> {
    let any_recvs = collect_any_binding_names(stmts, exprs);
    let arraylit_recvs = collect_arraylit_binding_names(stmts, exprs);
    let mut out = std::collections::HashSet::new();
    for e in exprs {
        let Expr::Call { callee, args } = e else {
            continue;
        };
        let Expr::Member { obj, name } = &exprs[callee.0 as usize] else {
            continue;
        };
        if !matches!(
            name.as_str(),
            "map"
                | "filter"
                | "forEach"
                | "every"
                | "some"
                | "find"
                | "findIndex"
                | "findLast"
                | "findLastIndex"
                | "flatMap"
                | "reduce"
                | "reduceRight"
        ) || args.is_empty()
        {
            continue;
        }
        // Only the As-wrapped spelling — see the bare-spelling note.
        let Expr::As { expr, ty_ann } = &exprs[args[0].0 as usize] else {
            continue;
        };
        if ty_ann != "any" {
            continue;
        }
        let inner = peel_as(exprs, *expr);
        if !matches!(&exprs[inner.0 as usize], Expr::Ident(_)) {
            continue;
        }
        let obj = peel_as(exprs, *obj);
        let recv_ok = matches!(&exprs[obj.0 as usize], Expr::Array(_))
            || matches!(&exprs[obj.0 as usize], Expr::Ident(n)
                if any_recvs.contains(n) || arraylit_recvs.contains(n));
        if recv_ok {
            out.insert(inner);
        }
    }
    out
}
