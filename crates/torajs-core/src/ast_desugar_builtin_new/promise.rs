//! `new Promise(executor)` desugar — split out of the parent pass
//! file verbatim (rotation 283; the parent sat on the file-size
//! known-debt list at 516 and a new pass had to land there).

pub(crate) mod context;

use crate::ast::{Ast, Expr, ExprId};
use context::{EXECUTOR_HELPER, executor_contexts, helper_name, settled};
use std::collections::BTreeSet;

/// `new Promise(executor)` — §27.2.3.1 (rotation 234). The runtime
/// already owns the whole mechanism as `Promise.withResolvers()`
/// (§27.2.4.8: a pending cell plus its two settle closures), so the
/// constructor form desugars to a synthesized helper that mints the
/// trio, runs the executor synchronously against the settle pair,
/// and converts an executor throw into a rejection (§27.2.3.1 step
/// 10):
///
/// ```text
/// function __promise_from_executor(__ex: any): any {
///   if (typeof __ex !== "function") throw __new_TypeError("…");
///   let __pr: any = Promise.withResolvers();
///   try { __ex(__pr.resolve, __pr.reject); }
///   catch (__e) { __pr.reject(__e); }
///   return __pr.promise;
/// }
/// ```
///
/// Everything the helper's body says rides existing lanes (any-call
/// for the executor, any-member for the trio fields, any-receiver
/// then/catch on the result), probe-verified byte-equal before this
/// pass existed. Step 2's IsCallable check is the helper's first
/// statement, OUTSIDE the `try`: a non-callable executor is a
/// synchronous TypeError (the `try` used to turn it into a rejection
/// that nobody could catch). Every arity rewrites: a missing executor
/// is `undefined` and takes that same throw, and trailing arguments are
/// passed on so they are evaluated in order and then ignored, as a
/// direct call with extra arguments already is.
///
/// Where the context names a primitive `T` (`context.rs`), the
/// call goes to a twin `__promise_from_executor__<T>` that returns
/// `Promise<T>`; each helper is made once, and only when some call
/// names it.
pub(super) fn rewrite_promise_new(ast: &mut Ast) {
    // P4.6 stdlib-override — a user `class Promise` (or any user
    // binding of the name) owns every `new Promise(...)` in the
    // program; rewriting would hand the user's constructor arg to the
    // executor helper (gate caught: promise-001-basic declares
    // `class Promise<T>` with a SEED-value ctor).
    let user_shadows_promise = ast.stmts.iter().any(|s| {
        matches!(s, crate::ast::Stmt::ClassDecl { name, .. } if name == "Promise")
            || matches!(s, crate::ast::Stmt::FnDecl { name, .. } if name == "Promise")
            || matches!(s, crate::ast::Stmt::LetDecl { name, .. } if name == "Promise")
    });
    if user_shadows_promise {
        return;
    }
    let n_exprs = ast.exprs.len();
    let mut callees = Vec::new();
    for i in 0..n_exprs {
        let (executor, type_args) = match &ast.exprs[i] {
            Expr::New {
                class_name,
                args,
                type_args,
            } if class_name == "Promise" => (args.clone(), type_args.clone()),
            _ => continue,
        };
        let executor = if executor.is_empty() {
            vec![ast.add_expr(Expr::Ident("undefined".into()))]
        } else {
            executor
        };
        let callee = ast.add_expr(Expr::Ident(EXECUTOR_HELPER.into()));
        callees.push((ExprId(i as u32), callee));
        ast.exprs[i] = Expr::Call {
            callee,
            args: executor,
        };
        // `new Promise<T>(…)` states what the executor's `resolve`
        // takes; the helper is not generic, so only that reads it
        if !type_args.is_empty() {
            ast.call_type_args.insert(ExprId(i as u32), type_args);
        }
    }
    name_helpers(ast, callees);
}

/// Point each rewritten call's callee at its helper and write the
/// helpers out. A promise whose `T` the context names is made by a
/// helper that returns `Promise<T>`, so the expression is typed where
/// it sits.
fn name_helpers(ast: &mut Ast, callees: Vec<(ExprId, ExprId)>) {
    let contexts = executor_contexts(ast, |t| settled(t).is_some());
    let mut helpers = BTreeSet::new();
    for (call, callee) in callees {
        let t = contexts.get(&call).and_then(|t| settled(t));
        ast.exprs[callee.0 as usize] = Expr::Ident(helper_name(t));
        helpers.insert(t);
    }
    // one helper is written out whole; several share the `any` one,
    // each typed twin a call of it
    match helpers.iter().copied().collect::<Vec<_>>().as_slice() {
        [] => {}
        [only] => synthesize_promise_executor_helper(ast, *only),
        _ => {
            synthesize_promise_executor_helper(ast, None);
            for t in helpers.into_iter().flatten() {
                synthesize_settled_twin(ast, t);
            }
        }
    }
}

/// `function __promise_from_executor__<T>(__ex: any): Promise<T> {
///   return __promise_from_executor(__ex); }`
fn synthesize_settled_twin(ast: &mut Ast, settles: &str) {
    use crate::ast::{Param, Stmt};
    let callee = ast.add_expr(Expr::Ident(EXECUTOR_HELPER.into()));
    let ex = ast.add_expr(Expr::Ident("__ex".into()));
    let call = ast.add_expr(Expr::Call {
        callee,
        args: vec![ex],
    });
    ast.stmts.push(Stmt::FnDecl {
        name: helper_name(Some(settles)),
        type_params: Vec::new(),
        params: vec![Param {
            name: "__ex".into(),
            type_ann: Some("any".into()),
            default: None,
            is_rest: false,
        }],
        return_type: Some(format!("Promise<{settles}>")),
        body: vec![Stmt::Return(Some(call))],
        is_generator: false,
        span: crate::lexer::Span { start: 0, end: 0 },
    });
}

fn synthesize_promise_executor_helper(ast: &mut Ast, settles: Option<&str>) {
    use crate::ast::{Param, Stmt};

    // if (typeof __ex !== "function") throw __new_TypeError(…);
    let ex_typeof = ast.add_expr(Expr::Ident("__ex".into()));
    let type_of = ast.add_expr(Expr::TypeOf { expr: ex_typeof });
    let function_str = ast.add_expr(Expr::String("function".into()));
    let not_callable = ast.add_expr(Expr::BinOp {
        op: crate::ast::BinOp::Neq,
        left: type_of,
        right: function_str,
    });
    let type_error = ast.add_expr(Expr::Ident("__new_TypeError".into()));
    let msg = ast.add_expr(Expr::String(
        "Promise constructor takes a function argument".into(),
    ));
    let err = ast.add_expr(Expr::Call {
        callee: type_error,
        args: vec![msg],
    });
    let guard = Stmt::If {
        cond: not_callable,
        then_branch: Box::new(Stmt::Throw(err)),
        else_branch: None,
    };

    // let __pr: any = Promise.withResolvers();
    let promise_ns = ast.add_expr(Expr::Ident("Promise".into()));
    let wr_member = ast.add_expr(Expr::Member {
        obj: promise_ns,
        name: "withResolvers".into(),
    });
    let wr_call = ast.add_expr(Expr::Call {
        callee: wr_member,
        args: vec![],
    });
    let let_pr = Stmt::LetDecl {
        mutable: false,
        name: "__pr".into(),
        type_ann: Some("any".into()),
        init: wr_call,
        is_var: false,
    };

    // __ex(__pr.resolve, __pr.reject);
    let ex_ref = ast.add_expr(Expr::Ident("__ex".into()));
    let pr1 = ast.add_expr(Expr::Ident("__pr".into()));
    let res_ref = ast.add_expr(Expr::Member {
        obj: pr1,
        name: "resolve".into(),
    });
    let pr2 = ast.add_expr(Expr::Ident("__pr".into()));
    let rej_ref = ast.add_expr(Expr::Member {
        obj: pr2,
        name: "reject".into(),
    });
    let ex_call = ast.add_expr(Expr::Call {
        callee: ex_ref,
        args: vec![res_ref, rej_ref],
    });

    // catch (__e) { __pr.reject(__e); }
    let pr3 = ast.add_expr(Expr::Ident("__pr".into()));
    let rej_member = ast.add_expr(Expr::Member {
        obj: pr3,
        name: "reject".into(),
    });
    let err_ref = ast.add_expr(Expr::Ident("__e".into()));
    let rej_call = ast.add_expr(Expr::Call {
        callee: rej_member,
        args: vec![err_ref],
    });
    let try_stmt = Stmt::Try {
        body: vec![Stmt::Expr(ex_call)],
        had_catch: true,
        catch_param: Some("__e".into()),
        catch_type: None,
        catch_body: vec![Stmt::Expr(rej_call)],
        finally_body: None,
    };

    // return __pr.promise;
    let pr4 = ast.add_expr(Expr::Ident("__pr".into()));
    let promise_field = ast.add_expr(Expr::Member {
        obj: pr4,
        name: "promise".into(),
    });

    ast.stmts.push(Stmt::FnDecl {
        name: helper_name(settles),
        type_params: Vec::new(),
        params: vec![Param {
            name: "__ex".into(),
            type_ann: Some("any".into()),
            default: None,
            is_rest: false,
        }],
        return_type: Some(settles.map_or("any".into(), |t| format!("Promise<{t}>"))),
        body: vec![guard, let_pr, try_stmt, Stmt::Return(Some(promise_field))],
        is_generator: false,
        span: crate::lexer::Span { start: 0, end: 0 },
    });
}
