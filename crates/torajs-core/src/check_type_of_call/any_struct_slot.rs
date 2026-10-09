//! An `any` argument into an object-typed parameter of a function
//! value: `const f: (p: P) => string = show; f(a)`, a callback
//! parameter `cb(a)`, a local closure `g(a)`.
//!
//! §10.2.1.3 binds the argument as passed and §13.3.2.1 makes `p.x` a
//! [[Get]] on whatever `p` holds, so the call has a run-time meaning
//! for every value `a` can hold; refusing it narrows the language. A
//! direct call to a declared function takes the clone with that
//! parameter `any` ([`super::any_widen`]), and so does a call through
//! a `const` binding of one: the binding can only hold that function.
//!
//! Any other function value has no single declaration to clone. A
//! closure-repr value (a lifted closure, a declared function behind its
//! forwarder shell) carries a boxed entry, the lane every call through
//! `any` takes: the adapter tests each object slot
//! (`__torajs_anyv_struct_fits`), runs the typed body when every
//! argument is an instance the body can read and the value's all-`any`
//! twin otherwise (`check_monomorph_any_widen::struct_escape`). The
//! callee name is recorded so the closures it may hold get that twin.
//! The twin answers what its body computes from the argument as
//! passed, so the call types `any`, as a retargeted call does
//! ([`super::any_widen_ret`]).
//!
//! A binding that keeps the bare function pointer (`__fn(` repr: a
//! parameter or `let` only ever handed declared functions) has neither
//! env nor boxed entry, and stays refused.

use crate::ast::{Ast, Expr, ExprId, Stmt};
use crate::check::{Checker, Type, WidenTarget};

/// How an admitted call reaches its callee.
pub(super) enum Route {
    /// Retargeted to the clone of the one declared function the
    /// binding holds.
    Clone,
    /// Through the value's boxed entry.
    Boxed,
}

/// May call `eid` hand `any` argument `arg_ty` to the object-typed
/// parameter `i`, `param_ty`, of the function value `callee` names?
/// Records the clone site or the boxed callee when it may.
pub(super) fn admit(
    checker: &mut Checker,
    ast: &Ast,
    eid: ExprId,
    callee: ExprId,
    i: usize,
    arg_ty: &Type,
    param_ty: &Type,
) -> Option<Route> {
    if !matches!(arg_ty, Type::Any) || !matches!(param_ty, Type::ClassRef(_) | Type::Struct(_)) {
        return None;
    }
    let Expr::Ident(n) = ast.get_expr(callee) else {
        return None;
    };
    let face = fn_value_face(checker, ast, n)?;
    let decls = binding_decls(ast, n);
    if let [Decl::ConstOf(f)] = decls.as_slice()
        && let Some(target) = super::any_widen::clonable_callee(checker, ast, f, i)
    {
        super::any_widen::record_widen_site(checker, eid, &target, i, WidenTarget::Scalar);
        return Some(Route::Clone);
    }
    if decls.is_empty() || !decls.iter().all(|d| matches!(d, Decl::Closure)) {
        return None;
    }
    checker.any_struct_arg_callees.insert(n.clone(), face);
    Some(Route::Boxed)
}

/// The parameter types of the function value `n` names — a parameter
/// or a `let` / `const` — or None for a declared function, whose
/// direct call the clone lane serves or refuses on its own terms.
fn fn_value_face(checker: &Checker, ast: &Ast, n: &str) -> Option<Vec<Type>> {
    let ty = checker
        .lookup(n)
        .map(|info| info.ty)
        .or_else(|| checker.globals.get(n).cloned());
    match ty {
        Some(Type::Function(params, _)) if !declared_fn(ast, n) => Some(params),
        _ => None,
    }
}

fn declared_fn(ast: &Ast, n: &str) -> bool {
    ast.stmts
        .iter()
        .any(|s| matches!(s, Stmt::FnDecl { name, .. } if name == n))
}

/// A declaration of a binding name, by the representation its value
/// lowers to.
enum Decl {
    /// `const n = f` / `const n: (…) => R = f` over declared function `f`.
    ConstOf(String),
    /// A closure cell (`__cls(` / rest-tail annotation, a lifted
    /// closure initializer).
    Closure,
    /// A bare function pointer, or a shape this lane does not read.
    Raw,
}

/// Every declaration of `n` in the program, parameters included. Name
/// keyed, so a same-named binding elsewhere only makes the answer more
/// conservative.
fn binding_decls(ast: &Ast, n: &str) -> Vec<Decl> {
    // a program-level binding with a function annotation is a closure
    // global: every store into it takes the cell
    let program_level: Vec<ExprId> = ast
        .stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::LetDecl { name, init, .. } if name == n => Some(*init),
            _ => None,
        })
        .collect();
    let mut out = Vec::new();
    crate::ast::walk_stmts(&ast.stmts, &mut |s| match s {
        Stmt::LetDecl {
            name,
            type_ann,
            init,
            mutable,
            ..
        } if name == n => {
            let top = program_level.contains(init);
            out.push(let_decl(ast, type_ann.as_deref(), *init, *mutable, top));
        }
        Stmt::FnDecl { name, params, .. } => {
            if name == n {
                out.push(Decl::Raw);
            }
            out.extend(
                params
                    .iter()
                    .filter(|p| p.name == n)
                    .map(|p| ann_decl(p.type_ann.as_deref())),
            )
        }
        // any other binding form spelled `n` holds what this lane does
        // not read, and its scope may be the one the call resolves to
        Stmt::ForOf { var_name, .. } | Stmt::ForOfSplitIter { var_name, .. } if var_name == n => {
            out.push(Decl::Raw)
        }
        Stmt::UsingDecl { name, .. } if name == n => out.push(Decl::Raw),
        Stmt::Try {
            catch_param: Some(c),
            ..
        } if c == n => out.push(Decl::Raw),
        _ => {}
    });
    out
}

fn let_decl(ast: &Ast, ann: Option<&str>, init: ExprId, mutable: bool, top: bool) -> Decl {
    match (ast.get_expr(init), ann) {
        (Expr::Ident(f), _) if !mutable && declared_fn(ast, f) => Decl::ConstOf(f.clone()),
        (Expr::Closure { .. }, None) => Decl::Closure,
        (_, Some(a)) if top && crate::ast::is_fn_like_ann(a) => Decl::Closure,
        (Expr::Call { callee, .. }, None) => match ast.get_expr(*callee) {
            Expr::Ident(g) => returned_decl(ast, g),
            _ => Decl::Raw,
        },
        _ => ann_decl(ann),
    }
}

/// The representation of what declared function `g` returns.
fn returned_decl(ast: &Ast, g: &str) -> Decl {
    ast.stmts
        .iter()
        .find_map(|s| match s {
            Stmt::FnDecl {
                name, return_type, ..
            } if name == g => Some(ann_decl(return_type.as_deref())),
            _ => None,
        })
        .unwrap_or(Decl::Raw)
}

fn ann_decl(ann: Option<&str>) -> Decl {
    match ann.map(str::trim) {
        Some(a) if a.starts_with("__cls(") || (a.starts_with("__fn(") && a.contains("__rest(")) => {
            Decl::Closure
        }
        _ => Decl::Raw,
    }
}

/// The type of call `eid`, whose callee declares return `ret`, once
/// [`admit`] routed one of its arguments through the boxed entry.
pub(super) fn call_ret_face(checker: &mut Checker, eid: ExprId, ret: Type) -> Type {
    if checker.widen_ret_declared
        || matches!(ret, Type::Void | Type::Never | Type::Undefined | Type::Any)
    {
        return ret;
    }
    checker.widen_ret_any.insert(eid);
    checker.fn_slot_ret_any.insert(eid);
    Type::Any
}
