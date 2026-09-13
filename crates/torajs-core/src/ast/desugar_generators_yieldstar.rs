//! `yield* src` in an ASYNC generator — §27.6.3.9 / §27.5.3.2 with
//! generatorKind async, as the protocol a machine can run.
//!
//! The parser spells the delegation as a yield-bearing `for await`
//! (`emit_yieldstar_generic` / `emit_yieldstar_expr_hoist`) and names
//! its loop variable in `Ast::yieldstar_async_vars`; the generator prep
//! routes those loops here instead of the for-await protocol, whose
//! shape is a loop's and not a delegation's:
//!
//! ```text
//! let __gfam_N: any = (src as any)[Symbol.asyncIterator];   // GetMethod once
//! let __gfit_N: any = __gfam_N != null ? __gfam_N.call(src) : (src as any)[Symbol.iterator]();
//! let __ysn_N: any = __gfit_N.next;   // [[NextMethod]], read once
//! let __ysr_N: any = undefined;   // the value the outer next() sent in
//! let __ysk_N: number = 0;        // 0 next / 1 throw — how the last yield was resumed
//! while (true) {
//!   let __ysf_N: any = __ysn_N;                       // this round's method
//!   if (__ysk_N === 1) {
//!     const __yst_N: any = __gfit_N.throw;
//!     if (__yst_N == null) { <AsyncIteratorClose>; throw new TypeError(…) }
//!     __ysf_N = __yst_N;
//!   } else if (__ysk_N === 2) {
//!     const __ysrm_N: any = __gfit_N.return;
//!     if (__ysrm_N == null) { return __ysr_N; }
//!     __ysf_N = __ysrm_N;
//!   }
//!   let __gfstep_N: any = yield new __AwaitMark(__agen_delegate_call(__gfit_N, __ysf_N, __ysr_N, __ysk_N, __gfam_N));
//!   if (!(__gfstep_N is an object)) throw new TypeError(…);
//!   if (__gfstep_N.done) { if (__ysk_N === 2) return __gfstep_N.value; <done target | a discarded local> = __gfstep_N.value; break; }
//!   __ysk_N = 0;
//!   const __ysv_N = __gfstep_N.value;   // IteratorValue: a throwing getter is the body's throw
//!   try { yield __ysv_N; __ysr_N = this.__sent; if (this.__ysret) { this.__ysret = false; __ysk_N = 2; } }
//!   catch (__yse_N) { __ysk_N = 1; __ysr_N = __yse_N; }
//! }
//! (the loop sits in `this.__ysact = true; try { … } finally { this.__ysact = false; }`)
//!
//! (`let v = yield e` — `Stmt::YieldInto` — DECLARES v, so the step's
//! yield sits at the body's own level and the resumption value is read
//! off the machine's `__sent` slot after a plain yield.)
//! ```
//!
//! What the spec's ordering tests read off the object: the
//! `@@asyncIterator` getter runs once; `next` is read once (the
//! iterator record's [[NextMethod]]) and called with ONE argument —
//! `undefined` first, then whatever the outer `next(v)` sent (the
//! machine hands it back through the delegating yield); the step is
//! awaited (`then` read and called with two functions) BEFORE `done`
//! and `value` are read, in that order; a step that is not an object
//! is a TypeError after the await. A `throw(e)` resumption (the
//! machine's throw at the yield, caught here) forwards to the inner
//! iterator's `throw` — read when needed — and a missing one closes
//! the iterator (`return` read and, if present, called and awaited)
//! before the TypeError. The async posture is decided by the
//! `@@asyncIterator` probe as before (a class instance's folded member
//! is invisible to a symbol read, so such an iterator keeps the
//! `it.next(v)` spelling and the step-result posture — `__agen_delegate_call`).
//!
//! A `return(v)` resumption (§27.5.3.2 step 7.c): the machine's
//! `return()` sees `this.__ysact` (raised around the loop, cleared by
//! its finally) and resumes the delegating yield as a next(v) with
//! `this.__ysret` raised; the loop reads the flag after the yield and
//! takes the return arm — the inner `return`, read when needed, is
//! called with v (already awaited by the driver, AwaitReturn) and its
//! done step's value completes the outer generator; a missing `return`
//! completes it with v.

use super::desugar_generators_forof::symbol_index_expr;
use super::desugar_generators_yieldstar_ast::{
    assign_stmt, call_method, ident, is_null, let_any, member, not_null, not_object, this_field,
    type_error,
};
use super::{Ast, BinOp, Expr, ExprId, Stmt};

/// The delegation protocol for the for-await loop `var_name` over
/// `src_ident` whose body is the parser's `yield <var>`.
pub(super) fn build_delegation_protocol(
    ast: &mut Ast,
    var_name: String,
    src_ident: String,
    n: usize,
) -> Stmt {
    let names = Names {
        n,
        var_name,
        am: format!("__gfam_{n}"),
        it: format!("__gfit_{n}"),
        next_m: format!("__ysn_{n}"),
        recv: format!("__ysr_{n}"),
        kind: format!("__ysk_{n}"),
        step: format!("__gfstep_{n}"),
        throw_m: format!("__yst_{n}"),
        err: format!("__yse_{n}"),
        callee: format!("__ysf_{n}"),
    };
    let Names {
        am,
        it,
        next_m,
        recv,
        kind,
        ..
    } = &names;

    let mut out: Vec<Stmt> = Vec::new();
    // let __gfam = src[@@asyncIterator]
    let am_probe = symbol_index_expr(ast, &src_ident, "asyncIterator");
    out.push(let_any(am, am_probe));
    // let __gfit = __gfam != null ? __gfam.call(src) : src[@@iterator]()
    let has_async = not_null(ast, am);
    let am_ref = ident(ast, am);
    let src_ref = ident(ast, &src_ident);
    let am_call = call_method(ast, am_ref, "call", vec![src_ref]);
    let sync_get = symbol_index_expr(ast, &src_ident, "iterator");
    let sync_call = ast.add_expr(Expr::Call {
        callee: sync_get,
        args: Vec::new(),
    });
    let it_init = ast.add_expr(Expr::Ternary {
        cond: has_async,
        then_branch: am_call,
        else_branch: sync_call,
    });
    out.push(let_any(it, it_init));
    // let __ysn = __gfit.next   ([[NextMethod]], read once)
    let next_read = member(ast, it, "next");
    out.push(let_any(next_m, next_read));
    let undef1 = ident(ast, "undefined");
    out.push(Stmt::LetDecl {
        mutable: true,
        name: recv.clone(),
        type_ann: Some("any".into()),
        init: undef1,
        is_var: false,
    });
    let zero = ast.add_expr(Expr::Number(0.0));
    out.push(Stmt::LetDecl {
        mutable: true,
        name: kind.clone(),
        type_ann: Some("number".into()),
        init: zero,
        is_var: false,
    });

    let body = delegation_loop_body(ast, &names);

    // this.__ysact = true; try { while (true) {…} } finally { this.__ysact = false; }
    let act_on = this_field(ast, super::desugar_generators_async::YS_ACTIVE_FIELD);
    let t = ast.add_expr(Expr::Bool(true));
    let set_on = ast.add_expr(Expr::Assign {
        target: act_on,
        value: t,
    });
    out.push(Stmt::Expr(set_on));
    let true_lit = ast.add_expr(Expr::Bool(true));
    let loop_stmt = Stmt::While {
        cond: true_lit,
        body: Box::new(Stmt::Block(body)),
    };
    let act_off = this_field(ast, super::desugar_generators_async::YS_ACTIVE_FIELD);
    let f2 = ast.add_expr(Expr::Bool(false));
    let set_off = ast.add_expr(Expr::Assign {
        target: act_off,
        value: f2,
    });
    out.push(Stmt::Try {
        body: vec![loop_stmt],
        had_catch: false,
        catch_param: None,
        catch_type: None,
        catch_body: Vec::new(),
        finally_body: Some(vec![Stmt::Expr(set_off)]),
    });
    Stmt::Multi(out)
}

/// The names one delegation's locals are spelled with (`_N` suffixed).
struct Names {
    n: usize,
    var_name: String,
    am: String,
    it: String,
    next_m: String,
    recv: String,
    kind: String,
    step: String,
    throw_m: String,
    err: String,
    callee: String,
}

/// One round of the delegation loop (module doc): pick this round's
/// method by how the last yield was resumed, call it through the
/// helper and await the step, check it, finish on `done`, yield its
/// value and record how the outer resumed.
fn delegation_loop_body(ast: &mut Ast, names: &Names) -> Vec<Stmt> {
    let Names {
        n,
        var_name,
        am,
        it,
        next_m,
        recv,
        kind,
        step,
        throw_m,
        err,
        callee,
    } = names;
    let n = *n;
    let mut body: Vec<Stmt> = Vec::new();
    // let __ysf = __ysn;  if (__ysk === 1) { const __yst = __gfit.throw; if (__yst == null) {…} __ysf = __yst; }
    let next_ref = ident(ast, next_m);
    body.push(Stmt::LetDecl {
        mutable: true,
        name: callee.clone(),
        type_ann: Some("any".into()),
        init: next_ref,
        is_var: false,
    });
    let mut throw_arm: Vec<Stmt> = Vec::new();
    let throw_read = member(ast, it, "throw");
    throw_arm.push(let_any(throw_m, throw_read));
    let no_throw = is_null(ast, throw_m);
    throw_arm.push(Stmt::If {
        cond: no_throw,
        then_branch: Box::new(Stmt::Block(close_then_type_error(ast, it, n))),
        else_branch: None,
    });
    let throw_ref = ident(ast, throw_m);
    throw_arm.push(assign_stmt(ast, callee, throw_ref));
    let kind_ref = ident(ast, kind);
    let one = ast.add_expr(Expr::Number(1.0));
    let is_throw = ast.add_expr(Expr::BinOp {
        op: BinOp::Eq,
        left: kind_ref,
        right: one,
    });
    // return arm (§27.5.3.2 7.c): `const __ysrt = it.return; if
    // (__ysrt == null) return __ysr; __ysf = __ysrt;` — the request's
    // value was awaited by the driver (AwaitReturn) before it stepped.
    let mut return_arm: Vec<Stmt> = Vec::new();
    let return_m = format!("__ysrm_{n}");
    let return_read = member(ast, it, "return");
    return_arm.push(let_any(&return_m, return_read));
    let no_return = is_null(ast, &return_m);
    let recv_ref0 = ident(ast, recv);
    return_arm.push(Stmt::If {
        cond: no_return,
        then_branch: Box::new(Stmt::Return(Some(recv_ref0))),
        else_branch: None,
    });
    let return_ref = ident(ast, &return_m);
    return_arm.push(assign_stmt(ast, callee, return_ref));
    let kind_ref2 = ident(ast, kind);
    let two = ast.add_expr(Expr::Number(2.0));
    let is_return = ast.add_expr(Expr::BinOp {
        op: BinOp::Eq,
        left: kind_ref2,
        right: two,
    });
    body.push(Stmt::If {
        cond: is_throw,
        then_branch: Box::new(Stmt::Block(throw_arm)),
        else_branch: Some(Box::new(Stmt::If {
            cond: is_return,
            then_branch: Box::new(Stmt::Block(return_arm)),
            else_branch: None,
        })),
    });
    // let __gfstep = yield new __AwaitMark(__agen_delegate_call(it, fn, recv, kind, am));
    let mark = delegate_call(ast, it, callee, recv, kind, am);
    body.push(Stmt::YieldInto {
        var: step.clone(),
        type_ann: Some("any".into()),
        value: mark,
    });
    // if (!(step is object)) throw new TypeError(...)
    let not_obj = not_object(ast, step);
    let te = type_error(ast, "delegated iterator step is not an object");
    body.push(Stmt::If {
        cond: not_obj,
        then_branch: Box::new(Stmt::Throw(te)),
        else_branch: None,
    });
    // if (step.done) { if (__ysk === 2) return step.value; [target = step.value;] break; }
    let done_read = member(ast, step, "done");
    let kind_ref3 = ident(ast, kind);
    let two2 = ast.add_expr(Expr::Number(2.0));
    let done_by_return = ast.add_expr(Expr::BinOp {
        op: BinOp::Eq,
        left: kind_ref3,
        right: two2,
    });
    let ret_value = member(ast, step, "value");
    let mut done_stmts = vec![Stmt::If {
        cond: done_by_return,
        then_branch: Box::new(Stmt::Return(Some(ret_value))),
        else_branch: None,
    }];
    // §27.5.3.2 7.a.vii — `IteratorValue(innerResult)` on a done step
    // runs the `value` getter whether or not anything receives it
    // (`yield-star-next-call-value-get-abrupt`: the getter throws and
    // that throw completes the generator); a statement-position
    // delegation reads it into a discarded local.
    let final_value = member(ast, step, "value");
    match ast.yieldstar_done_targets.get(var_name).cloned() {
        Some(target) => {
            let target_ref = ident(ast, &target);
            let assign = ast.add_expr(Expr::Assign {
                target: target_ref,
                value: final_value,
            });
            done_stmts.push(Stmt::Expr(assign));
        }
        None => done_stmts.push(let_any(&format!("__ysdv_{n}"), final_value)),
    }
    done_stmts.push(Stmt::Break(None));
    body.push(Stmt::If {
        cond: done_read,
        then_branch: Box::new(Stmt::Block(done_stmts)),
        else_branch: None,
    });
    let zero2 = ast.add_expr(Expr::Number(0.0));
    body.push(assign_stmt(ast, kind, zero2));
    // const __ysv = step.value;   (§27.6.3.9 IteratorValue — a throwing
    // `value` getter is the body's own throw, OUTSIDE the try below,
    // which catches only a throw resumption at the yield)
    // try { yield __ysv; __ysr = this.__sent; if (this.__ysret) { this.__ysret = false; __ysk = 2; } }
    // catch (__yse) { __ysk = 1; __ysr = __yse; }
    let value_tmp = format!("__ysv_{n}");
    let value_read0 = member(ast, step, "value");
    body.push(let_any(&value_tmp, value_read0));
    let value_read = ident(ast, &value_tmp);
    let this_id = ast.add_expr(Expr::This);
    let sent = ast.add_expr(Expr::Member {
        obj: this_id,
        name: "__sent".into(),
    });
    let take_sent = assign_stmt(ast, recv, sent);
    let ret_flag = this_field(ast, super::desugar_generators_async::YS_RETURN_FIELD);
    let ret_flag2 = this_field(ast, super::desugar_generators_async::YS_RETURN_FIELD);
    let f = ast.add_expr(Expr::Bool(false));
    let clear_flag = ast.add_expr(Expr::Assign {
        target: ret_flag2,
        value: f,
    });
    let two3 = ast.add_expr(Expr::Number(2.0));
    let set_kind2 = assign_stmt(ast, kind, two3);
    let take_return = Stmt::If {
        cond: ret_flag,
        then_branch: Box::new(Stmt::Block(vec![Stmt::Expr(clear_flag), set_kind2])),
        else_branch: None,
    };
    let one2 = ast.add_expr(Expr::Number(1.0));
    let set_kind = assign_stmt(ast, kind, one2);
    let err_ref = ident(ast, err);
    let set_recv = assign_stmt(ast, recv, err_ref);
    body.push(Stmt::Try {
        body: vec![Stmt::Yield(value_read), take_sent, take_return],
        had_catch: true,
        catch_param: Some(err.clone()),
        catch_type: Some("any".into()),
        catch_body: vec![set_kind, set_recv],
        finally_body: None,
    });

    body
}

/// `[const __rt = it.return; if (__rt != null) { yield new
/// __AwaitMark(__rt.call(it)); } throw new TypeError(…)]` —
/// AsyncIteratorClose with a normal completion, then the TypeError
/// §27.5.3.2 step 7.b.vi throws for a missing `throw`.
fn close_then_type_error(ast: &mut Ast, it: &str, n: usize) -> Vec<Stmt> {
    let ret_m = format!("__ysrt_{n}");
    let ret_read = member(ast, it, "return");
    let mut v = vec![let_any(&ret_m, ret_read)];
    let has_ret = not_null(ast, &ret_m);
    let ret_ref = ident(ast, &ret_m);
    let it_ref = ident(ast, it);
    let ret_call = call_method(ast, ret_ref, "call", vec![it_ref]);
    let mark = super::desugar_generators_async::mint_await_mark(ast, ret_call);
    let awaited = format!("__ysrv_{n}");
    v.push(Stmt::If {
        cond: has_ret,
        then_branch: Box::new(Stmt::Block(vec![Stmt::YieldInto {
            var: awaited,
            type_ann: Some("any".into()),
            value: mark,
        }])),
        else_branch: None,
    });
    let te = type_error(ast, "the delegated iterator does not have a throw method");
    v.push(Stmt::Throw(te));
    v
}

/// `yield new __AwaitMark(__agen_delegate_call(it, fn, recv, kind, am))`
/// as an expression assigned to the step (the yield rides a
/// `YieldInto` on the step's own name).
fn delegate_call(ast: &mut Ast, it: &str, f: &str, recv: &str, kind: &str, am: &str) -> ExprId {
    let helper = ident(ast, "__agen_delegate_call");
    let args = vec![
        ident(ast, it),
        ident(ast, f),
        ident(ast, recv),
        ident(ast, kind),
        ident(ast, am),
    ];
    let call = ast.add_expr(Expr::Call {
        callee: helper,
        args,
    });
    super::desugar_generators_async::mint_await_mark(ast, call)
}
