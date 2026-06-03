//! Generator class method emit helpers.
//!
//! P10.6-A1 — extracted from `ast::desugar_generators` so the
//! ~478-LOC desugar fn stays within touching distance of the
//! file-size hard limit even as P10.6 layers new Generator
//! prototype methods (`.return`, `.throw`, future iterator-
//! protocol surface).

use super::{Ast, ClassMethod, Expr, Param, Stmt, Visibility};

const RET_VAL_PARAM: &str = "__ret_val";
const THROW_ERR_PARAM: &str = "__err";

/// Emit `this.__state = -1;` — the sentinel that turns any
/// subsequent `next()` into a no-op (no state-machine arm
/// matches; control falls through to the tail
/// `{ value: <default>, done: true }`). Shared by `.return` and
/// `.throw` since both close the generator the same way.
fn emit_close_state(ast: &mut Ast) -> Stmt {
    let this_id = ast.add_expr(Expr::This);
    let state_member = ast.add_expr(Expr::Member {
        obj: this_id,
        name: "__state".into(),
    });
    let neg_one = ast.add_expr(Expr::Number(-1.0));
    let assign_state = ast.add_expr(Expr::Assign {
        target: state_member,
        value: neg_one,
    });
    Stmt::Expr(assign_state)
}

/// Build the `Generator.prototype.return(value)` method per ES
/// spec §27.5.1.7. Writes a sentinel `__state = -1` so any
/// subsequent `next()` falls through every state arm to the
/// `{ value: <default>, done: true }` tail, then returns
/// `{ value: arg, done: true }` directly.
///
/// Narrow MVP — no try/finally cleanup: J.2.b still forbids
/// `yield` inside `try` / `catch` / `finally`, so the spec's
/// "abrupt completion runs through any open finally" branch
/// has nothing to walk. P10.6-A2 lifts that restriction
/// together with `.throw` and will revisit the cleanup path.
pub(super) fn build_return_method(ast: &mut Ast, yield_ty: &str, step_ann: &str) -> ClassMethod {
    let val_param = Param {
        name: RET_VAL_PARAM.into(),
        type_ann: Some(yield_ty.into()),
        default: None,
        is_rest: false,
    };
    let close_stmt = emit_close_state(ast);
    let val_ident = ast.add_expr(Expr::Ident(RET_VAL_PARAM.into()));
    let done_true = ast.add_expr(Expr::Bool(true));
    let result = ast.add_expr(Expr::ObjectLit {
        fields: vec![("value".into(), val_ident), ("done".into(), done_true)],
    });
    ClassMethod {
        name: "return".into(),
        params: vec![val_param],
        return_type: Some(step_ann.into()),
        body: vec![close_stmt, Stmt::Return(Some(result))],
        is_abstract: false,
        visibility: Visibility::Public,
        accessor_kind: None,
    }
}

/// Build the `Generator.prototype.throw(err)` method per ES spec
/// §27.5.1.4. Force-closes the generator (same `__state = -1`
/// sentinel as `.return`) and rethrows `err` to the caller via
/// `Stmt::Throw`. The throw substrate (`torajs_throw_set` +
/// `emit_throw_check`) propagates the error to the innermost
/// enclosing try/catch in the caller's frame.
///
/// Narrow MVP — when J.2.b still forbids `yield` inside `try`,
/// the spec's "rethrow inside the generator body if a try block
/// wraps the suspended yield" branch has nothing to walk: the
/// throw simply propagates out of `next()`'s outer call as if
/// the caller's `throw` was at the call site. P10.6-A2-follow-up
/// (with the J.2.b lift) revisits this to inject the error at
/// the suspended yield position so an in-body `catch` can
/// observe it.
pub(super) fn build_throw_method(ast: &mut Ast, step_ann: &str) -> ClassMethod {
    let err_param = Param {
        name: THROW_ERR_PARAM.into(),
        type_ann: Some("any".into()),
        default: None,
        is_rest: false,
    };
    let close_stmt = emit_close_state(ast);
    let err_ident = ast.add_expr(Expr::Ident(THROW_ERR_PARAM.into()));
    ClassMethod {
        name: "throw".into(),
        params: vec![err_param],
        return_type: Some(step_ann.into()),
        body: vec![close_stmt, Stmt::Throw(err_ident)],
        is_abstract: false,
        visibility: Visibility::Public,
        accessor_kind: None,
    }
}
