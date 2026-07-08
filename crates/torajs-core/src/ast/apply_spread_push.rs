//! `xs.push(...ys)` statement desugar (chunk 687, spread longtail
//! #1-push).
//!
//! The push lane takes one element-typed arg, so a trailing dynamic
//! spread desugars the STATEMENT form into a per-element loop:
//!
//! ```text
//! xs.push(a, ...ys);
//!   ⇒ { xs.push(a); for (let __spi_N = 0; __spi_N < ys.length;
//!        __spi_N = __spi_N + 1) { xs.push(ys[__spi_N]); } }
//! ```
//!
//! Everything downstream is existing surface: the loop guard keeps
//! the index reads bounds-proven, and the element type flows
//! naturally through the index-read + single-arg push lanes (number
//! / string / any element arrays all ride). Receiver and source
//! must both be Idents (an expression receiver would re-evaluate
//! per iteration). The EXPRESSION form (`const n = xs.push(...ys)`)
//! keeps the checker's loud reject — the loop shape has no value
//! position.

use super::{Ast, BinOp, Expr, ExprId, Stmt};

pub fn apply_spread_push(ast: &mut Ast) {
    let mut counter = 0usize;
    let mut stmts = std::mem::take(&mut ast.stmts);
    for s in &mut stmts {
        rewrite_stmt(ast, s, &mut counter);
    }
    ast.stmts = stmts;
}

fn rewrite_stmt(ast: &mut Ast, s: &mut Stmt, counter: &mut usize) {
    match s {
        Stmt::Expr(eid) => {
            if let Some(replacement) = try_desugar_push_spread(ast, *eid, counter) {
                *s = replacement;
            }
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for c in stmts {
                rewrite_stmt(ast, c, counter);
            }
        }
        Stmt::FnDecl { body, .. } => {
            for c in body {
                rewrite_stmt(ast, c, counter);
            }
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            rewrite_stmt(ast, then_branch, counter);
            if let Some(eb) = else_branch {
                rewrite_stmt(ast, eb, counter);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            rewrite_stmt(ast, body, counter);
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                rewrite_stmt(ast, i, counter);
            }
            rewrite_stmt(ast, body, counter);
        }
        _ => {}
    }
}

/// `Stmt::Expr(xs.push(prefix…, ...ys))` → the block replacement,
/// or None when the shape doesn't match (multiple spreads,
/// non-trailing spread, non-Ident receiver/source, other callees).
fn try_desugar_push_spread(ast: &mut Ast, eid: ExprId, counter: &mut usize) -> Option<Stmt> {
    let Expr::Call { callee, args } = ast.get_expr(eid) else {
        return None;
    };
    let Expr::Member { obj, name } = ast.get_expr(*callee) else {
        return None;
    };
    if name != "push" {
        return None;
    }
    let Expr::Ident(recv) = ast.get_expr(*obj) else {
        return None;
    };
    let spread_count = args
        .iter()
        .filter(|a| matches!(ast.get_expr(**a), Expr::Spread { .. }))
        .count();
    if spread_count != 1 {
        return None;
    }
    let (&last, prefix) = args.split_last()?;
    let Expr::Spread { expr } = ast.get_expr(last) else {
        return None;
    };
    let Expr::Ident(src) = ast.get_expr(*expr) else {
        return None;
    };
    let recv = recv.clone();
    let src = src.clone();
    let prefix: Vec<ExprId> = prefix.to_vec();
    let ivar = format!("__spi_{}", *counter);
    *counter += 1;
    let mut block: Vec<Stmt> = Vec::with_capacity(prefix.len() + 1);
    for p in prefix {
        block.push(Stmt::Expr(emit_push(ast, &recv, p)));
    }
    let zero = ast.add_expr(Expr::Number(0.0));
    let i_decl = Stmt::LetDecl {
        mutable: true,
        name: ivar.clone(),
        type_ann: Some("number".into()),
        init: zero,
        is_var: false,
    };
    let i_ref = ast.add_expr(Expr::Ident(ivar.clone()));
    let src_ref = ast.add_expr(Expr::Ident(src.clone()));
    let len = ast.add_expr(Expr::Member {
        obj: src_ref,
        name: "length".into(),
    });
    let cond = ast.add_expr(Expr::BinOp {
        op: BinOp::Lt,
        left: i_ref,
        right: len,
    });
    let i_ref2 = ast.add_expr(Expr::Ident(ivar.clone()));
    let one = ast.add_expr(Expr::Number(1.0));
    let i_plus = ast.add_expr(Expr::BinOp {
        op: BinOp::Add,
        left: i_ref2,
        right: one,
    });
    let i_tgt = ast.add_expr(Expr::Ident(ivar.clone()));
    let step = ast.add_expr(Expr::Assign {
        target: i_tgt,
        value: i_plus,
    });
    let src_ref2 = ast.add_expr(Expr::Ident(src));
    let i_ref3 = ast.add_expr(Expr::Ident(ivar));
    let elem = ast.add_expr(Expr::Index {
        obj: src_ref2,
        index: i_ref3,
    });
    let push_call = emit_push(ast, &recv, elem);
    block.push(Stmt::For {
        init: Some(Box::new(i_decl)),
        cond: Some(cond),
        step: Some(step),
        body: Box::new(Stmt::Block(vec![Stmt::Expr(push_call)])),
    });
    Some(Stmt::Block(block))
}

fn emit_push(ast: &mut Ast, recv: &str, arg: ExprId) -> ExprId {
    let recv_ref = ast.add_expr(Expr::Ident(recv.to_string()));
    let callee = ast.add_expr(Expr::Member {
        obj: recv_ref,
        name: "push".into(),
    });
    ast.add_expr(Expr::Call {
        callee,
        args: vec![arg],
    })
}
