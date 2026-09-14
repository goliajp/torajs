//! The receiver-honoring CHANNEL classifier — the fourth
//! receiver-safe use shape of a knife-2 promotion candidate, split
//! out of `fnexpr_this_args.rs` in rotation 632 when that file hit
//! the 500-line limit and this family was the one with a name of its
//! own. Answers one question: which builtins reach a function
//! argument only through a path that honors the receiver channel,
//! and which argument nodes ride them.
//!
//! [`super::fnexpr_this_shapes`] holds the census that consumes this;
//! [`super::fnexpr_this_routed`] decides whether a binding qualifies.

use super::{Expr, ExprId};

/// The callee whitelist backing the fourth receiver-safe use shape:
/// builtins that reach a function argument ONLY through a
/// receiver-honoring channel. Named for the channel, not for
/// "construct" — the set was construct-only when it was written
/// (rotation 345) and the proof each entry carries is about the
/// receiver, which is why `Reflect.apply` belongs in it too.
///
/// Today: `Reflect.construct` and the `Array.from` / `Array.of`
/// statics re-dispatched through `.call` / `.apply`
/// (their ns-static cells are recv-first, so the thisArg rides argv[0]
/// into the kernel's Construct split).
///
/// `of` joins in rotation 417 on the same reading as `from`: §23.1.2.3
/// step 4.a does `Construct(C, «len»)` with the `.call` thisArg as C,
/// and does nothing else with it. `Array.of.call(A, 2)` — a t262
/// spelling for "does the array-like allocation go through `A` as a
/// constructor" — is how it shows up, and `A` there is normally a
/// `this`-writing function expression.
fn receiver_honoring_callee(exprs: &[Expr], callee: ExprId) -> bool {
    match &exprs[callee.0 as usize] {
        // Rotation 410 — the value-shaped-parent synthetics: the
        // super dispatch reaches a closure only through
        // `invoke_with_this` (receiver-honoring), and the heritage
        // gate never invokes its argument at all (§15.7.14 step 5 is
        // a header read).
        Expr::Ident(n) if n == "__torajs_super_value" || n == "__torajs_heritage_check" => true,
        // §28.1.1 `Reflect.apply(target, thisArg, argumentsList)` —
        // the kernel hands `thisArg` to `target` through
        // `method_call_closure_apply_like::apply_list`, the same
        // dispatch `f.apply(o, …)` rides, which shifts argv on
        // FLAG_CLOSURE_RECV_FIRST. The `thisArg` and `argumentsList`
        // arguments join the set too and cost nothing: the kernel
        // never calls either one.
        //
        // The `Reflect` test sits in the GUARD, not the body. An arm
        // matching the member name alone swallows `Array.of.apply(C,
        // …)` and answers false, where the `Array.from` / `of` arm
        // below would have answered true — the gate caught exactly
        // that (`array-of-construct-receiver-001`).
        Expr::Member { obj, name }
            if (name == "construct" || name == "apply")
                && matches!(&exprs[obj.0 as usize], Expr::Ident(n) if n == "Reflect") =>
        {
            true
        }
        Expr::Member { obj, name } if name == "call" || name == "apply" => {
            matches!(&exprs[obj.0 as usize], Expr::Member { obj: ns, name: m }
                if (m == "from" || m == "of")
                    && matches!(&exprs[ns.0 as usize], Expr::Ident(n) if n == "Array"))
        }
        _ => false,
    }
}

/// RFC 20260808-construct-channel B6 刀 2 — an argument handed to a
/// RECEIVER-HONORING builtin (`Reflect.construct(C, …)`,
/// `Array.from.call(C, …)` / `.apply`) is the
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
pub(super) fn receiver_honoring_arg_idents(exprs: &[Expr]) -> std::collections::HashSet<ExprId> {
    let mut out: std::collections::HashSet<ExprId> = exprs
        .iter()
        .filter_map(|e| match e {
            Expr::Call { callee, args } if receiver_honoring_callee(exprs, *callee) => Some(
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
