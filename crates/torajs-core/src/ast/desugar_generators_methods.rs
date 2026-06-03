//! Generator class method emit helpers.
//!
//! P10.6-A1 — extracted from `ast::desugar_generators` so the
//! ~478-LOC desugar fn stays within touching distance of the
//! file-size hard limit even as P10.6 layers new Generator
//! prototype methods (`.return`, `.throw`, future iterator-
//! protocol surface).

use super::{Ast, ClassMethod, Expr, Param, Stmt, Visibility};

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
pub(super) fn build_return_method(
    ast: &mut Ast,
    yield_ty: &str,
    step_ann: &str,
) -> ClassMethod {
    let val_param = Param {
        name: "__ret_val".into(),
        type_ann: Some(yield_ty.into()),
        default: None,
        is_rest: false,
    };
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
    let val_ident = ast.add_expr(Expr::Ident("__ret_val".into()));
    let done_true = ast.add_expr(Expr::Bool(true));
    let result = ast.add_expr(Expr::ObjectLit {
        fields: vec![("value".into(), val_ident), ("done".into(), done_true)],
    });
    ClassMethod {
        name: "return".into(),
        params: vec![val_param],
        return_type: Some(step_ann.into()),
        body: vec![Stmt::Expr(assign_state), Stmt::Return(Some(result))],
        is_abstract: false,
        visibility: Visibility::Public,
        accessor_kind: None,
    }
}
