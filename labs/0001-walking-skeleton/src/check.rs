//! Type checker. Subset:
//! - primitives: `number`, `string`, `boolean`, `void`
//! - hardcoded `console: { log: any -> void }`
//! - top-level `function` declarations (hoisted, monomorphic)
//! - lexical scope stack (`let`/`const` block-scoped; fn params are a fresh scope)

use std::collections::HashMap;

use crate::ast::{Ast, BinOp, Expr, ExprId, Param, Stmt};

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Number,
    String,
    Boolean,
    Void,
    /// v0 hack — `console.log`'s parameter accepts any printable type.
    /// Replace with a sum/union type later.
    Any,
    Function(Vec<Type>, Box<Type>),
    /// Object stand-in for hardcoded globals like `console`. Real object types come in P2.
    Object(&'static str),
    /// Homogeneous array. Owned by `Rc<Vec<Value>>` at runtime in v0.
    Array(Box<Type>),
}

fn resolve_type_ann(name: &str) -> Option<Type> {
    if let Some(rest) = name.strip_suffix("[]") {
        return resolve_type_ann(rest).map(|inner| Type::Array(Box::new(inner)));
    }
    match name {
        "number" => Some(Type::Number),
        "string" => Some(Type::String),
        "boolean" => Some(Type::Boolean),
        "void" => Some(Type::Void),
        _ => None,
    }
}

fn build_fn_type(
    fn_name: &str,
    params: &[Param],
    return_type: &Option<String>,
) -> Result<Type, String> {
    let mut param_tys = Vec::new();
    for p in params {
        let Some(ann) = &p.type_ann else {
            return Err(format!(
                "parameter `{}` of function `{fn_name}` requires a type annotation",
                p.name
            ));
        };
        let Some(ty) = resolve_type_ann(ann) else {
            return Err(format!(
                "unknown type `{ann}` for parameter `{}` of function `{fn_name}`",
                p.name
            ));
        };
        param_tys.push(ty);
    }
    let ret_ty = match return_type {
        None => Type::Void,
        Some(t) => match resolve_type_ann(t) {
            Some(ty) => ty,
            None => {
                return Err(format!(
                    "unknown return type `{t}` for function `{fn_name}`"
                ));
            }
        },
    };
    Ok(Type::Function(param_tys, Box::new(ret_ty)))
}

#[derive(Debug, Clone)]
struct LocalInfo {
    ty: Type,
    mutable: bool,
}

pub fn check(ast: &Ast) -> Result<(), String> {
    let mut c = Checker {
        globals: HashMap::new(),
        scopes: vec![HashMap::new()],
        errors: Vec::new(),
        expected_return: None,
    };

    // First pass: hoist top-level function signatures.
    for stmt in &ast.stmts {
        if let Stmt::FnDecl {
            name,
            params,
            return_type,
            ..
        } = stmt
        {
            match build_fn_type(name, params, return_type) {
                Ok(ty) => {
                    if c.globals.contains_key(name) {
                        c.errors.push(format!("redeclaration of function `{name}`"));
                    } else {
                        c.globals.insert(name.clone(), ty);
                    }
                }
                Err(e) => c.errors.push(e),
            }
        }
    }

    // Second pass: check each statement.
    for stmt in &ast.stmts {
        c.check_stmt(ast, stmt);
    }

    if c.errors.is_empty() {
        Ok(())
    } else {
        Err(c.errors.join("\n"))
    }
}

struct Checker {
    globals: HashMap<String, Type>,
    scopes: Vec<HashMap<String, LocalInfo>>,
    errors: Vec<String>,
    expected_return: Option<Type>,
}

impl Checker {
    fn declare(&mut self, name: String, info: LocalInfo) -> Result<(), String> {
        let top = self
            .scopes
            .last_mut()
            .expect("at least one scope is always present");
        if top.contains_key(&name) {
            return Err(format!("redeclaration of `{name}` in current scope"));
        }
        top.insert(name, info);
        Ok(())
    }

    fn lookup(&self, name: &str) -> Option<LocalInfo> {
        for s in self.scopes.iter().rev() {
            if let Some(i) = s.get(name) {
                return Some(i.clone());
            }
        }
        None
    }

    fn check_stmt(&mut self, ast: &Ast, stmt: &Stmt) {
        match stmt {
            Stmt::Expr(eid) => {
                if let Err(e) = self.type_of(ast, *eid) {
                    self.errors.push(e);
                }
            }
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                match self.type_of(ast, *cond) {
                    Ok(Type::Boolean) => {}
                    Ok(other) => self
                        .errors
                        .push(format!("if condition must be boolean, got {other:?}")),
                    Err(e) => self.errors.push(e),
                }
                self.check_stmt(ast, then_branch);
                if let Some(eb) = else_branch {
                    self.check_stmt(ast, eb);
                }
            }
            Stmt::While { cond, body } => {
                match self.type_of(ast, *cond) {
                    Ok(Type::Boolean) => {}
                    Ok(other) => self
                        .errors
                        .push(format!("while condition must be boolean, got {other:?}")),
                    Err(e) => self.errors.push(e),
                }
                self.check_stmt(ast, body);
            }
            Stmt::Block(stmts) => {
                self.scopes.push(HashMap::new());
                for s in stmts {
                    self.check_stmt(ast, s);
                }
                self.scopes.pop();
            }
            Stmt::LetDecl {
                mutable,
                name,
                type_ann,
                init,
            } => {
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
                if let Err(e) = self.declare(
                    name.clone(),
                    LocalInfo {
                        ty: final_ty,
                        mutable: *mutable,
                    },
                ) {
                    self.errors.push(e);
                }
            }
            Stmt::FnDecl {
                name, params, body, ..
            } => {
                // Signature already hoisted in the first pass.
                let Some(Type::Function(param_tys, ret_ty)) = self.globals.get(name).cloned()
                else {
                    // First pass had an error; skip body to avoid cascading.
                    return;
                };
                // Top-level FnDecl bodies see no outer locals (none exist) but do
                // see globals via lookup-fallback. We use a fresh scope stack to
                // mirror the arrow-fn rule (no captures).
                let saved_scopes = std::mem::replace(&mut self.scopes, vec![HashMap::new()]);
                let saved_return = self.expected_return.replace(*ret_ty);
                for (p, ty) in params.iter().zip(param_tys.iter()) {
                    if let Err(e) = self.declare(
                        p.name.clone(),
                        LocalInfo {
                            ty: ty.clone(),
                            mutable: true,
                        },
                    ) {
                        self.errors.push(e);
                    }
                }
                for s in body {
                    self.check_stmt(ast, s);
                }
                self.expected_return = saved_return;
                self.scopes = saved_scopes;
            }
            Stmt::Return(maybe_expr) => {
                let Some(expected) = self.expected_return.clone() else {
                    self.errors.push("`return` outside of a function".into());
                    return;
                };
                let actual = match maybe_expr {
                    None => Type::Void,
                    Some(eid) => match self.type_of(ast, *eid) {
                        Ok(t) => t,
                        Err(e) => {
                            self.errors.push(e);
                            return;
                        }
                    },
                };
                if actual != expected {
                    self.errors.push(format!(
                        "return type mismatch: function expects {expected:?}, got {actual:?}"
                    ));
                }
            }
        }
    }

    fn type_of(&mut self, ast: &Ast, eid: ExprId) -> Result<Type, String> {
        match ast.get_expr(eid) {
            Expr::String(_) => Ok(Type::String),
            Expr::Number(_) => Ok(Type::Number),
            Expr::Bool(_) => Ok(Type::Boolean),
            Expr::Ident(name) => {
                if let Some(info) = self.lookup(name) {
                    return Ok(info.ty);
                }
                if let Some(ty) = self.globals.get(name) {
                    return Ok(ty.clone());
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
                    (Type::String, "length") | (Type::Array(_), "length") => Ok(Type::Number),
                    _ => Err(format!("no member `.{name}` on type {obj_ty:?}")),
                }
            }
            Expr::Index { obj, index } => {
                let obj_ty = self.type_of(ast, *obj)?;
                let idx_ty = self.type_of(ast, *index)?;
                if idx_ty != Type::Number {
                    return Err(format!("index must be number, got {idx_ty:?}"));
                }
                match obj_ty {
                    Type::String => Ok(Type::String),
                    Type::Array(elem) => Ok(*elem),
                    other => Err(format!("can't index into {other:?}")),
                }
            }
            Expr::Array(elements) => {
                if elements.is_empty() {
                    return Err(
                        "empty array literal needs a type annotation (not yet supported in v0)"
                            .into(),
                    );
                }
                let ids: Vec<ExprId> = elements.clone();
                let first_ty = self.type_of(ast, ids[0])?;
                for &eid in ids.iter().skip(1) {
                    let ty = self.type_of(ast, eid)?;
                    if ty != first_ty {
                        return Err(format!(
                            "array element type mismatch: expected {first_ty:?}, got {ty:?}"
                        ));
                    }
                }
                Ok(Type::Array(Box::new(first_ty)))
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
            Expr::BinOp { op, left, right } => {
                let l = self.type_of(ast, *left)?;
                let r = self.type_of(ast, *right)?;
                match op {
                    BinOp::Add => {
                        if l == Type::Number && r == Type::Number {
                            Ok(Type::Number)
                        } else if l == Type::String && r == Type::String {
                            Ok(Type::String)
                        } else {
                            Err(format!(
                                "`+` requires both number or both string, got {l:?} and {r:?}"
                            ))
                        }
                    }
                    BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                        if l == Type::Number && r == Type::Number {
                            Ok(Type::Number)
                        } else {
                            Err(format!(
                                "arithmetic requires number operands, got {l:?} and {r:?}"
                            ))
                        }
                    }
                    BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr => {
                        if l == Type::Number && r == Type::Number {
                            Ok(Type::Number)
                        } else {
                            Err(format!(
                                "bitwise op requires number operands, got {l:?} and {r:?}"
                            ))
                        }
                    }
                    BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => {
                        if l == Type::Number && r == Type::Number {
                            Ok(Type::Boolean)
                        } else {
                            Err(format!(
                                "ordering comparison requires number operands, got {l:?} and {r:?}"
                            ))
                        }
                    }
                    BinOp::Eq | BinOp::Neq => {
                        if l == r && matches!(l, Type::Number | Type::String | Type::Boolean) {
                            Ok(Type::Boolean)
                        } else {
                            Err(format!(
                                "strict equality requires same primitive type, got {l:?} and {r:?}"
                            ))
                        }
                    }
                }
            }
            Expr::Assign { target, value } => {
                let Expr::Ident(name) = ast.get_expr(*target) else {
                    return Err("invalid assignment target".into());
                };
                let info = match self.lookup(name) {
                    Some(i) => i,
                    None => return Err(format!("assignment to undeclared `{name}`")),
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
            Expr::ArrowFn {
                params,
                return_type,
                body,
            } => {
                // Clone the body so we don't keep borrowing ast.exprs[eid] while
                // re-entering check_stmt below.
                let params = params.clone();
                let return_type = return_type.clone();
                let body = body.clone();
                let fn_ty = build_fn_type("<arrow>", &params, &return_type)?;
                let Type::Function(param_tys, ret_ty) = fn_ty.clone() else {
                    unreachable!("build_fn_type returned non-Function");
                };
                // Arrow fn body does NOT see outer locals — captures land in P4.
                let saved_scopes = std::mem::replace(&mut self.scopes, vec![HashMap::new()]);
                let saved_return = self.expected_return.replace(*ret_ty);
                for (p, ty) in params.iter().zip(param_tys.iter()) {
                    if let Err(e) = self.declare(
                        p.name.clone(),
                        LocalInfo {
                            ty: ty.clone(),
                            mutable: true,
                        },
                    ) {
                        self.errors.push(e);
                    }
                }
                for s in &body {
                    self.check_stmt(ast, s);
                }
                self.expected_return = saved_return;
                self.scopes = saved_scopes;
                Ok(fn_ty)
            }
        }
    }
}
