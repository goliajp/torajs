//! The AST-building helpers `desugar_generators_yieldstar` spells its
//! protocol with — names, member reads, calls, the nullish and
//! not-an-object tests, a `TypeError` — moved out so the protocol file
//! stays under the 500-line limit.

use super::{Ast, BinOp, Expr, ExprId, Stmt};

/// `name = value;`
pub(super) fn assign_stmt(ast: &mut Ast, name: &str, value: ExprId) -> Stmt {
    let target = ident(ast, name);
    let assign = ast.add_expr(Expr::Assign { target, value });
    Stmt::Expr(assign)
}

pub(super) fn ident(ast: &mut Ast, name: &str) -> ExprId {
    ast.add_expr(Expr::Ident(name.to_string()))
}

pub(super) fn member(ast: &mut Ast, obj: &str, name: &str) -> ExprId {
    let o = ident(ast, obj);
    ast.add_expr(Expr::Member {
        obj: o,
        name: name.to_string(),
    })
}

pub(super) fn call_method(ast: &mut Ast, obj: ExprId, name: &str, args: Vec<ExprId>) -> ExprId {
    let callee = ast.add_expr(Expr::Member {
        obj,
        name: name.to_string(),
    });
    ast.add_expr(Expr::Call { callee, args })
}

pub(super) fn let_any(name: &str, init: ExprId) -> Stmt {
    Stmt::LetDecl {
        mutable: false,
        name: name.to_string(),
        type_ann: Some("any".into()),
        init,
        is_var: false,
    }
}

pub(super) fn not_null(ast: &mut Ast, name: &str) -> ExprId {
    let l = ident(ast, name);
    let r = ast.add_expr(Expr::Null);
    ast.add_expr(Expr::BinOp {
        op: BinOp::LooseNeq,
        left: l,
        right: r,
    })
}

pub(super) fn is_null(ast: &mut Ast, name: &str) -> ExprId {
    let l = ident(ast, name);
    let r = ast.add_expr(Expr::Null);
    ast.add_expr(Expr::BinOp {
        op: BinOp::LooseEq,
        left: l,
        right: r,
    })
}

/// `x == null || (typeof x !== "object" && typeof x !== "function")`.
pub(super) fn not_object(ast: &mut Ast, name: &str) -> ExprId {
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

pub(super) fn string(ast: &mut Ast, s: &str) -> ExprId {
    ast.add_expr(Expr::String(torajs_wtf8::Wtf8Buf::from(s)))
}

pub(super) fn type_error(ast: &mut Ast, msg: &str) -> ExprId {
    let m = string(ast, msg);
    ast.add_expr(Expr::New {
        class_name: "TypeError".into(),
        args: vec![m],
        type_args: Vec::new(),
    })
}

/// `this.<name>`.
pub(super) fn this_field(ast: &mut Ast, name: &str) -> ExprId {
    let this_id = ast.add_expr(Expr::This);
    ast.add_expr(Expr::Member {
        obj: this_id,
        name: name.to_string(),
    })
}
