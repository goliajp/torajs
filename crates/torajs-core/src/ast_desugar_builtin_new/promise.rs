//! `new Promise(executor)` desugar — split out of the parent pass
//! file verbatim (rotation 283; the parent sat on the file-size
//! known-debt list at 516 and a new pass had to land there).

use crate::ast::{Ast, Expr};

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
/// pass existed. Only the exact 1-arg form rewrites: a 0-arg
/// `new Promise()` is a TypeError per spec (executor not callable)
/// and keeps today's loud reject, and a >1-arg call would have to
/// evaluate trailing args for their side effects to be faithful —
/// left loud rather than silently dropped.
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
    let mut synthesized = false;
    for i in 0..n_exprs {
        let executor = match &ast.exprs[i] {
            Expr::New {
                class_name, args, ..
            } if class_name == "Promise" && args.len() == 1 => args[0],
            _ => continue,
        };
        if !synthesized {
            synthesize_promise_executor_helper(ast);
            synthesized = true;
        }
        let callee = ast.add_expr(Expr::Ident("__promise_from_executor".into()));
        ast.exprs[i] = Expr::Call {
            callee,
            args: vec![executor],
        };
    }
}

fn synthesize_promise_executor_helper(ast: &mut Ast) {
    use crate::ast::{Param, Stmt};

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
        name: "__promise_from_executor".into(),
        type_params: Vec::new(),
        params: vec![Param {
            name: "__ex".into(),
            type_ann: Some("any".into()),
            default: None,
            is_rest: false,
        }],
        return_type: Some("any".into()),
        body: vec![let_pr, try_stmt, Stmt::Return(Some(promise_field))],
        is_generator: false,
        span: crate::lexer::Span { start: 0, end: 0 },
    });
}
