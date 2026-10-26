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
use crate::ast::nested_fns_idents::{rewrite_idents_in_expr, rewrite_idents_in_stmt};
use crate::ast::tdz_reads;
use std::collections::{HashMap, HashSet};

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
/// whole, and so does a lexical init of several declarations (a
/// destructuring one is several too), under fresh names the loop's
/// bindings copy ([`declarations_ahead`]).
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
    let several = matches!(init.as_deref(), Some(Stmt::Multi(ds))
        if ds.iter().any(|d| matches!(d, Stmt::LetDecl { is_var: false, .. })));
    if several && init.as_deref().is_some_and(|i| stmt_has_await(ast, i)) {
        let Some(ahead) = declarations_ahead(ast, init, cx, log) else {
            return false;
        };
        prefix = ahead;
    } else if init.as_deref().is_some_and(|i| stmt_has_await(ast, i)) {
        let lexical = match init.as_deref() {
            Some(Stmt::LetDecl {
                init: e,
                is_var: false,
                ..
            }) => Some(*e),
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

/// `for (let a = i1, b = await i2; …)` becomes `let a' = i1; let b' =
/// await i2'; for (let a = a', b = b'; …)`: the declarations run once
/// ahead of the loop, in order, with their awaits rewritten, under
/// fresh names (`i2'` reads `a'` wherever `i2` read `a`), and the
/// loop's own bindings (the ones CreatePerIterationEnvironment copies,
/// §14.7.4.4) start from their values. A closure an initializer made
/// captures the binding ahead, which nothing writes after the copy, as
/// the spec's captures the loop environment before the first
/// iteration. Answers the statements ahead, leaving the copies as the
/// loop's init; `None` when a declaration does not rewrite.
fn declarations_ahead(
    ast: &mut Ast,
    init: &mut Option<Box<Stmt>>,
    cx: &Cx,
    log: &mut LiftLog,
) -> Option<Vec<Stmt>> {
    let Some(Stmt::Multi(mut decls)) = init.as_deref().cloned() else {
        unreachable!("a lexical init of several declarations")
    };
    // every name the init declares is renamed before any initializer
    // is: a closure reads a later one when called, a direct read of it
    // stays one in the temporal dead zone, never a read of an outer one
    let mut fresh: HashMap<String, String> = HashMap::new();
    let mut reads = Vec::new();
    for d in &decls {
        match d {
            Stmt::LetDecl {
                name,
                is_var: false,
                ..
            } => {
                let read = ast.add_expr(Expr::Ident(String::new()));
                let ahead = format!("__awd_{}", read.0);
                ast.exprs[read.0 as usize] = Expr::Ident(ahead.clone());
                fresh.insert(name.clone(), ahead);
                reads.push(read);
            }
            // a `yield` in an initializer, hoisted to a declaration
            Stmt::YieldInto { var, value, .. } => {
                fresh.insert(var.clone(), format!("__awd_{}", value.0));
            }
            _ => {}
        }
    }
    // a direct read of a name declared at or after its own position
    // raises (§14.7.4.2 step 1 makes them all, uninitialized, first);
    // under its fresh name the read is one the dead-zone rule no longer
    // sees, so it is marked here
    let mut later: Vec<&str> = Vec::new();
    for d in decls.iter().rev() {
        if let Stmt::LetDecl {
            name,
            is_var: false,
            ..
        } = d
        {
            later.push(name.as_str());
        }
        let names: HashSet<&str> = later.iter().copied().collect();
        // the reads `d` makes itself, not a function written in it; a
        // mark the log does not undo: a body given up keeps these reads
        // under their own names, where they raise all the same
        let mut dead = HashSet::new();
        tdz_reads::mark_stmt(ast, d, &names, &HashSet::new(), &mut dead);
        ast.tdz_reads.extend(dead);
    }
    let before = LiftLog::reads_of(ast, &fresh);
    let mut copies = Vec::new();
    let mut reads = reads.into_iter();
    for d in &mut decls {
        match d {
            Stmt::LetDecl {
                mutable,
                name,
                type_ann,
                init: value,
                is_var: false,
            } => {
                rewrite_idents_in_expr(ast, *value, &fresh, true);
                let ahead = fresh[name.as_str()].clone();
                let Some(read) = reads.next() else {
                    unreachable!("one read per declaration")
                };
                copies.push(Stmt::LetDecl {
                    mutable: *mutable,
                    name: std::mem::replace(name, ahead),
                    type_ann: type_ann.clone(),
                    init: read,
                    is_var: false,
                });
            }
            Stmt::YieldInto { var, value, .. } => {
                rewrite_idents_in_expr(ast, *value, &fresh, true);
                *var = fresh[var.as_str()].clone();
            }
            // a destructuring's checks and default branches run in
            // their place, reading the fresh names
            _ => rewrite_idents_in_stmt(ast, d, &fresh, true),
        }
    }
    log.keep_renamed(ast, before);
    if !rewrite_list(ast, &mut decls, cx, log) {
        return None;
    }
    *init = Some(Box::new(Stmt::Multi(copies)));
    Some(decls)
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
            type_ann: None,
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
