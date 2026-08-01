//! Generator class method emit helpers.
//!
//! P10.6-A1 — extracted from `ast::desugar_generators` so the
//! ~478-LOC desugar fn stays within touching distance of the
//! file-size hard limit even as P10.6 layers new Generator
//! prototype methods (`.return`, `.throw`, future iterator-
//! protocol surface).

use super::{Ast, ClassMethod, Expr, ExprId, Param, Stmt, Visibility};

const RET_VAL_PARAM: &str = "__ret_val";
const THROW_ERR_PARAM: &str = "__err";
const YIELD_ARG_PARAM: &str = "__yield_arg";

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
        span: crate::lexer::Span { start: 0, end: 0 },
    }
}

/// Build the `Generator.prototype.throw(err)` method per ES spec
/// §27.5.1.4. Force-closes the generator (same `__state = -1`
/// sentinel as `.return`) and rethrows `err` to the caller via
/// `Stmt::Throw`. The throw substrate (`torajs_throw_set` +
/// `emit_throw_check`) propagates the error to the innermost
/// enclosing try/catch in the caller's frame.
///
/// RFC 20260802 C2 — a REGION-BEARING generator (`has_try_regions`)
/// instead injects: `this.__thrown = err; this.__inject = true;
/// return this.next();`. next()'s prologue throws the stash at the
/// suspended state, so a try wrapping the suspended yield observes
/// it (§27.5.1.4 resume-with-throw-completion); a state in no region
/// rethrows out of next() — same caller-visible shape as the close
/// path, with the prologue kill supplying the "completed" sentinel.
///
/// Region-FREE generators keep the close+rethrow shape: no body try
/// can observe the error, and skipping injection keeps their class
/// free of the `__inject` / `__thrown` fields.
pub(super) fn build_throw_method(
    ast: &mut Ast,
    step_ann: &str,
    has_try_regions: bool,
) -> ClassMethod {
    let err_param = Param {
        name: THROW_ERR_PARAM.into(),
        type_ann: Some("any".into()),
        default: None,
        is_rest: false,
    };
    let body = if has_try_regions {
        let this_id = ast.add_expr(Expr::This);
        let thrown = ast.add_expr(Expr::Member {
            obj: this_id,
            name: "__thrown".into(),
        });
        let err_ident = ast.add_expr(Expr::Ident(THROW_ERR_PARAM.into()));
        let stash = ast.add_expr(Expr::Assign {
            target: thrown,
            value: err_ident,
        });
        let this_arm = ast.add_expr(Expr::This);
        let inj = ast.add_expr(Expr::Member {
            obj: this_arm,
            name: "__inject".into(),
        });
        let true_lit = ast.add_expr(Expr::Bool(true));
        let arm = ast.add_expr(Expr::Assign {
            target: inj,
            value: true_lit,
        });
        // `this.next()` — the arg pads via apply_default_args like
        // every user `it.next()` call site.
        let this_recv = ast.add_expr(Expr::This);
        let next_callee = ast.add_expr(Expr::Member {
            obj: this_recv,
            name: "next".into(),
        });
        let call = ast.add_expr(Expr::Call {
            callee: next_callee,
            args: Vec::new(),
        });
        vec![Stmt::Expr(stash), Stmt::Expr(arm), Stmt::Return(Some(call))]
    } else {
        let close_stmt = emit_close_state(ast);
        let err_ident = ast.add_expr(Expr::Ident(THROW_ERR_PARAM.into()));
        vec![close_stmt, Stmt::Throw(err_ident)]
    };
    ClassMethod {
        name: "throw".into(),
        params: vec![err_param],
        return_type: Some(step_ann.into()),
        body,
        is_abstract: false,
        visibility: Visibility::Public,
        accessor_kind: None,
        span: crate::lexer::Span { start: 0, end: 0 },
    }
}

/// Build the `Generator.prototype.next(arg)` method. Prepends a
/// `this.__sent = __yield_arg;` stash so YieldInto-expanded reads
/// of `this.__sent` see whatever the caller passed to `g.next(arg)`
/// on the resume; the rest of the method body is the caller-built
/// state-machine `while(true) { ... }` + tail return.
///
/// `yield_arg_default_id` carries the type-driven default value
/// (built via `default_init_for_type` at the caller — kept caller-
/// side so this sibling doesn't need to import that ast-internal
/// helper just to forward an ExprId).
pub(super) fn build_next_method(
    ast: &mut Ast,
    yield_arg_default_id: ExprId,
    yield_ty: &str,
    step_ann: &str,
    state_machine_body: Vec<Stmt>,
) -> ClassMethod {
    let yield_arg_param = Param {
        name: YIELD_ARG_PARAM.into(),
        type_ann: Some(yield_ty.into()),
        default: Some(yield_arg_default_id),
        is_rest: false,
    };
    let stash_sent = {
        let this_id = ast.add_expr(Expr::This);
        let sent_member = ast.add_expr(Expr::Member {
            obj: this_id,
            name: "__sent".into(),
        });
        let arg_ident = ast.add_expr(Expr::Ident(YIELD_ARG_PARAM.into()));
        let assign = ast.add_expr(Expr::Assign {
            target: sent_member,
            value: arg_ident,
        });
        Stmt::Expr(assign)
    };
    let mut body: Vec<Stmt> = Vec::with_capacity(state_machine_body.len() + 1);
    body.push(stash_sent);
    body.extend(state_machine_body);
    ClassMethod {
        name: "next".into(),
        params: vec![yield_arg_param],
        return_type: Some(step_ann.into()),
        body,
        is_abstract: false,
        visibility: Visibility::Public,
        accessor_kind: None,
        span: crate::lexer::Span { start: 0, end: 0 },
    }
}
