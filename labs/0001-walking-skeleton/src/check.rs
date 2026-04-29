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

    /// Like `lookup` but also returns the scope depth at which the binding
    /// was found (0 = outermost / fn-root, `scopes.len() - 1` = innermost).
    /// M1.3 uses this to detect cross-scope `let n = s` cases — an Ident
    /// init from an outer scope is treated as alias-only (n borrows s's
    /// heap, both stay readable; no ownership transfer that would dangle
    /// the outer reference at this block's close).
    fn lookup_with_depth(&self, name: &str) -> Option<(LocalInfo, usize)> {
        for (i, s) in self.scopes.iter().enumerate().rev() {
            if let Some(info) = s.get(name) {
                return Some((info.clone(), i));
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

    /// Try to transfer ownership FROM the given expression. Called at the
    /// four transfer sites: let-rhs, assign-rhs, non-Copy fn arg, return
    /// value, struct field write.
    ///
    /// TS-shape semantics: `let n = s; console.log(s);` works — both
    /// bindings read the same heap. But ambiguous multi-rooted ownership
    /// (`let n = s; let c = { name: s };` — s aliased AND moved into struct)
    /// can't be statically resolved without a runtime mechanism we don't
    /// have, so we **reject at compile time**: the second transfer of an
    /// already-aliased binding is an error. The user restructures (e.g.
    /// transfers from `n` instead of `s`).
    ///
    /// Member / Index reads of obj's field are NOT transfers — the field's
    /// heap is owned by obj, and the new binding is an alias (handled at
    /// the LetDecl site via `classify_init_alias`, not here).
    fn consume(&mut self, ast: &Ast, eid: ExprId) {
        if let Expr::Ident(name) = ast.get_expr(eid) {
            let name = name.clone();
            if let Some(info) = self.lookup(&name) {
                if info.ty.is_copy() {
                    return;
                }
                if info.moved {
                    self.errors.push(format!(
                        "cannot transfer `{name}` — value was already aliased or moved earlier; transfer from the most recent binding instead"
                    ));
                    return;
                }
                self.mark_moved(&name);
            }
        }
    }

    /// Decide whether a let-bound or struct-field's init expression
    /// produces a fresh-owned value or aliases an existing one. Member
    /// and Index reads (`obj.field`, `arr[i]`) yield aliases — the heap
    /// is still owned by obj/arr; the new binding just holds a pointer
    /// for shared-read access. M1.3 extends this to cross-scope Ident
    /// init: when `s` lives in an outer scope, `let n = s` becomes an
    /// alias (otherwise transferring would dangle the outer reference
    /// when the inner block's drop fires). Same-scope Ident init is
    /// still a transfer — handled by `consume` at the let-decl site.
    /// Fresh-value init (literal, Call return, BinOp, ObjectLit, Array)
    /// produces a new owner; not an alias.
    fn classify_init_alias(&self, ast: &Ast, eid: ExprId) -> bool {
        match ast.get_expr(eid) {
            Expr::Member { .. } | Expr::Index { .. } => true,
            Expr::Ident(name) => {
                if let Some((_, src_depth)) = self.lookup_with_depth(name) {
                    let cur_depth = self.scopes.len() - 1;
                    src_depth < cur_depth
                } else {
                    false
                }
            }
            _ => false,
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
                // M1.2 — empty array literal `[]` carries no element-type
                // info; the annotation must provide it. Special-case to
                // skip type_of (which would error) and use the annotation
                // directly. Matches TS / bun: `let xs: number[] = [];`.
                let is_empty_array =
                    matches!(ast.get_expr(*init), Expr::Array(els) if els.is_empty());
                let init_ty = if is_empty_array {
                    let Some(ann) = type_ann else {
                        self.errors.push(format!(
                            "empty array literal `{name}` needs an explicit type annotation, e.g. `let {name}: number[] = []`"
                        ));
                        return;
                    };
                    let Some(ann_ty) = resolve_type_ann(ann, &self.aliases) else {
                        self.errors.push(format!("unknown type `{ann}`"));
                        return;
                    };
                    if !matches!(ann_ty, Type::Array(_)) {
                        self.errors.push(format!(
                            "empty array literal `{name}` needs an array type annotation, got `{ann}`"
                        ));
                        return;
                    }
                    ann_ty
                } else {
                    match self.type_of(ast, *init) {
                        Ok(t) => t,
                        Err(e) => {
                            self.errors.push(e);
                            return;
                        }
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
                // Member / Index init aliases obj's field — the new binding
                // doesn't own its heap, just borrows the obj's. Mark `moved`
                // so end-of-scope drop emission skips it (the obj's drop
                // walk handles the field's heap). For all other init shapes
                // (Ident, Call, BinOp, literal, ObjectLit), the new binding
                // owns: either it took transfer from a source (Ident → see
                // `consume` below), or the value is fresh.
                let is_alias_init = self.classify_init_alias(ast, *init);
                if let Err(e) = self.declare(
                    name.clone(),
                    LocalInfo {
                        ty: final_ty,
                        mutable: *mutable,
                        moved: is_alias_init,
                    },
                ) {
                    self.errors.push(e);
                }
                // Transfer ownership from the rhs only on owner-init
                // (alias-init keeps the source as owner). M1.3: this
                // catches the cross-scope Ident case — `let n = s` where
                // s is in an outer scope skips the consume so s remains
                // the owner; the alias n's slot drops as a no-op.
                if !is_alias_init {
                    self.consume(ast, *init);
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
                    // TS-shape: reads of an aliased / moved binding succeed
                    // (both `s` and `n` after `let n = s` reference the same
                    // heap and read the same value). Errors only fire at
                    // transfer sites — see `consume` above for the rule.
                    return Ok(info.ty);
                }
                if let Some(ty) = self.globals.get(name) {
                    return Ok(ty.clone());
                }
                match name.as_str() {
                    "console" => Ok(Type::Object("console")),
                    "Math" => Ok(Type::Object("Math")),
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
                    // `Math` global — every method takes one number and
                    // returns a number. f64-flavored at the SSA level
                    // (the lowerer auto-promotes integer args), but
                    // check.rs uses the umbrella Type::Number.
                    (Type::Object("Math"), m)
                        if matches!(
                            m,
                            "sqrt" | "abs" | "floor" | "ceil" | "log" | "exp"
                        ) =>
                    {
                        Ok(Type::Function(vec![Type::Number], Box::new(Type::Number)))
                    }
                    // Two-arg methods: pow(x, y), min(a, b), max(a, b).
                    (Type::Object("Math"), m)
                        if matches!(m, "pow" | "min" | "max") =>
                    {
                        Ok(Type::Function(
                            vec![Type::Number, Type::Number],
                            Box::new(Type::Number),
                        ))
                    }
                    // Constants — read directly without parens.
                    (Type::Object("Math"), m) if matches!(m, "PI" | "E") => {
                        Ok(Type::Number)
                    }
                    (Type::String, "length") | (Type::Array(_), "length") => Ok(Type::Number),
                    // M1.2 — `xs.push(v)`: takes one element-typed arg,
                    // returns void (TS doesn't surface push's "new length"
                    // return value in our subset since it's rarely useful).
                    (Type::Array(elem), "push") => {
                        let inner = (**elem).clone();
                        Ok(Type::Function(vec![inner], Box::new(Type::Void)))
                    }
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
                            // TS-shape: `a + b` reads both operands, returns
                            // a fresh string. Operands keep their heaps —
                            // `a` and `b` are still readable + droppable
                            // afterwards (matches bun / standard TS).
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
                    BinOp::LAnd | BinOp::LOr => {
                        // M1.5 — boolean ops are bool-only in the subset
                        // (no truthy coercion). Both operands must be bool;
                        // result is bool. Short-circuit semantics is
                        // observable at runtime via the lowerer's CFG split.
                        if l == Type::Boolean && r == Type::Boolean {
                            Ok(Type::Boolean)
                        } else {
                            Err(format!(
                                "`&&` / `||` require boolean operands, got {l:?} and {r:?}"
                            ))
                        }
                    }
                }
            }
            Expr::Unary { op, expr } => {
                let t = self.type_of(ast, *expr)?;
                match op {
                    crate::ast::UnaryOp::Not => {
                        if t == Type::Boolean {
                            Ok(Type::Boolean)
                        } else {
                            Err(format!("`!` requires boolean operand, got {t:?}"))
                        }
                    }
                }
            }
            Expr::Assign { target, value } => {
                match ast.get_expr(*target).clone() {
                    Expr::Ident(name) => {
                        let info = match self.lookup(&name) {
                            Some(i) => i,
                            None => {
                                return Err(format!(
                                    "assignment to undeclared `{name}`"
                                ));
                            }
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
                        // Reassign moves rhs in. consume marks Ident sources
                        // moved; mark_unmoved clears the target's transient
                        // moved if rhs was `target + ...` (e.g. string concat).
                        self.consume(ast, *value);
                        self.mark_unmoved(&name);
                        Ok(target_ty)
                    }
                    Expr::Member { obj, name: field } => {
                        // M1.4 — `obj.field = value`. Type-check the field
                        // write: obj must be a Struct with `field`, value
                        // type matches the field's declared type. For
                        // non-Copy fields the old value's heap is dropped
                        // by the lowerer; we only typecheck here.
                        let obj_ty = self.type_of(ast, obj)?;
                        let Type::Struct(fields) = &obj_ty else {
                            return Err(format!(
                                "field assignment target must be a struct, got {obj_ty:?}"
                            ));
                        };
                        let Some((_, field_ty)) =
                            fields.iter().find(|(n, _)| n == &field)
                        else {
                            return Err(format!(
                                "no field `{field}` on type {obj_ty:?}"
                            ));
                        };
                        let field_ty = field_ty.clone();
                        let value_ty = self.type_of(ast, *value)?;
                        if value_ty != field_ty {
                            return Err(format!(
                                "type mismatch assigning to `{field}`: field is {field_ty:?}, value is {value_ty:?}"
                            ));
                        }
                        // Value transfers into the struct field — Ident
                        // sources get marked moved.
                        self.consume(ast, *value);
                        Ok(field_ty)
                    }
                    Expr::Index { obj, index } => {
                        // M1.4 — `arr[i] = value`. obj must be Array<T>;
                        // index must be number; value type must match elem.
                        let obj_ty = self.type_of(ast, obj)?;
                        let idx_ty = self.type_of(ast, index)?;
                        if idx_ty != Type::Number {
                            return Err(format!(
                                "index must be number, got {idx_ty:?}"
                            ));
                        }
                        let Type::Array(elem) = &obj_ty else {
                            return Err(format!(
                                "index assignment target must be an array, got {obj_ty:?}"
                            ));
                        };
                        let elem_ty = (**elem).clone();
                        let value_ty = self.type_of(ast, *value)?;
                        if value_ty != elem_ty {
                            return Err(format!(
                                "type mismatch on element assignment: array of {elem_ty:?}, value is {value_ty:?}"
                            ));
                        }
                        self.consume(ast, *value);
                        Ok(elem_ty)
                    }
                    _ => Err("invalid assignment target".into()),
                }
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
    fn shared_reads_after_let_alias_succeed() {
        // TS-shape: `let b = a` aliases the heap. Both `a` and `b` are
        // readable afterwards — the underlying string is the same value,
        // and end-of-scope drops it once via the b binding (a transferred
        // ownership at the let).
        let src = r#"
            let a: string = "hello";
            let b: string = a;
            let n: number = a.length;
            let m: number = b.length;
        "#;
        assert!(check_src(src).is_ok(), "got {:?}", check_src(src));
    }

    #[test]
    fn shared_reads_after_assign_succeed() {
        // TS-shape: after `b = a`, both names alias the same heap.
        // a is still readable.
        let src = r#"
            let a: string = "x";
            let b: string = "y";
            b = a;
            let n: number = a.length;
        "#;
        assert!(check_src(src).is_ok(), "got {:?}", check_src(src));
    }

    #[test]
    fn re_transfer_of_aliased_binding_errors() {
        // Multi-rooted ownership can't be statically resolved without a
        // runtime mechanism (refcount / GC). Rejecting at compile-time:
        // after `let b = a`, transferring `a` again into `c` is the
        // ambiguous case. User restructures to transfer from `b` instead.
        let src = r#"
            let a: string = "x";
            let b: string = a;
            let c: string = a;
        "#;
        let r = check_src(src);
        assert!(
            r.as_ref()
                .err()
                .map(|s| s.contains("cannot transfer"))
                .unwrap_or(false),
            "expected transfer-after-aliased error, got {r:?}"
        );
    }

    #[test]
    fn string_concat_does_not_consume_operands() {
        // TS-shape: `a + b` reads bytes from both, returns a fresh string.
        // Operands keep their heaps (matches bun) and remain readable.
        let src = r#"
            let a: string = "x";
            let b: string = "y";
            let c: string = a + b;
            let n: number = a.length;
            let m: number = b.length;
        "#;
        assert!(check_src(src).is_ok(), "got {:?}", check_src(src));
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
    fn struct_alias_reads_succeed() {
        // TS-shape: `let q = p` aliases the same struct. Reading p's
        // fields after still works; one drop fires at end of scope (via q,
        // the current owner; p transferred).
        let src = r#"
            type Point = { x: number, y: number };
            let p: Point = { x: 3, y: 4 };
            let q: Point = p;
            let n: number = p.x;
            let m: number = q.y;
        "#;
        assert!(check_src(src).is_ok(), "got {:?}", check_src(src));
    }

    #[test]
    fn struct_re_transfer_errors() {
        // Multi-rooted struct: `let q = p; let r = p` would alias p twice
        // AND claim two owners — rejected.
        let src = r#"
            type Point = { x: number, y: number };
            let p: Point = { x: 3, y: 4 };
            let q: Point = p;
            let r: Point = p;
        "#;
        let r = check_src(src);
        assert!(
            r.as_ref()
                .err()
                .map(|s| s.contains("cannot transfer"))
                .unwrap_or(false),
            "expected transfer-after-aliased error, got {r:?}"
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

    // ----- M1.5 — boolean ops -----

    #[test]
    fn logical_and_or_not_typecheck() {
        let src = r#"
            let a: boolean = true;
            let b: boolean = false;
            let r1: boolean = a && b;
            let r2: boolean = a || b;
            let r3: boolean = !a;
            let r4: boolean = a && !b;
            let r5: boolean = (a || b) && !a;
        "#;
        assert!(check_src(src).is_ok(), "got {:?}", check_src(src));
    }

    #[test]
    fn logical_op_on_non_bool_errors() {
        let src = "let n: number = 1; let r: boolean = n && true;";
        let r = check_src(src);
        assert!(
            r.as_ref()
                .err()
                .map(|s| s.contains("`&&` / `||`") || s.contains("boolean"))
                .unwrap_or(false),
            "expected boolean-required error, got {r:?}"
        );
    }

    #[test]
    fn not_on_non_bool_errors() {
        let src = "let n: number = 1; let r: boolean = !n;";
        let r = check_src(src);
        assert!(
            r.as_ref()
                .err()
                .map(|s| s.contains("`!`") || s.contains("boolean"))
                .unwrap_or(false),
            "expected boolean-required error, got {r:?}"
        );
    }

    // ----- M1.4 — mutable struct field write + array index write -----

    #[test]
    fn struct_field_write_typechecks() {
        let src = r#"
            type Point = { x: number, y: number };
            let p: Point = { x: 1, y: 2 };
            p.x = 10;
            p.y = 20;
            let n: number = p.x;
        "#;
        assert!(check_src(src).is_ok(), "got {:?}", check_src(src));
    }

    #[test]
    fn struct_field_write_type_mismatch_errors() {
        let src = r#"
            type Point = { x: number, y: number };
            let p: Point = { x: 1, y: 2 };
            p.x = "hello";
        "#;
        let r = check_src(src);
        assert!(
            r.as_ref()
                .err()
                .map(|s| s.contains("type mismatch"))
                .unwrap_or(false),
            "expected type-mismatch error, got {r:?}"
        );
    }

    #[test]
    fn array_index_write_typechecks() {
        let src = r#"
            let xs: number[] = [];
            xs.push(0);
            xs[0] = 42;
            let n: number = xs[0];
        "#;
        assert!(check_src(src).is_ok(), "got {:?}", check_src(src));
    }

    #[test]
    fn array_index_write_type_mismatch_errors() {
        let src = r#"
            let xs: number[] = [];
            xs.push(0);
            xs[0] = "hello";
        "#;
        let r = check_src(src);
        assert!(
            r.as_ref()
                .err()
                .map(|s| s.contains("type mismatch"))
                .unwrap_or(false),
            "expected type-mismatch error, got {r:?}"
        );
    }

    // ----- M1.3 — block-scope drops + cross-scope alias -----

    #[test]
    fn cross_scope_let_is_alias_outer_still_readable() {
        // `let n = s` where s is in outer scope: under M1.3 this is
        // alias-only — n borrows s's heap, both readable, only one
        // drop fires (s at fn-end; n's slot is alias and skipped).
        let src = r#"
            function f(): number {
                let s: string = "outer";
                {
                    let n: string = s;
                    let m: number = n.length;
                }
                let len: number = s.length;
                return len;
            }
            console.log(f());
        "#;
        assert!(check_src(src).is_ok(), "got {:?}", check_src(src));
    }

    #[test]
    fn cross_scope_alias_does_not_consume_source() {
        // After cross-scope `let n = s; ...`, s should still be usable
        // via assign target without a "moved" error.
        let src = r#"
            function f(): number {
                let s: string = "x";
                {
                    let n: string = s;
                    let len: number = n.length;
                }
                let len2: number = s.length;
                return len2;
            }
        "#;
        assert!(check_src(src).is_ok(), "got {:?}", check_src(src));
    }

    #[test]
    fn same_scope_let_is_still_transfer() {
        // Same-scope `let n = s` is a transfer (current behavior); a
        // subsequent transfer of s errors. This pins the rule edge.
        let src = r#"
            let s: string = "x";
            let n: string = s;
            let m: string = s;
        "#;
        let r = check_src(src);
        assert!(
            r.as_ref()
                .err()
                .map(|s| s.contains("cannot transfer"))
                .unwrap_or(false),
            "expected re-transfer error, got {r:?}"
        );
    }

    #[test]
    fn line_comment_skipped() {
        let src = r#"
            // a comment at top
            let n: number = 5;  // a trailing comment
            // another full-line comment
            let m: number = n + 1;
        "#;
        assert!(check_src(src).is_ok(), "got {:?}", check_src(src));
    }

    #[test]
    fn block_comment_skipped() {
        let src = r#"
            /* leading block */
            let n: number = 5;
            /* multi
             * line
             * block */
            let m: number = n + 1;
            let s: string = "hi"; /* trailing on same line */
        "#;
        assert!(check_src(src).is_ok(), "got {:?}", check_src(src));
    }

    #[test]
    fn unterminated_block_comment_errors() {
        let src = r#"let n: number = 5; /* unterminated"#;
        let r = check_src(src);
        assert!(
            r.as_ref()
                .err()
                .map(|s| s.contains("unterminated block comment"))
                .unwrap_or(false),
            "expected unterminated-block-comment error, got {r:?}"
        );
    }

    #[test]
    fn slash_as_division_still_works() {
        // `/` not followed by `/` or `*` is still the division operator.
        let src = "let n: number = 10 / 2;";
        assert!(check_src(src).is_ok(), "got {:?}", check_src(src));
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
