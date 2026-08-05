//! §20.5.8.1 InstallErrorCause — the `options` / `cause` face shared
//! by every synthesized Error ctor. Sibling of
//! `inject_builtin_classes` (the parent sat at 413 lines and this
//! family would have carried it past the 500 limit), on the same
//! split as `inject_builtin_classes_data`: one spec section, one
//! file.
//!
//! `Error`'s ctor holds the single copy of the test; the NativeError
//! subclasses and the data-carrying ones (AggregateError /
//! SuppressedError) only forward `options` to it through `super`.

use super::{Ast, BinOp, Expr, ExprId, Param, Stmt};

/// §20.5.8.1 — the `options` param every Error ctor accepts after
/// `message`, defaulting to `undefined` (`Expr::Ident("undefined")`
/// is how the parser spells the literal, so synthesized code spells
/// it the same way). Typed `any`: the spec accepts any value here and
/// only inspects it when it turns out to be an Object.
pub(super) fn build_options_param(ast: &mut Ast) -> Param {
    let undef = ast.add_expr(Expr::Ident("undefined".to_string()));
    Param {
        name: "options".to_string(),
        type_ann: Some("any".to_string()),
        default: Some(undef),
        is_rest: false,
    }
}

/// §20.5.8.1 InstallErrorCause — the tail every Error ctor runs:
///
/// ```ts
/// if (options !== null
///     && (typeof options === "object" || typeof options === "function")
///     && "cause" in options) {
///   __torajs_error_install_cause(this, options.cause);
/// }
/// ```
///
/// Three things the spec pins that a shorter spelling would get
/// wrong:
///
/// - The test is **HasProperty**, not a defined value. `new
///   Error("m", { cause: undefined })` owns a `cause` property whose
///   value is `undefined`, and is observably different from `new
///   Error("m")`, which owns none. `options.cause !== undefined`
///   cannot tell those apart.
/// - The guard is **"is an Object"**, which includes callables — hence
///   the `typeof === "function"` arm, not `typeof === "object"` alone.
///   It has to run first: `in` throws a TypeError on a primitive
///   (verified: tr and bun both throw on `"a" in undefined`), and
///   `&&` short-circuits before that can happen.
/// - The property is installed **conditionally**, so it cannot be a
///   declared class field — a field exists on every instance and would
///   make `"cause" in new Error("m")` answer true. Class instances are
///   static-layout structs, so the entry lands in the receiver's
///   expando dict.
/// - The attributes are `{W:1, E:0, C:1}`
///   (CreateNonEnumerableDataPropertyOrThrow), which an assignment
///   cannot express — that is why this is a call and not
///   `(this as any).cause = ...`. A user's own `err.cause = x` after
///   construction IS an ordinary enumerable assignment, and bun
///   reports exactly that difference.
///
/// `in` has no `Expr` variant of its own: the parser emits a call to
/// `__torajs_in_op(key, obj)` that check/ssa_lower intercept by name
/// (T-45), so synthesized code builds that call directly.
pub(super) fn build_install_cause(ast: &mut Ast) -> Stmt {
    // options !== null
    let opts_null = ast.add_expr(Expr::Ident("options".to_string()));
    let null_lit = ast.add_expr(Expr::Null);
    let not_null = ast.add_expr(Expr::BinOp {
        op: BinOp::Neq,
        left: opts_null,
        right: null_lit,
    });

    // typeof options === "object"
    let typeof_obj = build_typeof_eq(ast, "object");
    // typeof options === "function"
    let typeof_fn = build_typeof_eq(ast, "function");
    let is_object = ast.add_expr(Expr::BinOp {
        op: BinOp::LOr,
        left: typeof_obj,
        right: typeof_fn,
    });

    // "cause" in options
    let key = ast.add_expr(Expr::String("cause".to_string()));
    let opts_in = ast.add_expr(Expr::Ident("options".to_string()));
    let in_callee = ast.add_expr(Expr::Ident("__torajs_in_op".to_string()));
    let has_cause = ast.add_expr(Expr::Call {
        callee: in_callee,
        args: vec![key, opts_in],
    });

    let guard = ast.add_expr(Expr::BinOp {
        op: BinOp::LAnd,
        left: not_null,
        right: is_object,
    });
    let cond = ast.add_expr(Expr::BinOp {
        op: BinOp::LAnd,
        left: guard,
        right: has_cause,
    });

    // __torajs_error_install_cause(this, options.cause);
    let this_expr = ast.add_expr(Expr::This);
    let opts_read = ast.add_expr(Expr::Ident("options".to_string()));
    let value = ast.add_expr(Expr::Member {
        obj: opts_read,
        name: "cause".to_string(),
    });
    let callee = ast.add_expr(Expr::Ident("__torajs_error_install_cause".to_string()));
    let install = ast.add_expr(Expr::Call {
        callee,
        args: vec![this_expr, value],
    });

    Stmt::If {
        cond,
        then_branch: Box::new(Stmt::Block(vec![Stmt::Expr(install)])),
        else_branch: None,
    }
}

/// `typeof options === "<want>"` — the two arms of the
/// [`build_install_cause`] Object test.
fn build_typeof_eq(ast: &mut Ast, want: &str) -> ExprId {
    let opts = ast.add_expr(Expr::Ident("options".to_string()));
    let t = ast.add_expr(Expr::TypeOf { expr: opts });
    let lit = ast.add_expr(Expr::String(want.to_string()));
    ast.add_expr(Expr::BinOp {
        op: BinOp::Eq,
        left: t,
        right: lit,
    })
}
