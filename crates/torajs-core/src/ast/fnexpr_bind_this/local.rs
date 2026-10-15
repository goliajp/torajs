//! The bind-only receiver of the parent module, for a binding
//! declared inside a function body.
//!
//! The parent proves a TOP-LEVEL binding's uses by name with a
//! shadow-aware walk. A function-local name gets a proof both steps
//! can re-run without carrying state across the lift: every `Ident`
//! spelling the name, program-wide, stands in `.bind` receiver
//! position, and every declaration of it is a `let` / `const` / `var`
//! inside a function body — no parameter, function, class, catch or
//! loop binding anywhere spells it. Then whichever declaration a use
//! resolves to, it is one of these, so step 2 promotes all of them or
//! none (a class method body is cloned, and its clone declares the
//! name a second time): each initializer must be a function expression
//! reading `this`, which gets its `__this: any` parameter before the
//! lift and is recorded in `Ast::fnexpr_bind_local_inits`. Step 3
//! registers the lifted closure in each recorded slot, so the two
//! steps cannot disagree on which functions took the parameter.
//! Anything else (a direct call next to the bind, a parameter of the
//! same name) keeps today's refusal.

use std::collections::{HashMap, HashSet};

use crate::ast::desugar_with::walk::stmt_children_ref;
use crate::ast::{Ast, Expr, ExprId, Stmt};

/// Step 2 — the function-expression inits of qualifying local
/// bindings (pre-lift: each init is still the `ArrowFn`), recorded for
/// step 3.
pub(super) fn local_bind_receiver_inits(ast: &mut Ast) -> Vec<ExprId> {
    let mut out = Vec::new();
    for inits in qualifying(ast).into_values() {
        if inits.iter().all(|&i| reads_own_this(ast, i)) {
            out.extend(inits);
        }
    }
    ast.fnexpr_bind_local_inits.extend(out.iter().copied());
    out
}

/// Step 3 — the lifted closure in every slot step 2 promoted (the lift
/// replaces the `ArrowFn` in place, so the id still names it).
pub(super) fn local_bind_receiver_closures(ast: &Ast) -> Vec<String> {
    ast.fnexpr_bind_local_inits
        .iter()
        .map(|&init| match ast.get_expr(init) {
            Expr::Closure { fn_name, .. } => fn_name.clone(),
            // step 2 records only `ArrowFn` slots and `lift_arrow_fns`
            // turns every arena `ArrowFn` into a `Closure`; a slot that is
            // neither would leave a `__this` parameter the bind kernel
            // never routes into
            other => unreachable!("promoted bind-receiver init not lifted: {other:?}"),
        })
        .collect()
}

/// Names declared only by function-local `let`s and used only as
/// `.bind` receivers, with the inits of their declarations.
fn qualifying(ast: &Ast) -> HashMap<&str, Vec<ExprId>> {
    let decls = Decls::of(ast);
    if decls.unwalked {
        return HashMap::new();
    }
    let top: HashSet<&str> = crate::ast::toplevel_stmts_flat(ast)
        .into_iter()
        .filter_map(|s| match s {
            Stmt::LetDecl { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    let receivers = bind_receiver_names(ast);
    decls
        .lets
        .into_iter()
        .filter(|(name, _)| {
            !top.contains(name) && !decls.others.contains(name) && receivers.contains(name)
        })
        .collect()
}

/// A function expression whose body reads its own `this` and that can
/// take the leading `__this` parameter.
fn reads_own_this(ast: &Ast, init: ExprId) -> bool {
    if !ast.fn_expr_exprs.contains(&init) {
        return false;
    }
    let Expr::ArrowFn { params, body, .. } = ast.get_expr(init) else {
        return false;
    };
    let names: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
    !params.iter().any(|p| p.is_rest || p.name == "__this")
        && crate::ast::free_vars::free_vars_of_body(ast, &names, body)
            .iter()
            .any(|v| v == "__this")
}

/// Names every `Ident` of which is the object of a `.bind`.
fn bind_receiver_names(ast: &Ast) -> HashSet<&str> {
    let bind_objs: HashSet<ExprId> = ast
        .exprs
        .iter()
        .filter_map(|e| match e {
            Expr::Member { obj, name } if name == "bind" => Some(*obj),
            _ => None,
        })
        .collect();
    let mut seen: HashMap<&str, bool> = HashMap::new();
    for (i, e) in ast.exprs.iter().enumerate() {
        if let Expr::Ident(n) = e {
            let ok = bind_objs.contains(&ExprId(i as u32));
            *seen.entry(n.as_str()).or_insert(true) &= ok;
        }
    }
    seen.into_iter()
        .filter(|(_, ok)| *ok)
        .map(|(n, _)| n)
        .collect()
}

/// Every declaration in the program, by name.
struct Decls<'a> {
    /// `let` / `const` / `var`: the init of each.
    lets: HashMap<&'a str, Vec<ExprId>>,
    /// Any other declarator: parameters, functions, classes, catch
    /// and loop bindings.
    others: HashSet<&'a str>,
    /// A class still undesugared has bodies this walk does not enter;
    /// no local binding is proved then.
    unwalked: bool,
}

impl<'a> Decls<'a> {
    fn of(ast: &'a Ast) -> Self {
        let mut d = Decls {
            lets: HashMap::new(),
            others: HashSet::new(),
            unwalked: false,
        };
        let mut todo: Vec<&'a Stmt> = ast.stmts.iter().collect();
        for e in &ast.exprs {
            if let Expr::ArrowFn { params, body, .. } = e {
                d.others.extend(params.iter().map(|p| p.name.as_str()));
                todo.extend(body.iter());
            }
        }
        while let Some(s) = todo.pop() {
            match s {
                Stmt::LetDecl { name, init, .. } => {
                    d.lets.entry(name.as_str()).or_default().push(*init);
                }
                Stmt::FnDecl {
                    name, params, body, ..
                } => {
                    d.others.insert(name.as_str());
                    d.others.extend(params.iter().map(|p| p.name.as_str()));
                    todo.extend(body.iter());
                }
                Stmt::ClassDecl { .. } => d.unwalked = true,
                Stmt::Try {
                    catch_param: Some(c),
                    ..
                } => {
                    d.others.insert(c.as_str());
                }
                Stmt::ForOf { var_name, .. } | Stmt::ForOfSplitIter { var_name, .. } => {
                    d.others.insert(var_name.as_str());
                }
                _ => {}
            }
            todo.extend(stmt_children_ref(s));
        }
        d
    }
}
