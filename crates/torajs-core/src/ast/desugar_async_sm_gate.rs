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
/// - a dynamic `import()` — the imported module's top-level await
///   and the driver's resumption do not agree yet (642-15);
/// - `for await` and `await using` — their suspend points are inside
///   the iteration / disposal protocol, not at a statement the
///   `Stmt::Yield` rewrite can reach (blades 4 / 5). `await using`
///   has already become a `__usenv_N` dispose stack + try/finally by
///   this pass (`desugar_using` runs first), so it is recognised by
///   that environment's name; a body carrying one still keeps the
///   synchronous lowering (`using-await-001`: the SuppressedError
///   path through the generator's finally answered a TypeError);
/// (`let xs: T[] = await e` used to be listed here: through `yield`
/// the width analysis lost the link between the binding and the
/// awaited literal. The expansion now records the `this.__sent` init's
/// await origin — `Ast::yield_into_await_origins` — and the analysis
/// reads it as `e.value`, so the binding moves like any other.)
///
/// (`this` used to be listed here for a declaration and a value: the
/// generator is called plainly by the driver's thunk. Every blade now
/// rewrites the body's own `this` to the generator's leading receiver
/// parameter and hands the wrapper's `this` in — `desugar_async_sm_
/// this` — so a `.call` receiver, the sloppy global object and an
/// arrow's lexical `this` all arrive.)
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
pub(super) fn blocks_move(ast: &Ast, params: &[Param], body: &[Stmt]) -> bool {
    params.iter().any(|p| typed_default_param(p, ast))
        || !body
            .iter()
            .any(|s| super::desugar_async_sm::stmt_has_await(ast, s))
        || body.iter().any(|s| stmt_blocks_move(ast, s))
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

fn stmt_blocks_move(ast: &Ast, s: &Stmt) -> bool {
    if matches!(
        s,
        Stmt::ForOf { is_await: true, .. } | Stmt::UsingDecl { is_await: true, .. }
    ) {
        return true;
    }
    walk::stmt_exprs(s)
        .into_iter()
        .any(|e| expr_blocks_move(ast, e))
        || walk::stmt_children_ref(s)
            .into_iter()
            .any(|c| stmt_blocks_move(ast, c))
}

fn expr_blocks_move(ast: &Ast, eid: ExprId) -> bool {
    match ast.get_expr(eid) {
        Expr::Ident(n) if n == "arguments" || n.starts_with("__usenv_") => true,
        // A dynamic `import()` (the parser spells it as a call of
        // `__torajs_dyn_import`) — the imported module's own
        // top-level await and the driver's resumption have not been
        // made to agree: a body that suspended before a module it
        // imported reached its `await` never resumed
        // (`top-level-await/dynamic-import-of-waiting-module`, the
        // one pass regression left of blade 2's landing). The
        // synchronous lowering keeps that case right; the module
        // scheduling is registered (642-15).
        Expr::Ident(n) if n == "__torajs_dyn_import" => true,
        Expr::NewTarget => true,
        // A function expression / object-literal method is its own
        // function: its `arguments`, `new.target` and `this` are not
        // the body's, and its awaits are its own (`desugar_async_sm_
        // this::owns_this`).
        Expr::ArrowFn { .. } if super::desugar_async_sm_this::owns_this(ast, eid) => false,
        // An arrow's `arguments` / `new.target` / `this` are the
        // enclosing function's, so its body is scanned too — but a
        // nested `FnDecl` is its own function and `stmt_children_ref`
        // stops at it.
        Expr::ArrowFn { body, .. } => body.iter().any(|s| stmt_blocks_move(ast, s)),
        _ => walk::expr_children(ast, eid)
            .into_iter()
            .any(|c| expr_blocks_move(ast, c)),
    }
}

/// Does the body declare a function or a class anywhere (a nested
/// `FnDecl`, a class declaration — a class EXPRESSION is one by this
/// point, `__ClassExpr_<id>`), arrows' bodies included? Blade 2 hoists
/// a value's generator through `hoist_gen_fn_exprs`, whose capture
/// policy snapshots the enclosing locals for the generator alone: a
/// nested declaration that reads them is left with no binding
/// (`unknown identifier`), and a class declaration lifted as a
/// generator field is registered twice (`declares field __ClassExpr_6
/// twice`). A declaration's own body is not scanned — it is its own
/// function.
pub(super) fn declares_fn_or_class(ast: &Ast, body: &[Stmt]) -> bool {
    body.iter().any(|s| stmt_declares(ast, s))
}

fn stmt_declares(ast: &Ast, s: &Stmt) -> bool {
    if matches!(s, Stmt::FnDecl { .. } | Stmt::ClassDecl { .. }) {
        return true;
    }
    walk::stmt_exprs(s)
        .into_iter()
        .any(|e| expr_declares(ast, e))
        || walk::stmt_children_ref(s)
            .into_iter()
            .any(|c| stmt_declares(ast, c))
}

fn expr_declares(ast: &Ast, eid: ExprId) -> bool {
    match ast.get_expr(eid) {
        Expr::ArrowFn { body, .. } => body.iter().any(|s| stmt_declares(ast, s)),
        _ => walk::expr_children(ast, eid)
            .into_iter()
            .any(|c| expr_declares(ast, c)),
    }
}
