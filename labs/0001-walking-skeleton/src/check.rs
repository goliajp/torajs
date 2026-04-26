//! Type checker. Subset: `number`, `string`, `void`, function types, hardcoded
//! `console: { log: any -> void }`, and a flat scope of `let`-bound locals.

use std::collections::HashMap;

use crate::ast::{Ast, Expr, ExprId, Stmt};

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Number,
    String,
    Boolean,
    Void,
    /// v0 hack — `console.log` parameter so it accepts any printable type.
    /// Replace with proper sum/union types once we have them.
    Any,
    Function(Vec<Type>, Box<Type>),
    /// Object stand-in for hardcoded globals like `console`. Real object types come in P2.
    Object(&'static str),
}

fn resolve_type_ann(name: &str) -> Option<Type> {
    match name {
        "number" => Some(Type::Number),
        "string" => Some(Type::String),
        "boolean" => Some(Type::Boolean),
        _ => None,
    }
}

pub fn check(ast: &Ast) -> Result<(), String> {
    let mut c = Checker {
        locals: HashMap::new(),
        errors: Vec::new(),
    };
    for stmt in &ast.stmts {
        c.check_stmt(ast, stmt);
    }
    if c.errors.is_empty() {
        Ok(())
    } else {
        Err(c.errors.join("\n"))
    }
}

#[derive(Debug, Clone)]
struct LocalInfo {
    ty: Type,
    mutable: bool,
}

struct Checker {
    locals: HashMap<String, LocalInfo>,
    errors: Vec<String>,
}

impl Checker {
    fn check_stmt(&mut self, ast: &Ast, stmt: &Stmt) {
        match stmt {
            Stmt::Expr(eid) => {
                if let Err(e) = self.type_of(ast, *eid) {
                    self.errors.push(e);
                }
            }
            Stmt::LetDecl {
                mutable,
                name,
                type_ann,
                init,
            } => {
                if self.locals.contains_key(name) {
                    self.errors.push(format!("redeclaration of `{name}`"));
                    return;
                }
                let init_ty = match self.type_of(ast, *init) {
                    Ok(t) => t,
                    Err(e) => {
                        self.errors.push(e);
                        return;
                    }
                };
                let final_ty = match type_ann {
                    None => init_ty,
                    Some(ann) => {
                        let Some(ann_ty) = resolve_type_ann(ann) else {
                            self.errors.push(format!("unknown type `{ann}`"));
                            return;
                        };
                        if ann_ty != init_ty {
                            self.errors.push(format!(
                                "type mismatch on `{name}`: declared {ann_ty:?}, init has {init_ty:?}"
                            ));
                            return;
                        }
                        ann_ty
                    }
                };
                self.locals.insert(
                    name.clone(),
                    LocalInfo {
                        ty: final_ty,
                        mutable: *mutable,
                    },
                );
            }
        }
    }

    fn type_of(&self, ast: &Ast, eid: ExprId) -> Result<Type, String> {
        match ast.get_expr(eid) {
            Expr::String(_) => Ok(Type::String),
            Expr::Number(_) => Ok(Type::Number),
            Expr::Ident(name) => {
                if let Some(info) = self.locals.get(name) {
                    return Ok(info.ty.clone());
                }
                match name.as_str() {
                    "console" => Ok(Type::Object("console")),
                    other => Err(format!("unknown identifier `{other}`")),
                }
            }
            Expr::Member { obj, name } => {
                let obj_ty = self.type_of(ast, *obj)?;
                match (&obj_ty, name.as_str()) {
                    (Type::Object("console"), "log") => {
                        Ok(Type::Function(vec![Type::Any], Box::new(Type::Void)))
                    }
                    _ => Err(format!("no member `.{name}` on type {obj_ty:?}")),
                }
            }
            Expr::Call { callee, args } => {
                let callee_ty = self.type_of(ast, *callee)?;
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
                    let arg_ty = self.type_of(ast, *arg_id)?;
                    if param_ty != &Type::Any && &arg_ty != param_ty {
                        return Err(format!(
                            "argument {i}: expected {param_ty:?}, got {arg_ty:?}"
                        ));
                    }
                }
                Ok(*ret)
            }
            Expr::BinOp { op: _, left, right } => {
                let l = self.type_of(ast, *left)?;
                let r = self.type_of(ast, *right)?;
                match (&l, &r) {
                    (Type::Number, Type::Number) => Ok(Type::Number),
                    _ => Err(format!(
                        "arithmetic requires number operands, got {l:?} and {r:?}"
                    )),
                }
            }
            Expr::Assign { target, value } => {
                let Expr::Ident(name) = ast.get_expr(*target) else {
                    return Err("invalid assignment target".into());
                };
                let Some(info) = self.locals.get(name) else {
                    return Err(format!("assignment to undeclared `{name}`"));
                };
                if !info.mutable {
                    return Err(format!("cannot assign to const `{name}`"));
                }
                let target_ty = info.ty.clone();
                let value_ty = self.type_of(ast, *value)?;
                if value_ty != target_ty {
                    return Err(format!(
                        "type mismatch assigning to `{name}`: declared {target_ty:?}, value is {value_ty:?}"
                    ));
                }
                Ok(target_ty)
            }
        }
    }
}
