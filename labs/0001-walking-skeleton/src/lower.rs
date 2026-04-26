//! AST → IR.
//!
//! Each statement-position expression leaves its value on the stack;
//! the lowering emits `Pop` after each statement and `Ret` at end of program.

use std::rc::Rc;

use crate::ast::{Ast, Expr, ExprId, Stmt};
use crate::ir::{IrModule, Op};
use crate::value::Value;

pub fn lower(ast: &Ast) -> IrModule {
    let mut m = IrModule::default();
    for stmt in &ast.stmts {
        let Stmt::Expr(eid) = stmt;
        lower_expr(ast, &mut m, *eid);
        m.code.push(Op::Pop);
    }
    m.code.push(Op::Ret);
    m
}

fn lower_expr(ast: &Ast, m: &mut IrModule, eid: ExprId) {
    match ast.get_expr(eid) {
        Expr::String(s) => {
            let cid = intern_const(m, Value::String(Rc::new(s.clone())));
            m.code.push(Op::LoadConst(cid));
        }
        Expr::Number(n) => {
            let cid = intern_const(m, Value::Number(*n));
            m.code.push(Op::LoadConst(cid));
        }
        Expr::Member { obj, name } => {
            // P0: only Ident("console").log → host fn slot.
            // The type checker has already rejected anything else.
            if let Expr::Ident(obj_name) = ast.get_expr(*obj)
                && obj_name == "console"
                && name == "log"
            {
                let hid = intern_host(m, "console.log");
                m.code.push(Op::LoadHost(hid));
                return;
            }
            unreachable!("P0 lower: unsupported member access slipped past type-check");
        }
        Expr::Call { callee, args } => {
            lower_expr(ast, m, *callee);
            for a in args {
                lower_expr(ast, m, *a);
            }
            m.code.push(Op::Call(args.len() as u8));
        }
        Expr::Ident(_) => {
            unreachable!("P0 lower: bare ident slipped past type-check");
        }
    }
}

fn intern_const(m: &mut IrModule, v: Value) -> u32 {
    let id = m.consts.len() as u32;
    m.consts.push(v);
    id
}

fn intern_host(m: &mut IrModule, name: &str) -> u32 {
    if let Some(i) = m.host_fns.iter().position(|n| n == name) {
        return i as u32;
    }
    let id = m.host_fns.len() as u32;
    m.host_fns.push(name.into());
    id
}
