//! §10.2.2 [[Construct]] step 15 for a flagged derived constructor:
//! a completion that answers `undefined` reads the this-binding, and
//! it does so after the body's own finalization — every `finally`,
//! every IteratorClose the return walks out of — has run. That
//! finalization may be the code that calls `super` (an iterator's
//! `return()` reaching an arrow that holds `super()`), so the check
//! cannot sit at the `return` itself.
//!
//! Each top-level statement ahead of the binding that holds such a
//! `return` of the constructor's own frame is wrapped in two labeled
//! blocks, and each of those returns becomes a `break` to the inner
//! label:
//!
//! ```text
//! done: { exit: { stmt; break done; } __this_bound || raise(false); return; }
//! ```
//!
//! The lowering already runs the finalizers and the iterator closes a
//! `break` leaves. Only a `break exit` reaches the check: when a
//! `finally` or an iterator's `return()` replaces the return's
//! completion with a break, a continue or a throw that the statement
//! then handles itself, control leaves `stmt` the ordinary way and
//! skips the exit, as the replaced return no longer happens. Wrapping
//! one statement at a time keeps the body flat, so the bindings a
//! statement declares stay where the rest of the body reads them.
//!
//! A `return v` whose value is not `undefined` answers itself (step
//! 13 picks an object and raises on any other value) without reading
//! the binding, so only an `undefined` takes the `break`: `v` is held
//! in a local typed by inference, which leaves an object or primitive
//! value on its typed path. A `v` that is syntactically an object or a
//! primitive literal keeps its `return` unchanged.

use super::desugar_classes_ctor_return::is_undefined_literal;
use super::desugar_classes_this_tdz::order::{binds, each_unbound, lists_mut, parser_site};
use super::desugar_classes_this_tdz::read_check;
use super::{Ast, BinOp, Expr, ExprId, Stmt};

const RET: &str = "__this_ret";
const EXIT: &str = "__this_exit_l";
const DONE: &str = "__this_done_l";

/// Whether `region` holds a `return` of the constructor's own frame
/// that may run before the binding and may answer `undefined`.
pub(super) fn has_own_return(ast: &Ast, region: &[Stmt]) -> bool {
    let mut any = false;
    each_unbound(ast, region, &parser_site, &mut |s| {
        any |= matches!(s, Stmt::Return(v) if v.is_none_or(|e| !answers_itself(ast, e)));
    });
    any
}

/// Route every own-frame `return` in `region` that may answer
/// `undefined` before the binding through one exit per statement.
pub(super) fn route_returns(ast: &mut Ast, region: &mut [Stmt]) {
    let mut n = 0;
    for (i, s) in region.iter_mut().enumerate() {
        let last = binds(ast, s, &parser_site);
        let before = n;
        let exit = format!("{EXIT}{i}");
        to_break(ast, s, &exit, &mut n);
        if n > before {
            let body = std::mem::replace(s, Stmt::Multi(Vec::new()));
            *s = wrap(ast, body, exit, format!("{DONE}{i}"));
        }
        if last {
            break;
        }
    }
}

/// `done: { exit: { body; break done; } __this_bound || raise(false); return; }`
fn wrap(ast: &mut Ast, body: Stmt, exit: String, done: String) -> Stmt {
    let inner = Stmt::Labeled {
        label: exit,
        body: Box::new(Stmt::Multi(vec![body, Stmt::Break(Some(done.clone()))])),
    };
    let check = read_check(ast);
    Stmt::Labeled {
        label: done,
        body: Box::new(Stmt::Multi(vec![
            inner,
            Stmt::Expr(check),
            Stmt::Return(None),
        ])),
    }
}

/// Is `e` an object or a primitive by its shape alone?
fn answers_itself(ast: &Ast, e: ExprId) -> bool {
    matches!(
        ast.get_expr(e),
        Expr::ObjectLit { .. }
            | Expr::Array(_)
            | Expr::New { .. }
            | Expr::ArrowFn { .. }
            | Expr::Number(_)
            | Expr::String(_)
            | Expr::Bool(_)
            | Expr::Null
            | Expr::BigInt { .. }
    )
}

fn to_break_list(ast: &mut Ast, list: &mut [Stmt], exit: &str, n: &mut usize) {
    for s in list.iter_mut() {
        let last = binds(ast, s, &parser_site);
        to_break(ast, s, exit, n);
        if last {
            break;
        }
    }
}

/// `return;` / `return undefined;` → `break exit;`, and `return v;` →
/// `{ const t = v; if (t !== undefined) return t; break exit; }`.
fn to_break(ast: &mut Ast, s: &mut Stmt, exit: &str, n: &mut usize) {
    let Stmt::Return(v) = s else {
        for l in lists_mut(s) {
            to_break_list(ast, l, exit, n);
        }
        return;
    };
    if v.is_some_and(|e| answers_itself(ast, e)) {
        return;
    }
    let slot = *n;
    *n += 1;
    let brk = Stmt::Break(Some(exit.into()));
    *s = match *v {
        None => brk,
        Some(e) if is_undefined_literal(ast, e) => brk,
        Some(e) => {
            let name = format!("{RET}{slot}");
            let decl = Stmt::LetDecl {
                mutable: false,
                name: name.clone(),
                type_ann: None,
                init: e,
                is_var: false,
            };
            let left = ast.add_expr(Expr::Ident(name.clone()));
            let right = ast.add_expr(Expr::Ident("undefined".into()));
            let cond = ast.add_expr(Expr::BinOp {
                op: BinOp::Neq,
                left,
                right,
            });
            let answer = ast.add_expr(Expr::Ident(name));
            let ret = Stmt::If {
                cond,
                then_branch: Box::new(Stmt::Return(Some(answer))),
                else_branch: None,
            };
            Stmt::Block(vec![decl, ret, brk])
        }
    };
}
