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
//!   }
//!   let __gfstep_N: any = yield new __AwaitMark(__agen_delegate_call(__gfit_N, __ysf_N, __ysr_N, __ysk_N, __gfam_N));
//!   __ysk_N = 0;
//!   if (!(__gfstep_N is an object)) throw new TypeError(…);
//!   if (__gfstep_N.done) { [<done target> = __gfstep_N.value;] break; }
//!   try { yield __gfstep_N.value; __ysr_N = this.__sent; } catch (__yse_N) { __ysk_N = 1; __ysr_N = __yse_N; }
//! }
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
//! Registered residue: a `return(v)` resumption still completes the
//! outer generator without forwarding to the inner iterator's `return`
//! (§27.5.3.2 step 7.c) — the machine has no "resume with a return
//! completion" yet.

use super::desugar_generators_forof::symbol_index_expr;
use super::{Ast, BinOp, Expr, ExprId, Stmt};

/// The delegation protocol for the for-await loop `var_name` over
/// `src_ident` whose body is the parser's `yield <var>`.
pub(super) fn build_delegation_protocol(
    ast: &mut Ast,
    var_name: String,
    src_ident: String,
    n: usize,
) -> Stmt {
    let am = format!("__gfam_{n}");
    let it = format!("__gfit_{n}");
    let next_m = format!("__ysn_{n}");
    let recv = format!("__ysr_{n}");
    let kind = format!("__ysk_{n}");
    let step = format!("__gfstep_{n}");
    let throw_m = format!("__yst_{n}");
    let err = format!("__yse_{n}");
    let callee = format!("__ysf_{n}");

    let mut out: Vec<Stmt> = Vec::new();
    // let __gfam = src[@@asyncIterator]
    let am_probe = symbol_index_expr(ast, &src_ident, "asyncIterator");
    out.push(let_any(&am, am_probe));
    // let __gfit = __gfam != null ? __gfam.call(src) : src[@@iterator]()
    let has_async = not_null(ast, &am);
    let am_ref = ident(ast, &am);
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
    out.push(let_any(&it, it_init));
    // let __ysn = __gfit.next   ([[NextMethod]], read once)
    let next_read = member(ast, &it, "next");
    out.push(let_any(&next_m, next_read));
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

    // The loop body.
    let mut body: Vec<Stmt> = Vec::new();
    // let __ysf = __ysn;  if (__ysk === 1) { const __yst = __gfit.throw; if (__yst == null) {…} __ysf = __yst; }
    let next_ref = ident(ast, &next_m);
    body.push(Stmt::LetDecl {
        mutable: true,
        name: callee.clone(),
        type_ann: Some("any".into()),
        init: next_ref,
        is_var: false,
    });
    let mut throw_arm: Vec<Stmt> = Vec::new();
    let throw_read = member(ast, &it, "throw");
    throw_arm.push(let_any(&throw_m, throw_read));
    let no_throw = is_null(ast, &throw_m);
    throw_arm.push(Stmt::If {
        cond: no_throw,
        then_branch: Box::new(Stmt::Block(close_then_type_error(ast, &it, n))),
        else_branch: None,
    });
    let throw_ref = ident(ast, &throw_m);
    throw_arm.push(assign_stmt(ast, &callee, throw_ref));
    let kind_ref = ident(ast, &kind);
    let one = ast.add_expr(Expr::Number(1.0));
    let is_throw = ast.add_expr(Expr::BinOp {
        op: BinOp::Eq,
        left: kind_ref,
        right: one,
    });
    body.push(Stmt::If {
        cond: is_throw,
        then_branch: Box::new(Stmt::Block(throw_arm)),
        else_branch: None,
    });
    // let __gfstep = yield new __AwaitMark(__agen_delegate_call(it, fn, recv, kind, am));
    let mark = delegate_call(ast, &it, &callee, &recv, &kind, &am);
    body.push(Stmt::YieldInto {
        var: step.clone(),
        type_ann: Some("any".into()),
        value: mark,
    });
    let zero2 = ast.add_expr(Expr::Number(0.0));
    body.push(assign_stmt(ast, &kind, zero2));
    // if (!(step is object)) throw new TypeError(...)
    let not_obj = not_object(ast, &step);
    let te = type_error(ast, "delegated iterator step is not an object");
    body.push(Stmt::If {
        cond: not_obj,
        then_branch: Box::new(Stmt::Throw(te)),
        else_branch: None,
    });
    // if (step.done) { [target = step.value;] break; }
    let done_read = member(ast, &step, "done");
    let done_then: Stmt = match ast.yieldstar_done_targets.get(&var_name).cloned() {
        Some(target) => {
            let final_value = member(ast, &step, "value");
            let target_ref = ident(ast, &target);
            let assign = ast.add_expr(Expr::Assign {
                target: target_ref,
                value: final_value,
            });
            Stmt::Block(vec![Stmt::Expr(assign), Stmt::Break(None)])
        }
        None => Stmt::Break(None),
    };
    body.push(Stmt::If {
        cond: done_read,
        then_branch: Box::new(done_then),
        else_branch: None,
    });
    // try { yield step.value; __ysr = this.__sent; } catch (__yse) { __ysk = 1; __ysr = __yse; }
    let value_read = member(ast, &step, "value");
    let this_id = ast.add_expr(Expr::This);
    let sent = ast.add_expr(Expr::Member {
        obj: this_id,
        name: "__sent".into(),
    });
    let take_sent = assign_stmt(ast, &recv, sent);
    let one2 = ast.add_expr(Expr::Number(1.0));
    let set_kind = assign_stmt(ast, &kind, one2);
    let err_ref = ident(ast, &err);
    let set_recv = assign_stmt(ast, &recv, err_ref);
    body.push(Stmt::Try {
        body: vec![Stmt::Yield(value_read), take_sent],
        had_catch: true,
        catch_param: Some(err),
        catch_type: Some("any".into()),
        catch_body: vec![set_kind, set_recv],
        finally_body: None,
    });

    let true_lit = ast.add_expr(Expr::Bool(true));
    out.push(Stmt::While {
        cond: true_lit,
        body: Box::new(Stmt::Block(body)),
    });
    Stmt::Multi(out)
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

/// `name = value;`
fn assign_stmt(ast: &mut Ast, name: &str, value: ExprId) -> Stmt {
    let target = ident(ast, name);
    let assign = ast.add_expr(Expr::Assign { target, value });
    Stmt::Expr(assign)
}

fn ident(ast: &mut Ast, name: &str) -> ExprId {
    ast.add_expr(Expr::Ident(name.to_string()))
}

fn member(ast: &mut Ast, obj: &str, name: &str) -> ExprId {
    let o = ident(ast, obj);
    ast.add_expr(Expr::Member {
        obj: o,
        name: name.to_string(),
    })
}

fn call_method(ast: &mut Ast, obj: ExprId, name: &str, args: Vec<ExprId>) -> ExprId {
    let callee = ast.add_expr(Expr::Member {
        obj,
        name: name.to_string(),
    });
    ast.add_expr(Expr::Call { callee, args })
}

fn let_any(name: &str, init: ExprId) -> Stmt {
    Stmt::LetDecl {
        mutable: false,
        name: name.to_string(),
        type_ann: Some("any".into()),
        init,
        is_var: false,
    }
}

fn not_null(ast: &mut Ast, name: &str) -> ExprId {
    let l = ident(ast, name);
    let r = ast.add_expr(Expr::Null);
    ast.add_expr(Expr::BinOp {
        op: BinOp::LooseNeq,
        left: l,
        right: r,
    })
}

fn is_null(ast: &mut Ast, name: &str) -> ExprId {
    let l = ident(ast, name);
    let r = ast.add_expr(Expr::Null);
    ast.add_expr(Expr::BinOp {
        op: BinOp::LooseEq,
        left: l,
        right: r,
    })
}

/// `x == null || (typeof x !== "object" && typeof x !== "function")`.
fn not_object(ast: &mut Ast, name: &str) -> ExprId {
    let nullish = is_null(ast, name);
    let x1 = ident(ast, name);
    let t1 = ast.add_expr(Expr::TypeOf { expr: x1 });
    let obj_s = string(ast, "object");
    let not_obj = ast.add_expr(Expr::BinOp {
        op: BinOp::Neq,
        left: t1,
        right: obj_s,
    });
    let x2 = ident(ast, name);
    let t2 = ast.add_expr(Expr::TypeOf { expr: x2 });
    let fn_s = string(ast, "function");
    let not_fn = ast.add_expr(Expr::BinOp {
        op: BinOp::Neq,
        left: t2,
        right: fn_s,
    });
    let both = ast.add_expr(Expr::BinOp {
        op: BinOp::LAnd,
        left: not_obj,
        right: not_fn,
    });
    ast.add_expr(Expr::BinOp {
        op: BinOp::LOr,
        left: nullish,
        right: both,
    })
}

fn string(ast: &mut Ast, s: &str) -> ExprId {
    ast.add_expr(Expr::String(torajs_wtf8::Wtf8Buf::from(s)))
}

fn type_error(ast: &mut Ast, msg: &str) -> ExprId {
    let m = string(ast, msg);
    ast.add_expr(Expr::New {
        class_name: "TypeError".into(),
        args: vec![m],
        type_args: Vec::new(),
    })
}
