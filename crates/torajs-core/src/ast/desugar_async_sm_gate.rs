//! Which async functions RFC 20260805 blade 1 moves — the conservative
//! gate in front of [`super::desugar_async_sm`]'s rewrite. Every shape
//! refused here keeps today's synchronous lowering, which is never
//! worse than today; moving it would be wrong. Split from the pass
//! when it crossed the file-size line (rotation 641).

use super::desugar_with::walk;
use super::{Ast, Expr, ExprId, Param, Stmt};

/// Shapes blade 1 does not move yet, each for a reason the wrapper's
/// plain forwarding call cannot honour:
///
/// - `arguments` anywhere in the body, arrows included (they inherit
///   it) — the wrapper forwards declared parameters only, so
///   `arguments[2]` of a call with three arguments would be gone;
/// - `new.target` — the generator is called plainly;
/// - `for await` and `await using` — their suspend points are inside
///   the iteration / disposal protocol, not at a statement the
///   `Stmt::Yield` rewrite can reach (blades 4 / 5). `await using`
///   has already become a `__usenv_N` dispose stack + try/finally by
///   this pass (`desugar_using` runs first), so it is recognised by
///   that environment's name; a body carrying one still keeps the
///   synchronous lowering (`using-await-001`: the SuppressedError
///   path through the generator's finally answered a TypeError);
/// - `let xs: T[] = await e` — an array-annotated await binding. The
///   synchronous lowering lets num_width tie the awaited literal's
///   element width to the binding's writes; through `yield` that
///   link is cut and `xs[0] = 1.5` on an int-born array is refused
///   (`arr-promise-elem-width-001`). Registered follow-up: tag the
///   YieldInto with its await origin and let num_width follow it.
///
/// - `this` in a plain async function (arrows included, they inherit
///   it) — the generator is called plainly by the driver's thunk, so
///   a receiver passed with `.call` or the sloppy global `this` would
///   be lost (test262 `async-function/evaluation-this-value-*`,
///   `dynamic-import/assignment-expression/this`). A method's `this`
///   is rewritten to the `__genrecv` channel before the move and is
///   not a reason to refuse ([`ThisRead::Rewritten`]);
/// - a body with no await at all — there is no suspend point to gain,
///   and moving it still reorders the microtask the driver's promise
///   settles on against the caller's (`async-disposable-stack-dispose-
///   sync-calls`: two `disposeAsync()` calls without await, observed
///   from the caller's own await).
///
/// Refusing keeps today's synchronous lowering for that function,
/// which is never worse than today; moving it would be wrong.
/// (Parameter defaults and rest parameters used to be listed here;
/// `wrapper_params` and the driver's thunk own them now.)
pub(super) fn blocks_move(ast: &Ast, params: &[Param], body: &[Stmt], this_read: ThisRead) -> bool {
    params.iter().any(|p| typed_default_param(p, ast))
        || !body
            .iter()
            .any(|s| super::desugar_async_sm::stmt_has_await(ast, s))
        || body.iter().any(|s| stmt_blocks_move(ast, s, this_read))
}

/// What a `this` inside the body means for the move.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ThisRead {
    /// A plain function's `this` has no channel to the generator.
    Blocks,
    /// A method's `this` becomes `__genrecv` before the move.
    Rewritten,
}

/// A defaulted parameter that is not spelled `any`. The wrapper
/// forwards a defaulted parameter as an `any` optional (see
/// `wrapper_params`), and the generator binds the default through the
/// callee-side guard that only an `any` slot carries
/// (`apply_args::defaulted_args`'s doc names the lanes). A typed slot
/// never does — and an UNANNOTATED one is typed too, from its
/// initializer (`infer_default_param_anns`): `ok(a: number, b = a + 1)`
/// moved that way answered `b` as NaN and never ran the initializer.
/// Only an explicit `any` moves; everything else keeps the synchronous
/// lowering until the typed lane binds defaults for a forwarded
/// `undefined`.
fn typed_default_param(p: &Param, ast: &Ast) -> bool {
    let Some(d) = p.default else {
        return false;
    };
    // An OPTIONAL parameter (`n?: T`, the parser's implicit default:
    // `undefined` or the `__undef_slot__T` sentinel) binds nothing —
    // the wrapper forwards the very value the generator's own optional
    // reads as "absent", so any type moves.
    if matches!(ast.get_expr(d), Expr::Ident(n)
        if n == "undefined" || n.starts_with(crate::ast::UNDEF_SLOT_MARKER))
    {
        return false;
    }
    match p.type_ann.as_deref() {
        Some("any" | "__nullable(any)") => false,
        // A boxed scalar optional (`n?: number`) reaches the generator
        // as an `any` actual, and the or_default lane binds a LITERAL
        // default for exactly that pairing (`ssa_lower_call_terminal_
        // coerce`); a computed default has no lane there yet.
        Some("number" | "__nullable(number)" | "boolean" | "__nullable(boolean)") => {
            !matches!(ast.get_expr(d), Expr::Number(_) | Expr::Bool(_))
        }
        _ => true,
    }
}

fn stmt_blocks_move(ast: &Ast, s: &Stmt, this_read: ThisRead) -> bool {
    if matches!(
        s,
        Stmt::ForOf { is_await: true, .. } | Stmt::UsingDecl { is_await: true, .. }
    ) {
        return true;
    }
    if let Stmt::LetDecl {
        type_ann: Some(ann),
        init,
        ..
    } = s
        && ast.await_value_reads.contains(init)
        && (ann.ends_with("[]") || ann.starts_with("Array<"))
    {
        return true;
    }
    walk::stmt_exprs(s)
        .into_iter()
        .any(|e| expr_blocks_move(ast, e, this_read))
        || walk::stmt_children_ref(s)
            .into_iter()
            .any(|c| stmt_blocks_move(ast, c, this_read))
}

fn expr_blocks_move(ast: &Ast, eid: ExprId, this_read: ThisRead) -> bool {
    match ast.get_expr(eid) {
        Expr::Ident(n) if n == "arguments" || n.starts_with("__usenv_") => true,
        Expr::NewTarget => true,
        Expr::This => this_read == ThisRead::Blocks,
        // An arrow's `arguments` / `new.target` / `this` are the
        // enclosing function's, so its body is scanned too — but a
        // nested `FnDecl` is its own function and `stmt_children_ref`
        // stops at it.
        Expr::ArrowFn { body, .. } => body.iter().any(|s| stmt_blocks_move(ast, s, this_read)),
        _ => walk::expr_children(ast, eid)
            .into_iter()
            .any(|c| expr_blocks_move(ast, c, this_read)),
    }
}
