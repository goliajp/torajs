//! The constructor half of §10.2.1 on the capturing lane: a class
//! constructor is entered by [[Construct]] only, with the new.target
//! the `new` (or the subclass's `super(…)`) hands it.
//!
//! The lane's constructor is a function expression, so a call through
//! the value (`K()`, `[2, 1].sort(K)`, `"a".replace(/a/, K)`) ran its
//! body where §10.2.1 step 2 throws. [`call_guard`] puts the throw at
//! the top of the body, keyed on `new.target`: a function expression
//! that reads it gets a construct face that `new`, `Reflect.construct`
//! and the `super(…)` kernel enter with the real value, while every
//! call through the value runs the call channel, where it is undefined
//! (`new_target_param.rs`, `check_monomorph_ctor_face.rs`). So an
//! undefined `new.target` is exactly the [[Call]].
//!
//! The guard is what makes `P.call(this, …)` — the `super(…)` a lane
//! sibling used to be lowered to — a [[Call]] that throws, and it also
//! read `new.target` as undefined inside `P`. A sibling's `super(…)` is
//! [[Construct]] with the subclass's new.target (§13.3.7.1 step 5), so
//! it goes through the kernel a value-shaped parent takes, which enters
//! the construct face.

use super::super::{Ast, Expr, ExprId, Stmt, UnaryOp};
use super::extends::REST_ARGS;

/// What a `super(…)` passes on: the site's own argument list, or the
/// implicit constructor's rest binding.
pub(super) enum SuperArgs {
    List(Vec<ExprId>),
    Rest,
}

/// `super(…)` into the lane sibling whose constructor is bound to
/// `target`: `__torajs_super_value(target, this, [args…], new.target)`.
pub(super) fn sibling_super(ast: &mut Ast, target: &str, args: SuperArgs) -> Expr {
    let callee = ast.add_expr(Expr::Ident("__torajs_super_value".to_string()));
    let p = ast.add_expr(Expr::Ident(target.to_string()));
    let this = ast.add_expr(Expr::This);
    let pack = match args {
        SuperArgs::List(a) => ast.add_expr(Expr::Array(a)),
        SuperArgs::Rest => ast.add_expr(Expr::Ident(REST_ARGS.to_string())),
    };
    let nt = ast.add_expr(Expr::NewTarget);
    Expr::Call {
        callee,
        args: vec![p, this, pack, nt],
    }
}

/// The synthetic raise the guard calls: a TypeError carrying bun's
/// (JSC's) message, raised the way a runtime kernel raises one, so a
/// program that never names `TypeError` needs no injected class for
/// it (`ssa_lower_call_super_value`).
pub(crate) const CALL_THROW: &str = "__torajs_class_call_throw";

/// `if (!new.target) __torajs_class_call_throw("<name>")` — `shown`
/// is the class's own name, which the message carries. new.target is
/// undefined or a constructor, so its falsiness is the [[Call]] test
/// (the guard names no `undefined` a program's own binding could take).
pub(super) fn call_guard(ast: &mut Ast, shown: &str) -> Stmt {
    let nt = ast.add_expr(Expr::NewTarget);
    let cond = ast.add_expr(Expr::Unary {
        op: UnaryOp::Not,
        expr: nt,
    });
    let callee = ast.add_expr(Expr::Ident(CALL_THROW.to_string()));
    let name = ast.add_expr(Expr::String(shown.to_string().into()));
    let raise = ast.add_expr(Expr::Call {
        callee,
        args: vec![name],
    });
    Stmt::If {
        cond,
        then_branch: Box::new(Stmt::Expr(raise)),
        else_branch: None,
    }
}

/// Is `s` a [`call_guard`]? A default the body takes over goes after it.
pub(crate) fn is_call_guard(ast: &Ast, s: &Stmt) -> bool {
    let Stmt::If {
        then_branch,
        else_branch: None,
        ..
    } = s
    else {
        return false;
    };
    matches!(then_branch.as_ref(), Stmt::Expr(e)
        if matches!(ast.get_expr(*e), Expr::Call { callee, .. }
            if matches!(ast.get_expr(*callee), Expr::Ident(n) if n == CALL_THROW)))
}
