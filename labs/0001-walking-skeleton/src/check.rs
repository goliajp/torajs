//! Type checker. P0: only `string`, `number`, `boolean`, `void`, function types,
//! plus the hardcoded global `console: { log: (string) -> void }`.

use crate::ast::{Ast, Expr, ExprId, Stmt};

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)] // Number is reserved for P1.1 arithmetic; not yet exercised in P0.
pub enum Type {
    Number,
    String,
    Void,
    Function(Vec<Type>, Box<Type>),
    /// Object stand-in for hardcoded globals like `console`. Real object types come in P2.
    Object(&'static str),
}

pub fn check(ast: &Ast) -> Result<(), String> {
    let mut errors = Vec::new();
    for stmt in &ast.stmts {
        let Stmt::Expr(eid) = stmt;
        if let Err(e) = type_of(ast, *eid) {
            errors.push(e);
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}

fn type_of(ast: &Ast, eid: ExprId) -> Result<Type, String> {
    match ast.get_expr(eid) {
        Expr::String(_) => Ok(Type::String),
        Expr::Number(_) => Ok(Type::Number),
        Expr::Ident(name) => match name.as_str() {
            "console" => Ok(Type::Object("console")),
            other => Err(format!("unknown identifier `{other}`")),
        },
        Expr::Member { obj, name } => {
            let obj_ty = type_of(ast, *obj)?;
            match (&obj_ty, name.as_str()) {
                (Type::Object("console"), "log") => {
                    Ok(Type::Function(vec![Type::String], Box::new(Type::Void)))
                }
                _ => Err(format!("no member `.{name}` on type {obj_ty:?}")),
            }
        }
        Expr::Call { callee, args } => {
            let callee_ty = type_of(ast, *callee)?;
            let Type::Function(params, ret) = callee_ty else {
                return Err(format!("not callable: type {callee_ty:?}"));
            };
            if params.len() != args.len() {
                return Err(format!(
                    "expected {} argument(s), got {}",
                    params.len(),
                    args.len()
                ));
            }
            for (i, (param_ty, arg_id)) in params.iter().zip(args.iter()).enumerate() {
                let arg_ty = type_of(ast, *arg_id)?;
                if &arg_ty != param_ty {
                    return Err(format!(
                        "argument {i}: expected {param_ty:?}, got {arg_ty:?}"
                    ));
                }
            }
            Ok(*ret)
        }
    }
}
