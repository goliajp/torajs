//! AST → IR.
//!
//! Each statement-position expression leaves its value on the stack;
//! the lowering emits `Pop` after each expr-stmt and `Ret` at end of program.
//! `let` bindings reserve a local slot and consume the init value via `StoreLocal`.

use std::collections::HashMap;
use std::rc::Rc;

use crate::ast::{Ast, BinOp, Expr, ExprId, Stmt};
use crate::ir::{IrModule, Op};
use crate::value::Value;

pub fn lower(ast: &Ast) -> IrModule {
    let mut m = IrModule::default();
    let mut locals: HashMap<String, u8> = HashMap::new();
    for stmt in &ast.stmts {
        lower_stmt(ast, &mut m, &mut locals, stmt);
    }
    m.code.push(Op::Ret);
    m.locals_count = locals.len() as u8;
    m
}

fn lower_stmt(ast: &Ast, m: &mut IrModule, locals: &mut HashMap<String, u8>, stmt: &Stmt) {
    match stmt {
        Stmt::Expr(eid) => {
            lower_expr(ast, m, locals, *eid);
            m.code.push(Op::Pop);
        }
        Stmt::LetDecl { name, init, .. } => {
            lower_expr(ast, m, locals, *init);
            let slot = locals.len() as u8;
            locals.insert(name.clone(), slot);
            m.code.push(Op::StoreLocal(slot));
        }
        Stmt::Block(stmts) => {
            for s in stmts {
                lower_stmt(ast, m, locals, s);
            }
        }
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            lower_expr(ast, m, locals, *cond);
            let br_pos = m.code.len();
            m.code.push(Op::BrFalse(0)); // patched after we know else target
            lower_stmt(ast, m, locals, then_branch);
            if let Some(eb) = else_branch {
                let jump_pos = m.code.len();
                m.code.push(Op::Jump(0));
                let else_target = m.code.len() as u32;
                m.code[br_pos] = Op::BrFalse(else_target);
                lower_stmt(ast, m, locals, eb);
                let end_target = m.code.len() as u32;
                m.code[jump_pos] = Op::Jump(end_target);
            } else {
                let end_target = m.code.len() as u32;
                m.code[br_pos] = Op::BrFalse(end_target);
            }
        }
    }
}

fn lower_expr(ast: &Ast, m: &mut IrModule, locals: &mut HashMap<String, u8>, eid: ExprId) {
    match ast.get_expr(eid) {
        Expr::String(s) => {
            let cid = intern_const(m, Value::String(Rc::new(s.clone())));
            m.code.push(Op::LoadConst(cid));
        }
        Expr::Number(n) => {
            let cid = intern_const(m, Value::Number(*n));
            m.code.push(Op::LoadConst(cid));
        }
        Expr::Bool(b) => {
            m.code.push(Op::LoadBool(*b));
        }
        Expr::Ident(name) => {
            let slot = *locals
                .get(name)
                .unwrap_or_else(|| panic!("lower: unknown identifier `{name}`"));
            m.code.push(Op::LoadLocal(slot));
        }
        Expr::Member { obj, name } => {
            // P0/P1: only Ident("console").log → host fn slot.
            // Type checker has already rejected anything else.
            if let Expr::Ident(obj_name) = ast.get_expr(*obj)
                && obj_name == "console"
                && name == "log"
            {
                let hid = intern_host(m, "console.log");
                m.code.push(Op::LoadHost(hid));
                return;
            }
            unreachable!("lower: unsupported member access slipped past type-check");
        }
        Expr::Call { callee, args } => {
            lower_expr(ast, m, locals, *callee);
            for a in args {
                lower_expr(ast, m, locals, *a);
            }
            m.code.push(Op::Call(args.len() as u8));
        }
        Expr::BinOp { op, left, right } => {
            lower_expr(ast, m, locals, *left);
            lower_expr(ast, m, locals, *right);
            m.code.push(match op {
                BinOp::Add => Op::Add,
                BinOp::Sub => Op::Sub,
                BinOp::Mul => Op::Mul,
                BinOp::Div => Op::Div,
                BinOp::Lt => Op::Lt,
                BinOp::Gt => Op::Gt,
                BinOp::Le => Op::Le,
                BinOp::Ge => Op::Ge,
                BinOp::Eq => Op::Eq3,
                BinOp::Neq => Op::Neq3,
            });
        }
        Expr::Assign { target, value } => {
            let Expr::Ident(name) = ast.get_expr(*target) else {
                unreachable!("lower: non-ident assignment target slipped past type-check");
            };
            let slot = *locals
                .get(name)
                .unwrap_or_else(|| panic!("lower: assign to undeclared `{name}`"));
            lower_expr(ast, m, locals, *value);
            m.code.push(Op::StoreLocal(slot));
            // assignment expression evaluates to the assigned value
            m.code.push(Op::LoadLocal(slot));
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
