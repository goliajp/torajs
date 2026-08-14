//! Generator class method emit helpers.
//!
//! P10.6-A1 — extracted from `ast::desugar_generators` so the
//! ~478-LOC desugar fn stays within touching distance of the
//! file-size hard limit even as P10.6 layers new Generator
//! prototype methods (`.return`, `.throw`, future iterator-
//! protocol surface).

use super::{Ast, ClassMethod, Expr, ExprId, Param, Stmt, Visibility, default_init_for_type};

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

/// `this.next(<the same default `next` itself declares>)` — the
/// internal re-entry the region-bearing `.return` / `.throw` shapes
/// use to resume the state machine.
///
/// The argument is passed EXPLICITLY rather than left to
/// `apply_default_args`. That pass keys class-method defaults by bare
/// name whenever it cannot resolve the receiver, and `this` inside a
/// synthesized `__cm_*` body is exactly such a receiver — so a
/// program containing any second owner of the name `next` (a user
/// `class Cursor { next(step = 5) {} }`, or simply a generator on the
/// other yield-type lane) evicted the shared entry and left this call
/// unpadded: "expected 1 argument(s), got 0", reported against a call
/// the compiler wrote itself. The value handed over is identical
/// either way; what goes away is the dependency on a global table.
fn emit_self_next_call(ast: &mut Ast, yield_ty: &str) -> ExprId {
    let arg = ast.add_expr(default_init_for_type(yield_ty));
    let this_recv = ast.add_expr(Expr::This);
    let next_callee = ast.add_expr(Expr::Member {
        obj: this_recv,
        name: "next".into(),
    });
    ast.add_expr(Expr::Call {
        callee: next_callee,
        args: vec![arg],
    })
}

/// Build the `Generator.prototype.return(value)` method per ES
/// spec §27.5.1.7. Writes a sentinel `__state = -1` so any
/// subsequent `next()` falls through every state arm to the
/// `{ value: <default>, done: true }` tail, then returns
/// `{ value: arg, done: true }` directly.
///
/// RFC 20260802 D3b — a generator with a return-injectable finally
/// region (`has_finally_ret`) instead delegates, the dual of the C2
/// throw() shape: `this.__ret_inj = v; this.__ret_pending = true;
/// return this.next();`. next()'s dispatch return-check routes a
/// suspended-in-finally-region state to that region's D3a return
/// copy (F runs — may yield — then completes with the stashed
/// value, chaining outer frames), and completes directly otherwise
/// — same caller-visible shape as the close path.
///
/// Generators without such a region keep the close+return shape: no
/// finally can observe the abrupt completion, and skipping the
/// delegate keeps their class free of the `__ret_*` fields.
pub(super) fn build_return_method(
    ast: &mut Ast,
    yield_ty: &str,
    step_ann: &str,
    has_finally_ret: bool,
    is_async_gen: bool,
) -> ClassMethod {
    let val_param = Param {
        name: RET_VAL_PARAM.into(),
        type_ann: Some(yield_ty.into()),
        default: None,
        is_rest: false,
    };
    // §27.6.3.7 step 8.d — an ASYNC generator's return(v) awaits v
    // before completing with it: prepend `__ret_val = await
    // __ret_val;` (the await-read is type-dispatched — a Promise
    // unwraps, anything else passes through identity). Sync
    // generators complete with v verbatim (§27.5.1.7).
    let await_stash = if is_async_gen {
        let inner = ast.add_expr(Expr::Ident(RET_VAL_PARAM.into()));
        let read = ast.add_expr(Expr::Member {
            obj: inner,
            name: "value".into(),
        });
        ast.await_value_reads.insert(read);
        let target = ast.add_expr(Expr::Ident(RET_VAL_PARAM.into()));
        let assign = ast.add_expr(Expr::Assign {
            target,
            value: read,
        });
        Some(Stmt::Expr(assign))
    } else {
        None
    };
    let body = if has_finally_ret {
        let this_id = ast.add_expr(Expr::This);
        let inj = ast.add_expr(Expr::Member {
            obj: this_id,
            name: "__ret_inj".into(),
        });
        let val_ident = ast.add_expr(Expr::Ident(RET_VAL_PARAM.into()));
        let stash = ast.add_expr(Expr::Assign {
            target: inj,
            value: val_ident,
        });
        let this_arm = ast.add_expr(Expr::This);
        let pending = ast.add_expr(Expr::Member {
            obj: this_arm,
            name: "__ret_pending".into(),
        });
        let true_lit = ast.add_expr(Expr::Bool(true));
        let arm = ast.add_expr(Expr::Assign {
            target: pending,
            value: true_lit,
        });
        let call = emit_self_next_call(ast, yield_ty);
        vec![Stmt::Expr(stash), Stmt::Expr(arm), Stmt::Return(Some(call))]
    } else {
        let close_stmt = emit_close_state(ast);
        let val_ident = ast.add_expr(Expr::Ident(RET_VAL_PARAM.into()));
        let done_true = ast.add_expr(Expr::Bool(true));
        let result = ast.add_expr(Expr::ObjectLit {
            fields: vec![("value".into(), val_ident), ("done".into(), done_true)],
        });
        vec![close_stmt, Stmt::Return(Some(result))]
    };
    let body: Vec<Stmt> = await_stash.into_iter().chain(body).collect();
    ClassMethod {
        name: "return".into(),
        type_params: Vec::new(),
        params: vec![val_param],
        return_type: Some(step_ann.into()),
        body,
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
    yield_ty: &str,
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
        let call = emit_self_next_call(ast, yield_ty);
        vec![Stmt::Expr(stash), Stmt::Expr(arm), Stmt::Return(Some(call))]
    } else {
        let close_stmt = emit_close_state(ast);
        let err_ident = ast.add_expr(Expr::Ident(THROW_ERR_PARAM.into()));
        vec![close_stmt, Stmt::Throw(err_ident)]
    };
    ClassMethod {
        name: "throw".into(),
        type_params: Vec::new(),
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
        type_params: Vec::new(),
        params: vec![yield_arg_param],
        return_type: Some(step_ann.into()),
        body,
        is_abstract: false,
        visibility: Visibility::Public,
        accessor_kind: None,
        span: crate::lexer::Span { start: 0, end: 0 },
    }
}
