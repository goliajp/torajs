//! The slot type a lifted local takes when its declaration wrote no
//! annotation, read off the initializer.

use crate::ast::desugar_generators_walkers::LiftCtx;
use crate::ast::{Ast, Expr, ExprId, Stmt};

pub(super) fn local_ann(
    ast: &Ast,
    name: &str,
    type_ann: Option<&String>,
    init: ExprId,
    ctx: &LiftCtx,
) -> String {
    type_ann.cloned().unwrap_or_else(|| {
        // Knife 2a — an untyped alias of `arguments` lifts as
        // `any`: the capture rewrite turns the init into the
        // any-typed `this.arguments` field read, and the
        // historical "number" fallback would pin the lifted
        // field's type wrong (`.length on Number`).
        if matches!(ast.get_expr(init), Expr::Ident(nm)
            if nm == "arguments" || nm == crate::ast::GEN_ARGV_PARAM)
        {
            "any".into()
        } else if crate::ast::free_vars::free_vars_of_body(
            ast,
            &[],
            std::slice::from_ref(&Stmt::Expr(init)),
        )
        .iter()
        .any(|n| n.starts_with("__yx_"))
        {
            // RFC 20260802-yield-expr-hoist — an untyped local
            // whose init reads a hoisted yield-resumption temp
            // (`__yx_*`, always any) rides the any lane; the
            // "number" fallback would pin the lifted field's
            // type against whatever next() actually sends.
            "any".into()
        } else if matches!(ast.get_expr(init), Expr::Uninit) {
            // 422-01 — `let v;` (no init, no ann): the binding
            // starts undefined and takes whatever a later
            // assignment sends (`v = yield ...` most often).
            // Pinning number cannot even hold the initial
            // undefined; the field is an any slot by shape.
            "any".into()
        } else if name.starts_with("__forof_src_")
            || name.starts_with("__forof_destr_")
            || name.starts_with("__ary_src_")
            || name.starts_with("__nested_destr_")
        {
            // r283 — the parser's for-of / destructure desugar
            // temps carry NO annotation (checker-inferred
            // outside a generator: the hoisted source array,
            // the destructured loop element, and the pattern-
            // unpack aliases); lifted to fields the "number"
            // fallback pinned them against their array/element
            // inits (t262 for-await-of dstr family, 366-case
            // cluster "field is Number, value is Array(...)"
            // plus the plain-generator `for (const [a] of
            // [[7]])` shape). `__forof_i_N` stays on the
            // number fallback — it really is the counter.
            "any".into()
        } else {
            // D0 (RFC 20260805-async-fn-state-machine) — ask
            // the initializer. `number` was right for the loop
            // counters a hand-written generator lifts and
            // wrong for everything else a body can hold: a
            // `function*` with `const xs = [1, 2]` in it does
            // not compile today ("field is Number, value is
            // Array(Number)"). The carve-outs above outrank
            // this — each is a deliberate `any` for a slot
            // whose value the syntax cannot see (a resumption
            // temp, an `arguments` alias) — and `number`
            // stays as what is left when the sniff declines,
            // so every shape it cannot read keeps today's
            // behaviour.
            crate::ast::desugar_generators_field_ann::field_ann(ast, init, ctx).unwrap_or_else(
                || {
                    // r454 / r633 / r642 — a source the sniff
                    // cannot read (a dstr-assignment temp, a
                    // free Ident, a builtin call, a fn-expr, a
                    // thenable literal, a Boolean off a class
                    // instance) is an `any` slot: `number` here
                    // was a guess, and each wrong guess was a
                    // refused field. Only the parser's for-of
                    // counter temp is known to be a number.
                    if name.starts_with("__forof_i_") {
                        "number".into()
                    } else {
                        "any".into()
                    }
                },
            )
        }
    })
}
