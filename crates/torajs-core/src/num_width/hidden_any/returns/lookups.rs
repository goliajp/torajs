//! What the return walk of `hidden_any::returns` looks up by name.

use std::collections::{HashMap, HashSet};

use crate::ast::{Expr, ExprId, Stmt};
use crate::check::Type;
use crate::num_width::Analysis;
use crate::num_width::hidden_any::Origin;

/// What the return walk looks up by name.
pub(in crate::num_width) struct Lookups {
    /// Every value stored to a name by an assignment.
    pub(super) stores: HashMap<String, Vec<ExprId>>,
    /// (function, parameter) → whether its annotation leaves its kind
    /// open: absent, or [`open_ann`].
    pub(super) open_params: HashMap<(String, String), bool>,
    /// Every declaration of a name, as a top-level read reaches it.
    pub(super) decls: HashMap<String, Vec<Origin>>,
    /// (struct or class, field) → its annotation.
    pub(super) fields: HashMap<(String, String), String>,
    /// A function's parameters the program writes, in order.
    pub(super) user_params: HashMap<String, Vec<String>>,
    /// The argument lists of the direct calls of each name.
    pub(super) call_args: HashMap<String, Vec<Vec<ExprId>>>,
    /// The names read other than as a callee.
    pub(super) fn_values: HashSet<String>,
    /// (function, parameter) → its annotation.
    pub(super) param_anns: HashMap<(String, String), String>,
    /// A binding's initializer → the binding's annotation.
    pub(super) let_anns: HashMap<ExprId, String>,
    /// A type alias with no type parameters → the type it names
    /// (`type AB = A | B`).
    pub(super) aliases: HashMap<String, String>,
    /// The object types with no type parameters (`type P = { x: number }`).
    pub(super) structs: HashSet<String>,
    /// A function's return annotation, whoever wrote it.
    rets: HashMap<String, String>,
    /// The functions whose return annotation the program wrote, not a
    /// desugar pass (which spells `any` for a return it cannot name).
    written: HashSet<String>,
}

impl Lookups {
    pub(super) fn build(a: &Analysis<'_>) -> Self {
        let mut stores: HashMap<String, Vec<ExprId>> = HashMap::new();
        let mut call_args: HashMap<String, Vec<Vec<ExprId>>> = HashMap::new();
        let mut callees = HashSet::new();
        for e in &a.ast.exprs {
            match e {
                Expr::Assign { target, value } => {
                    if let Expr::Ident(n) = a.ast.get_expr(*target) {
                        stores.entry(n.clone()).or_default().push(*value);
                    }
                }
                Expr::Call { callee, args } => {
                    callees.insert(*callee);
                    if let Expr::Ident(n) = a.ast.get_expr(*callee) {
                        call_args.entry(n.clone()).or_default().push(args.clone());
                    }
                }
                _ => {}
            }
        }
        let fn_values = (a.ast.exprs.iter().enumerate())
            .filter_map(|(i, e)| match e {
                Expr::Ident(n) if !callees.contains(&ExprId(i as u32)) => Some(n.clone()),
                _ => None,
            })
            .collect();
        let mut user_params = HashMap::new();
        let mut open_params = HashMap::new();
        let mut param_anns = HashMap::new();
        let mut let_anns = HashMap::new();
        let mut decls: HashMap<String, Vec<Origin>> = HashMap::new();
        let mut fields = HashMap::new();
        let mut aliases = HashMap::new();
        let mut structs = HashSet::new();
        let mut rets = HashMap::new();
        let mut written = HashSet::new();
        crate::ast::walk_stmts(&a.ast.stmts, &mut |s| match s {
            Stmt::TypeDecl {
                name,
                fields: fs,
                type_params,
            } => {
                match fs.as_slice() {
                    [(k, ann)] if k.lossy() == "__alias__" => {
                        if type_params.is_empty() {
                            aliases.insert(name.clone(), ann.clone());
                        }
                    }
                    _ if type_params.is_empty() => {
                        structs.insert(name.clone());
                    }
                    _ => {}
                }
                for (k, ann) in fs {
                    fields.insert((name.clone(), k.lossy().into_owned()), ann.clone());
                }
            }
            Stmt::FnDecl {
                name,
                params,
                return_type,
                span,
                ..
            } => {
                if let Some(r) = return_type {
                    rets.insert(name.clone(), r.clone());
                    if a.ast.fn_ret_written(name, *span) {
                        written.insert(name.clone());
                    }
                }
                let user = params.iter().filter(|p| !p.name.starts_with("__"));
                user_params.insert(name.clone(), user.map(|p| p.name.clone()).collect());
                for p in params {
                    // a parameter the compiler adds (`__this`, a captured
                    // class) holds what it is handed
                    let open =
                        !p.name.starts_with("__") && p.type_ann.as_deref().is_none_or(open_ann);
                    open_params.insert((name.clone(), p.name.clone()), open);
                    if let Some(t) = &p.type_ann {
                        param_anns.insert((name.clone(), p.name.clone()), t.clone());
                    }
                }
            }
            Stmt::LetDecl {
                name,
                type_ann,
                init,
                mutable,
                ..
            } => {
                if let Some(t) = type_ann {
                    let_anns.insert(*init, t.clone());
                }
                let origin = match type_ann {
                    None if !*mutable => Origin::Fixed(*init),
                    _ => Origin::Open(*init),
                };
                decls.entry(name.clone()).or_default().push(origin);
            }
            _ => {}
        });
        Lookups {
            stores,
            open_params,
            decls,
            fields,
            user_params,
            call_args,
            fn_values,
            param_anns,
            let_anns,
            aliases,
            structs,
            rets,
            written,
        }
    }

    /// The type function `f` declares it returns, when a round may
    /// retype it (`number`, `string`, `boolean`).
    pub(super) fn declared(&self, f: &str) -> Option<Type> {
        scalar(self.rets.get(f)?)
    }

    /// Function `f`'s return annotation, when the program wrote one.
    pub(super) fn written_ret(&self, f: &str) -> Option<&str> {
        let r = self.rets.get(f).filter(|_| self.written.contains(f))?;
        Some(r.as_str())
    }

    /// Does the program declare function `f` to return a value of a
    /// kind left open (`g(): any`)?
    pub(super) fn returns_open(&self, f: &str) -> bool {
        self.written_ret(f).is_some_and(open_ann)
    }
}

/// The scalar type an annotation spells (`number`, `string`,
/// `boolean`).
pub(super) fn scalar(ann: &str) -> Option<Type> {
    match ann.trim() {
        "number" => Some(Type::Number),
        "string" => Some(Type::String),
        "boolean" => Some(Type::Boolean),
        _ => None,
    }
}

/// An annotation that leaves the kind of the value open.
pub(super) fn open_ann(ann: &str) -> bool {
    matches!(ann.trim(), "any" | "unknown" | "object")
}
