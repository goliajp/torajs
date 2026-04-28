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
    /// Hardcoded global stand-ins (currently only `console`). Real
    /// user-defined object types use `Type::Struct`.
    Object(&'static str),
    /// Homogeneous array. Owned by `Rc<Vec<Value>>` at runtime in v0.
    Array(Box<Type>),
    /// Structural object type — fields in declaration order. Two
    /// `Type::Struct` are equal iff they share field names + types in
    /// matching order. (TS-style structural compatibility, not nominal.)
    /// P2.4 introduced this; backed by heap allocation in P2.4.c.
    Struct(Vec<(String, Type)>),
}

impl Type {
    /// Cheap-to-duplicate types live entirely in registers / stack — using
    /// the binding twice just produces two independent copies with no
    /// runtime cost. Affine types own heap storage and follow Rust-shaped
    /// move semantics: each binding is the unique owner; consuming the
    /// binding (let-rhs / assign-rhs / call-arg / return) transfers
    /// ownership and the source name is marked moved.
    pub fn is_copy(&self) -> bool {
        matches!(
            self,
            Type::Number | Type::Boolean | Type::Void | Type::Any
        )
        // Struct, String, Function, Array — all heap-owned, all affine.
    }
}

fn resolve_type_ann(name: &str, aliases: &HashMap<String, Type>) -> Option<Type> {
    if let Some(rest) = name.strip_suffix("[]") {
        return resolve_type_ann(rest, aliases).map(|inner| Type::Array(Box::new(inner)));
    }
    match name {
        // `number` is the JS-spelled umbrella; `i64` and `f64` are explicit
        // Rust-shaped aliases. The typechecker treats all three as the same
        // numeric category — the SSA lowerer is what actually distinguishes
        // i64 vs f64 representation per `parse_type` in ssa_lower.rs.
        "number" | "i64" | "f64" => Some(Type::Number),
        "string" => Some(Type::String),
        "boolean" => Some(Type::Boolean),
        "void" => Some(Type::Void),
        // User-declared struct alias (P2.4): `type Point = { x: number, y: number }`
        // adds `Point` to the aliases map. Resolution returns the
        // structural Type::Struct directly — no nominal layer above.
        other => aliases.get(other).cloned(),
    }
}

fn build_fn_type(
    fn_name: &str,
    params: &[Param],
    return_type: &Option<String>,
    aliases: &HashMap<String, Type>,
) -> Result<Type, String> {
    let mut param_tys = Vec::new();
    for p in params {
        let Some(ann) = &p.type_ann else {
            return Err(format!(
                "parameter `{}` of function `{fn_name}` requires a type annotation",
                p.name
            ));
        };
        let Some(ty) = resolve_type_ann(ann, aliases) else {
            return Err(format!(
                "unknown type `{ann}` for parameter `{}` of function `{fn_name}`",
                p.name
            ));
        };
        param_tys.push(ty);
    }
    let ret_ty = match return_type {
        None => Type::Void,
        Some(t) => match resolve_type_ann(t, aliases) {
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
    /// Affine ownership flag. False until the binding's value is consumed
    /// (let-rhs, assign-rhs, non-Copy call-arg, return). After move, any
    /// further read of this binding is a type error. Copy-typed bindings
    /// never get marked.
    moved: bool,
}

pub fn check(ast: &Ast) -> Result<(), String> {
    let mut c = Checker {
        globals: HashMap::new(),
        scopes: vec![HashMap::new()],
        aliases: HashMap::new(),
        errors: Vec::new(),
        expected_return: None,
    };

    // Pass 0: register type aliases first so fn signatures + let
    // annotations can reference them. `type Point = { x: number, y: number }`
    // adds `Point → Type::Struct(...)` to `c.aliases`.
    for stmt in &ast.stmts {
        if let Stmt::TypeDecl { name, fields } = stmt {
            if c.aliases.contains_key(name) {
                c.errors.push(format!("redeclaration of type `{name}`"));
                continue;
            }
            let mut field_tys: Vec<(String, Type)> = Vec::new();
            let mut had_err = false;
            for (fname, fty_ann) in fields {
                match resolve_type_ann(fty_ann, &c.aliases) {
                    Some(ty) => field_tys.push((fname.clone(), ty)),
                    None => {
                        c.errors.push(format!(
                            "unknown type `{fty_ann}` for field `{fname}` of `{name}`"
                        ));
                        had_err = true;
                        break;
                    }
                }
            }
            if !had_err {
                c.aliases.insert(name.clone(), Type::Struct(field_tys));
            }
        }
    }

    // Pass 1: hoist top-level function signatures (uses aliases).
    for stmt in &ast.stmts {
        if let Stmt::FnDecl {
            name,
            params,
            return_type,
            ..
        } = stmt
        {
            match build_fn_type(name, params, return_type, &c.aliases) {
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

    // Pass 2: check each statement.
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
    /// User-declared type aliases — populated in pass 0 from
    /// `Stmt::TypeDecl`. `Point → Type::Struct(...)`.
    aliases: HashMap<String, Type>,
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

    /// Walk the scope stack from innermost outward and flip `moved=true`
    /// on the first matching binding. Caller must already have verified
    /// the binding exists.
    fn mark_moved(&mut self, name: &str) {
        for s in self.scopes.iter_mut().rev() {
            if let Some(info) = s.get_mut(name) {
                info.moved = true;
                return;
            }
        }
    }

    /// Inverse of `mark_moved` — the binding's slot now owns a fresh value
    /// (Assign rebound it). Used to clear any transient `moved` state set
    /// during rhs evaluation. Lets `s = s + "x"` work: the BinOp internally
    /// consumes s (because str+str consumes both), then Assign rebinds s
    /// with the concat result, so subsequent reads of s are fine.
    fn mark_unmoved(&mut self, name: &str) {
        for s in self.scopes.iter_mut().rev() {
            if let Some(info) = s.get_mut(name) {
                info.moved = false;
                return;
            }
        }
    }

    /// Consume an expression: if it resolves to a non-Copy binding-read
    /// (`Ident(name)`), mark that binding as moved. Other expression shapes
    /// produce fresh values (BinOp, Call return, literal, Index, etc.) —
    /// no source binding to flag. This is the move-detection hook called
    /// at the four consumption sites: let-rhs, assign-rhs, non-Copy call
    /// arg, return value.
    fn consume(&mut self, ast: &Ast, eid: ExprId) {
        if let Expr::Ident(name) = ast.get_expr(eid) {
            let name = name.clone();
            if let Some(info) = self.lookup(&name)
                && !info.ty.is_copy()
                && !info.moved
            {
                self.mark_moved(&name);
            }
        }
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
                        let Some(ann_ty) = resolve_type_ann(ann, &self.aliases) else {
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
                        moved: false,
                    },
                ) {
                    self.errors.push(e);
                }
                // Consume the rhs after recording the new binding (so that
                // `let a = x` correctly moves out of x — but only after x's
                // type-of read succeeded, which the lookup above implies).
                self.consume(ast, *init);
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
                            moved: false,
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
            Stmt::TypeDecl { .. } => {
                // Already handled in pass 0; re-encountering it during the
                // body walk is a no-op. (No nested type decls — top-level
                // only — but the AST shape allows them anywhere.)
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
                // Returning a non-Copy ident moves it out to the caller.
                if let Some(eid) = maybe_expr
                    && !expected.is_copy()
                {
                    self.consume(ast, *eid);
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
                    if info.moved {
                        return Err(format!("use of moved value `{name}`"));
                    }
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
                // Struct field access is the most general path — look up
                // the named field; type is whatever it was declared as.
                if let Type::Struct(fields) = &obj_ty
                    && let Some((_, ty)) = fields.iter().find(|(fname, _)| fname == name)
                {
                    return Ok(ty.clone());
                }
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
            Expr::ObjectLit { fields } => {
                // Infer a structural type from the literal's field types.
                // Order is preserved (matters for struct equality and
                // memory layout in the SSA layer). LetDecl downstream
                // compares this inferred type to its `: Point` annotation
                // — they match iff fields are listed in the same order
                // with matching types.
                //
                // Each non-Copy field expression also gets `consume()`d
                // since the literal takes ownership (e.g. `{ name: s }`
                // moves `s` into the struct).
                let entries: Vec<(String, ExprId)> = fields.clone();
                let mut field_tys: Vec<(String, Type)> = Vec::new();
                for (n, eid) in &entries {
                    let ty = self.type_of(ast, *eid)?;
                    if !ty.is_copy() {
                        self.consume(ast, *eid);
                    }
                    field_tys.push((n.clone(), ty));
                }
                Ok(Type::Struct(field_tys))
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
                    // Non-Copy params consume the arg binding (Rust-shaped
                    // move-on-pass). `Any` params (currently only
                    // `console.log`) borrow instead — the printer is a
                    // viewer, not an owner. Consuming an Any param would
                    // make `console.log(s); console.log(s)` an error,
                    // which we don't want for the most common shape.
                    if !param_ty.is_copy() && param_ty != &Type::Any {
                        self.consume(ast, *arg_id);
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
                            // String concat consumes both operands — the
                            // result is a fresh heap allocation, the inputs
                            // are folded into it.
                            self.consume(ast, *left);
                            self.consume(ast, *right);
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
                // Re-assignment moves the rhs into the binding's slot.
                // Two flag updates happen here:
                //   1. consume(value) — if the rhs is an Ident, mark that
                //      source binding moved.
                //   2. mark_unmoved(target) — clear any transient `moved`
                //      that fired during rhs evaluation (e.g. `s = s + "x"`
                //      consumes s inside the BinOp; the Assign re-binds
                //      it, so post-Assign reads of s are valid again).
                let target_name = match ast.get_expr(*target) {
                    Expr::Ident(n) => n.clone(),
                    _ => unreachable!("target was Ident — checked above"),
                };
                self.consume(ast, *value);
                self.mark_unmoved(&target_name);
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
                let fn_ty = build_fn_type("<arrow>", &params, &return_type, &self.aliases)?;
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
                            moved: false,
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

#[cfg(test)]
mod tests {
    use crate::lexer;
    use crate::parser;

    fn check_src(src: &str) -> Result<(), String> {
        let tokens = lexer::tokenize(src).map_err(|e| format!("lex: {e}"))?;
        let ast = parser::parse(&tokens).map_err(|e| format!("parse: {e}"))?;
        super::check(&ast)
    }

    #[test]
    fn copy_types_can_be_used_repeatedly() {
        // number is Copy — using `n` after `let m = n` is fine.
        let src = "let n: number = 5; let m: number = n; let r: number = n + m;";
        assert!(check_src(src).is_ok(), "expected ok, got {:?}", check_src(src));
    }

    #[test]
    fn move_then_use_errors() {
        // Move a string into b, then read a — should error.
        let src = r#"let a: string = "hello"; let b: string = a; let n: number = a.length;"#;
        let r = check_src(src);
        assert!(r.is_err(), "expected use-of-moved error, got {r:?}");
        assert!(
            r.as_ref().unwrap_err().contains("moved"),
            "expected 'moved' in error, got {r:?}"
        );
    }

    #[test]
    fn move_into_assign_then_use_errors() {
        let src = r#"
            let a: string = "x";
            let b: string = "y";
            b = a;
            let n: number = a.length;
        "#;
        let r = check_src(src);
        assert!(
            r.as_ref()
                .err()
                .map(|s| s.contains("moved"))
                .unwrap_or(false),
            "expected 'moved' in error, got {r:?}"
        );
    }

    #[test]
    fn string_concat_consumes_both() {
        let src = r#"
            let a: string = "x";
            let b: string = "y";
            let c: string = a + b;
            let n: number = a.length;
        "#;
        let r = check_src(src);
        assert!(
            r.as_ref()
                .err()
                .map(|s| s.contains("moved"))
                .unwrap_or(false),
            "expected 'moved' in error, got {r:?}"
        );
    }

    #[test]
    fn console_log_does_not_move() {
        // `console.log` is special: it's a borrow-style viewer (Any param
        // sidesteps move-on-pass). Calling twice is fine.
        let src = r#"
            let a: string = "x";
            console.log(a);
            console.log(a);
        "#;
        assert!(check_src(src).is_ok(), "got {:?}", check_src(src));
    }

    #[test]
    fn copy_args_are_borrowed() {
        // number args don't get moved; the caller can still read after the call.
        let src = r#"
            function id(x: number): number { return x; }
            let n: number = 5;
            let m: number = id(n);
            let r: number = n + m;
        "#;
        assert!(check_src(src).is_ok(), "got {:?}", check_src(src));
    }

    #[test]
    fn struct_field_access_works() {
        let src = r#"
            type Point = { x: number, y: number };
            let p: Point = { x: 3, y: 4 };
            let n: number = p.x;
        "#;
        assert!(check_src(src).is_ok(), "got {:?}", check_src(src));
    }

    #[test]
    fn struct_is_affine_move_then_use_errors() {
        // Struct is non-Copy — `let q = p` moves p, subsequent p.x should error.
        let src = r#"
            type Point = { x: number, y: number };
            let p: Point = { x: 3, y: 4 };
            let q: Point = p;
            let n: number = p.x;
        "#;
        let r = check_src(src);
        assert!(
            r.as_ref()
                .err()
                .map(|s| s.contains("moved"))
                .unwrap_or(false),
            "expected 'moved' error, got {r:?}"
        );
    }

    #[test]
    fn struct_field_type_must_resolve() {
        // Unknown type in field position errors at type-decl time.
        let src = "type Bad = { x: nope };";
        let r = check_src(src);
        assert!(
            r.as_ref()
                .err()
                .map(|s| s.contains("unknown type"))
                .unwrap_or(false),
            "expected 'unknown type' error, got {r:?}"
        );
    }

    #[test]
    fn struct_self_reference_unsupported() {
        // Forward reference in field — sibling alias must be declared first.
        // (We could relax to allow forward refs in pass 0; deferred. For
        // now this test pins the current behavior.)
        let src = r#"
            type A = { other: B };
            type B = { x: number };
        "#;
        let r = check_src(src);
        assert!(r.is_err(), "expected error from forward reference, got {r:?}");
    }
}
