//! The loop and `switch` arms of the await rewrite: a head that is
//! evaluated again on every iteration (or a case value evaluated only
//! while no earlier case matched) cannot take its awaits ahead of the
//! statement, so it moves into the body as statements of its own.
//!
//! A test that must run before every iteration but the first (a
//! `do … while` test, a `for` step) sits behind a flag the loop clears
//! on its first pass: `continue` returns to the head, which runs it.

use super::super::desugar_async_sm::{expr_has_await, stmt_has_await};
use super::super::{Ast, BinOp, Expr, ExprId, Stmt, UnaryOp};
use super::{Cx, lift_root, rewrite_list, rewrite_stmt, with_prefix};
use crate::ast::desugar_async_sm_lift::LiftLog;

/// `if (!cond) break;`, its awaits rewritten.
fn exit_unless(ast: &mut Ast, cond: ExprId, cx: &Cx, log: &mut LiftLog) -> Option<Stmt> {
    let not = ast.add_expr(Expr::Unary {
        op: UnaryOp::Not,
        expr: cond,
    });
    let mut test = Stmt::If {
        cond: not,
        then_branch: Box::new(Stmt::Break(None)),
        else_branch: None,
    };
    rewrite_stmt(ast, &mut test, cx, log).then_some(test)
}

/// A fresh flag, `true` until the loop's first pass clears it: the
/// declaration ahead of the loop and the head `if (flag) flag = false;
/// else <later>`.
fn first_pass(ast: &mut Ast, tag: &str, later: Stmt) -> (Stmt, Stmt) {
    let on = ast.add_expr(Expr::Bool(true));
    let flag = format!("__aw{tag}_{}", on.0);
    let decl = Stmt::LetDecl {
        mutable: true,
        name: flag.clone(),
        type_ann: Some("boolean".into()),
        init: on,
        is_var: false,
    };
    let read = ast.add_expr(Expr::Ident(flag.clone()));
    let target = ast.add_expr(Expr::Ident(flag));
    let off = ast.add_expr(Expr::Bool(false));
    let clear = ast.add_expr(Expr::Assign { target, value: off });
    let head = Stmt::If {
        cond: read,
        then_branch: Box::new(Stmt::Expr(clear)),
        else_branch: Some(Box::new(later)),
    };
    (decl, head)
}

/// `for (init; c; step) body`. An await in `c` moves the test into the
/// body's head. An await in `step` moves the step there too, behind the
/// first-pass flag, and the test after it: it runs at the top of the
/// next iteration, in that iteration's copy of the loop's bindings
/// (§14.7.4.4: CreatePerIterationEnvironment comes before the
/// increment). An await in `init`, which runs once, lifts ahead of the
/// loop: a `let` / `const` init keeps its binding in the loop's scope
/// and reads the lifted value; an expression or `var` init moves out
/// whole.
pub(super) fn rewrite_for(ast: &mut Ast, s: &mut Stmt, cx: &Cx, log: &mut LiftLog) -> bool {
    let Stmt::For {
        init,
        cond,
        step,
        body,
    } = s
    else {
        unreachable!("rewrite_for on a for")
    };
    if !rewrite_stmt(ast, body, cx, log) {
        return false;
    }
    let mut prefix = Vec::new();
    if init.as_deref().is_some_and(|i| stmt_has_await(ast, i)) {
        let lexical = match init.as_deref() {
            Some(Stmt::LetDecl {
                init: e,
                is_var: false,
                ..
            }) => Some(*e),
            // two declarators: the second's await would run before the
            // first's initializer
            Some(Stmt::Multi(_)) => return false,
            _ => None,
        };
        if let Some(e) = lexical {
            let Some(p) = lift_root(ast, e, cx, log) else {
                return false;
            };
            prefix = p;
        } else {
            let Some(mut once) = init.take().map(|b| *b) else {
                unreachable!("the init awaits")
            };
            if !rewrite_stmt(ast, &mut once, cx, log) {
                return false;
            }
            prefix.push(once);
        }
    }
    let mut head = Vec::new();
    if let Some(st) = (*step).filter(|e| expr_has_await(ast, *e)) {
        *step = None;
        let mut later = Stmt::Expr(st);
        if !rewrite_stmt(ast, &mut later, cx, log) {
            return false;
        }
        let (decl, pass) = first_pass(ast, "s", later);
        prefix.push(decl);
        head.push(pass);
        if let Some(c) = cond.take() {
            let Some(test) = exit_unless(ast, c, cx, log) else {
                return false;
            };
            head.push(test);
        }
    } else if let Some(c) = (*cond).filter(|c| expr_has_await(ast, *c)) {
        *cond = None;
        let Some(test) = exit_unless(ast, c, cx, log) else {
            return false;
        };
        head.push(test);
    }
    if !head.is_empty() {
        head.push(std::mem::replace(body.as_mut(), Stmt::Multi(Vec::new())));
        **body = Stmt::Block(head);
    }
    with_prefix(s, prefix);
    true
}

/// A loop head re-evaluates per iteration, so its await cannot hoist
/// ahead of the loop: `while (c)` becomes `while (true) { <lift of c>;
/// if (!c') break; body }` — `continue` in the body returns to the head
/// and re-runs the lift.
pub(super) fn rewrite_while(ast: &mut Ast, s: &mut Stmt, cx: &Cx, log: &mut LiftLog) -> bool {
    let Stmt::While { cond, body } = s else {
        unreachable!("rewrite_while on a while")
    };
    if !rewrite_stmt(ast, body, cx, log) {
        return false;
    }
    let cond = *cond;
    if !expr_has_await(ast, cond) {
        return true;
    }
    let Some(test) = exit_unless(ast, cond, cx, log) else {
        return false;
    };
    let body = std::mem::replace(body.as_mut(), Stmt::Multi(Vec::new()));
    let always = ast.add_expr(Expr::Bool(true));
    *s = Stmt::While {
        cond: always,
        body: Box::new(Stmt::Block(vec![test, body])),
    };
    true
}

/// `do body while (c)` with an await in `c` becomes `while (true) { if
/// (first) first = false; else { <lift of c>; if (!c') break; } body }`:
/// `continue` in the body returns to the head, which runs the test.
pub(super) fn rewrite_do_while(ast: &mut Ast, s: &mut Stmt, cx: &Cx, log: &mut LiftLog) -> bool {
    let Stmt::DoWhile { cond, body } = s else {
        unreachable!("rewrite_do_while on a do-while")
    };
    if !rewrite_stmt(ast, body, cx, log) {
        return false;
    }
    let cond = *cond;
    if !expr_has_await(ast, cond) {
        return true;
    }
    let Some(test) = exit_unless(ast, cond, cx, log) else {
        return false;
    };
    let body = std::mem::replace(body.as_mut(), Stmt::Multi(Vec::new()));
    let (decl, pass) = first_pass(ast, "d", test);
    let always = ast.add_expr(Expr::Bool(true));
    let lp = Stmt::While {
        cond: always,
        body: Box::new(Stmt::Block(vec![pass, body])),
    };
    *s = Stmt::Multi(vec![decl, lp]);
    true
}

/// `switch (d)`: the discriminant is evaluated once, its awaits lifted
/// ahead. A case value with an await is evaluated only once every
/// earlier one failed to match (§14.12.4 CaseBlockEvaluation, the
/// clauses before `default` and then the ones after it, which is the
/// order of `cases`): the tests become an `if` chain over the bound
/// discriminant that picks the clause by its index, and the switch
/// dispatches on that index, its bodies, `default` and fall-through
/// as they were.
pub(super) fn rewrite_switch(ast: &mut Ast, s: &mut Stmt, cx: &Cx, log: &mut LiftLog) -> bool {
    let Stmt::Switch {
        scrutinee,
        cases,
        default,
        ..
    } = s
    else {
        unreachable!("rewrite_switch on a switch")
    };
    for c in cases.iter_mut() {
        if !rewrite_list(ast, &mut c.body, cx, log) {
            return false;
        }
    }
    if let Some(d) = default.as_mut()
        && !rewrite_list(ast, d, cx, log)
    {
        return false;
    }
    if !cases.iter().any(|c| expr_has_await(ast, c.value)) {
        let Some(prefix) = lift_root(ast, *scrutinee, cx, log) else {
            return false;
        };
        with_prefix(s, prefix);
        return true;
    }
    let none = ast.add_expr(Expr::Number(-1.0));
    let (d, k) = (format!("__awsd_{}", none.0), format!("__awsk_{}", none.0));
    let mut prefix = vec![
        Stmt::LetDecl {
            mutable: false,
            name: d.clone(),
            type_ann: Some("any".into()),
            init: *scrutinee,
            is_var: false,
        },
        Stmt::LetDecl {
            mutable: true,
            name: k.clone(),
            type_ann: Some("number".into()),
            init: none,
            is_var: false,
        },
    ];
    let mut chain: Option<Box<Stmt>> = None;
    for (i, c) in cases.iter_mut().enumerate().rev() {
        let left = ast.add_expr(Expr::Ident(d.clone()));
        let cond = ast.add_expr(Expr::BinOp {
            op: BinOp::Eq,
            left,
            right: c.value,
        });
        let target = ast.add_expr(Expr::Ident(k.clone()));
        let value = ast.add_expr(Expr::Number(i as f64));
        let pick = ast.add_expr(Expr::Assign { target, value });
        chain = Some(Box::new(Stmt::If {
            cond,
            then_branch: Box::new(Stmt::Expr(pick)),
            else_branch: chain,
        }));
        c.value = ast.add_expr(Expr::Number(i as f64));
    }
    *scrutinee = ast.add_expr(Expr::Ident(k));
    prefix.extend(chain.map(|b| *b));
    if !rewrite_list(ast, &mut prefix, cx, log) {
        return false;
    }
    with_prefix(s, prefix);
    true
}

/// A labeled statement the rewrite turned into statements ahead of it
/// keeps the label on the statement itself: `continue L` names a loop.
pub(super) fn rewrite_labeled(ast: &mut Ast, s: &mut Stmt, cx: &Cx, log: &mut LiftLog) -> bool {
    let Stmt::Labeled { label, body } = s else {
        unreachable!("rewrite_labeled on a labeled statement")
    };
    let was_multi = matches!(body.as_ref(), Stmt::Multi(_));
    if !rewrite_stmt(ast, body, cx, log) {
        return false;
    }
    let Stmt::Multi(v) = body.as_mut() else {
        return true;
    };
    if was_multi {
        return true;
    }
    let Some(last) = v.pop() else {
        return true;
    };
    let mut v = std::mem::take(v);
    v.push(Stmt::Labeled {
        label: label.clone(),
        body: Box::new(last),
    });
    *s = Stmt::Multi(v);
    true
}
