//! `scan_call` — the `Expr::Call` arm of `ast_throw_info::scan_expr`.
//!
//! Split out from `ast_throw_info.rs` when that file reached the
//! 500-line limit. A pure relocation: the body below is the chunk-773
//! extraction moved verbatim, one directory level of visibility wider
//! so its single caller can still reach it.

use crate::ast::{Ast, Expr, ExprId};
use crate::ast_throw_info::scan_expr;
use std::collections::{HashMap, HashSet};

/// `Expr::Call` arm of [`scan_expr`] (chunk 773 extraction) — the
/// callee-shape dispatch deciding between named-target recording
/// and the conservative may-throw bit, plus the callback-argument
/// closure recording.
pub(crate) fn scan_call(
    ast: &Ast,
    callee: ExprId,
    args: &[ExprId],
    out: &mut Vec<String>,
    direct: &mut bool,
    fn_values: &HashSet<String>,
    expr_types: &HashMap<ExprId, crate::check::Type>,
) {
    match ast.get_expr(callee) {
        Expr::Ident(name) => {
            if !out.contains(name) {
                out.push(name.clone());
            }
            // P7.4-a-b — `BigInt(x)` throws a real RangeError on a
            // non-integer / non-finite argument (runtime_bigint.c →
            // __torajs_throw_range_error). Conservatively flag every
            // BigInt() call as may-throw: such calls are rare and
            // never hot-path, so the over-approximation costs at
            // most one cold-path throw-check while guaranteeing the
            // RangeError can't be silently swallowed across a fn
            // boundary. `BigInt` is a global ctor, not user-named,
            // so it never enters `out` (called user-fn names).
            if name == "BigInt" {
                *direct = true;
            }
            // `Number(x)` / `String(x)` over an Any/object argument
            // run OrdinaryToPrimitive at runtime, which records a
            // pending TypeError when both toString/valueOf answer
            // objects (§7.1.4 / §7.1.17) or when a user hook throws.
            // Primitive-typed args take never-throwing typed lanes,
            // so gate on the arg's static type to keep hot numeric
            // code out of `may_throw` (an unknown type flags
            // conservatively — a miss is a silent swallow, a false
            // positive is one cold throw-check).
            if (name == "Number" || name == "String")
                && args.first().is_some_and(|a| {
                    !matches!(
                        expr_types.get(a),
                        Some(
                            crate::check::Type::Number
                                | crate::check::Type::String
                                | crate::check::Type::Boolean
                        )
                    )
                })
            {
                *direct = true;
            }
            // RFC 20260718-error-message-own-prop 刀 3 — the
            // no-super ReferenceError raiser the class desugar
            // appends to super-less derived ctors records a pending
            // throw; without this bit the ctor (and its factory /
            // `new` site) is judged never-throwing and the pending
            // throw is silently stranded.
            if name == "__torajs_ctor_no_super_throw" {
                *direct = true;
            }
            // Rotation 297 — the parser's synthetic relational calls
            // throw: `in` records a §13.10.1 step-5 TypeError on a
            // non-Object rhs, and the `#x in o` brand check does the
            // same. Without this bit a fn whose ONLY throw source is
            // a bare `return #x in o` is judged never-throwing, the
            // caller prunes its check, and the pending throw strands
            // — poisoning the NEXT checked call into a bogus early
            // return (probe answered `false` for a receiver that
            // carries the field).
            if name == "__torajs_in_op" || name == "__torajs_priv_in_op" {
                *direct = true;
            }
            // bug-327 C2.5 — indirect call through a fn-valued
            // binding: the target is statically unknown, so the
            // fn must conservatively count as may-throw.
            if fn_values.contains(name) {
                *direct = true;
            }
        }
        // RC-4 arguments-object — IIFE: the callee closure is its
        // own lifted FnDecl; record the lifted name so the fixed-
        // point propagates its throw bit into this fn. Before this
        // arm the IIFE call fell out of the analysis entirely, so
        // a throw inside `(function () { throw x; })()` nested in
        // a named fn was swallowed at the caller's caller (the
        // caller's emit_throw_check was M4.3.b-skipped).
        Expr::Closure { fn_name, .. } => {
            if !out.contains(fn_name) {
                out.push(fn_name.clone());
            }
        }
        // Chained call `f(0)(5)` / direct arrow-IIFE — the target
        // is statically unknown: conservative may-throw (mirrors
        // the fn-valued-binding rule above).
        Expr::Call { .. } | Expr::ArrowFn { .. } => {
            *direct = true;
        }
        // Chunk 701 — method calls: many method lowerings bottom
        // out in runtime helpers that record pending throws
        // (kind-mismatch mutators / any_to_typed / JSON.parse /
        // index-assign OOB / any member dispatch — 68 in-body
        // throw-check sites across the lowerers). Mirroring each
        // dispatch condition here is unmaintainable and a missed
        // entry is a silent swallow, so the default flips to
        // may-throw with an exempt-list of known never-throwing
        // hot surfaces (an over-wide exempt entry is a swallow;
        // an over-narrow one is a single cold throw-check —
        // exemption is the safe direction). Pure-arithmetic hot
        // fns (fib / gcd / mandelbrot) have no method calls and
        // keep the M4.3.b skip.
        Expr::Member { obj, .. } => {
            let exempt = matches!(ast.get_expr(*obj), Expr::Ident(n) if n == "console");
            if !exempt {
                *direct = true;
            }
        }
        // `xs[i]()` / `a?.b()` — fn-valued targets that are
        // statically unknown, same conservatism as chained calls.
        Expr::Index { .. } | Expr::OptChain { .. } | Expr::OptIndex { .. } => {
            *direct = true;
        }
        _ => {}
    }
    scan_expr(ast, callee, out, direct, fn_values, expr_types);
    for a in args {
        // RC-4 replace A1_T4 — a closure literal passed as a
        // call argument is a callback the callee invokes
        // (stdlib protocols: replace / map / forEach / ...);
        // record its lifted FnDecl name so its throw bit
        // propagates into this fn. A callee that never
        // invokes it over-approximates by at most one
        // cold-path throw-check.
        if let Expr::Closure { fn_name, .. } = ast.get_expr(*a)
            && !out.contains(fn_name)
        {
            out.push(fn_name.clone());
        }
        scan_expr(ast, *a, out, direct, fn_values, expr_types);
    }
}
