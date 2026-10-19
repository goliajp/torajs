//! Shape helpers of the census: name conventions of the class
//! desugaring, type predicates, and statement walks.

use crate::ast::{Ast, Expr, ExprId, Param, Stmt};
use crate::check::Type;

use super::{PrefixWrite, THIS};

/// `__cm_<C>__ctor` (the one constructor the factory enters; a `$$`
/// twin is a separate write site the general census sees).
pub(super) fn ctor_class(name: &str) -> Option<&str> {
    name.strip_prefix("__cm_")?.strip_suffix("__ctor")
}

/// The leading `this.f = v` writes of constructor `name` of `class`
/// that reach an instance nothing else sees yet
/// (`ast::fresh_this`), with their statements.
pub(super) fn ctor_prefix(
    ast: &Ast,
    (name, class): (&str, &str),
    params: &[Param],
    body: &[Stmt],
) -> Vec<(ExprId, PrefixWrite)> {
    let fresh = crate::ast::fresh_this::fresh_this_writes(ast, name, params, body);
    let user = user_params(params);
    let mut out = Vec::new();
    for s in body {
        let Stmt::Expr(e) = s else { continue };
        let Expr::Assign { target, value } = ast.get_expr(*e) else {
            continue;
        };
        let Expr::Member { obj, name } = ast.get_expr(*target) else {
            continue;
        };
        if !fresh.recv.contains(obj) {
            continue;
        }
        let param = match ast.get_expr(*value) {
            Expr::Ident(n) => user.iter().position(|p| p.name == *n),
            _ => None,
        };
        let (class, field, value) = (class.to_string(), name.clone(), *value);
        out.push((
            *e,
            PrefixWrite {
                class,
                field,
                value,
                param,
            },
        ));
    }
    out
}

/// The parameters a `new C(..)` fills, in order: the receiver and the
/// desugaring's hidden parameters come first and are left out.
pub(super) fn user_params(params: &[Param]) -> Vec<&Param> {
    params
        .iter()
        .filter(|p| p.name != THIS && !p.name.starts_with("__"))
        .collect()
}

/// `__new_<C>`, the factory a `new C(..)` was rewritten to call.
pub(super) fn factory_class(name: &str) -> Option<&str> {
    let rest = name.strip_prefix("__new_")?;
    (rest != "target").then(|| rest.split(['<', '$']).next().unwrap_or(rest))
}

pub(super) fn pointer_shaped(t: &Type) -> bool {
    !matches!(t, Type::Nullable(_) | Type::Union(_))
        && crate::ssa_lower_nullable_guard::spells_undef_with_generic_cell(t)
}

pub(super) fn key_is_numeric(t: Option<&Type>) -> bool {
    matches!(
        t,
        Some(Type::Number | Type::Boolean | Type::BigInt | Type::Null | Type::Undefined)
    )
}

/// A value of this type is never a class instance.
pub(super) fn never_an_instance(t: Option<&Type>) -> bool {
    matches!(
        t,
        Some(
            Type::Array(_)
                | Type::String
                | Type::Number
                | Type::Boolean
                | Type::BigInt
                | Type::Symbol
                | Type::Null
                | Type::Undefined
        )
    )
}

pub(super) fn derives_from(ast: &Ast, class: &str, ancestor: &str) -> bool {
    let mut cur = class;
    for _ in 0..ast.class_parents.len() + 1 {
        match ast.class_parents.get(cur) {
            Some(Some(parent)) if parent == ancestor => return true,
            Some(Some(parent)) => cur = parent,
            _ => return false,
        }
    }
    false
}

/// The literal `__new_<class>` seeds `__this` with, when the factory
/// hands it straight to `__cm_<class>__ctor`: nothing between the two
/// can read a seeded field.
pub(super) fn ctor_fed_seed(ast: &Ast, class: &str, body: &[Stmt]) -> Option<ExprId> {
    let at = body
        .iter()
        .position(|s| matches!(s, Stmt::LetDecl { name, .. } if name == THIS))?;
    let (Stmt::LetDecl { init, .. }, Some(Stmt::Expr(call))) = (&body[at], body.get(at + 1)) else {
        return None;
    };
    let Expr::Call { callee, .. } = ast.get_expr(*call) else {
        return None;
    };
    let ctor = format!("__cm_{class}__ctor");
    (matches!(ast.get_expr(*callee), Expr::Ident(n) if *n == ctor)
        && matches!(ast.get_expr(*init), Expr::ObjectLit { .. }))
    .then_some(*init)
}

pub(super) fn collect_super_args(ast: &Ast, e: ExprId, f: &mut impl FnMut(&[ExprId])) {
    if let Expr::Super { args } = ast.get_expr(e) {
        f(args);
    }
    crate::ast::desugar_async_sm_walk::visit_expr_children(ast, e, &mut |c| {
        collect_super_args(ast, c, f)
    });
}

/// The `return` statements of a body, not descending into nested
/// function declarations (closure bodies are expressions and are not
/// statements of this body).
pub(in crate::ssa_lower_pass_0_5) fn own_returns(
    body: &[Stmt],
    f: &mut impl FnMut(Option<ExprId>),
) {
    for s in body {
        if let Stmt::Return(v) = s {
            f(*v);
        }
        if !matches!(s, Stmt::FnDecl { .. }) {
            nested_stmts(s, &mut |st| own_returns(std::slice::from_ref(st), f));
        }
    }
}

/// The statements directly nested in `s` (one level), for every body
/// shape.
pub(in crate::ssa_lower_pass_0_5) fn nested_stmts<'a>(s: &'a Stmt, f: &mut impl FnMut(&'a Stmt)) {
    match s {
        Stmt::FnDecl { .. } => {}
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            body.iter().for_each(&mut *f);
            catch_body.iter().for_each(&mut *f);
            if let Some(fin) = finally_body {
                fin.iter().for_each(&mut *f);
            }
        }
        Stmt::ForOf { body, .. }
        | Stmt::ForOfSplitIter { body, .. }
        | Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::Labeled { body, .. } => f(body),
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                f(i);
            }
            f(body);
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            f(then_branch);
            if let Some(e) = else_branch {
                f(e);
            }
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                c.body.iter().for_each(&mut *f);
            }
            if let Some(d) = default {
                d.iter().for_each(&mut *f);
            }
        }
        Stmt::Block(v) | Stmt::Multi(v) => v.iter().for_each(&mut *f),
        _ => {}
    }
}
