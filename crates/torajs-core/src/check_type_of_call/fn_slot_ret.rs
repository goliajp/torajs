//! The type of a call through a fn-typed slot whose stored function
//! declares its parameters otherwise than the slot does
//! (`const h: (s: any) => number = (s: string) => s.length`).
//!
//! Such a call rides the boxed entry, whose adapter hands an argument
//! that does not fit the stored function's own parameter type to its
//! any twin, and the twin answers what its body computes from that
//! argument: `h(5)` is `undefined`, not a number. Typing the call by
//! the slot's declared return would make the caller convert the
//! answer (`undefined` became `NaN`, then `0` in an integer slot),
//! which nothing in the spec asks for. So the call types `any` unless
//! every function the slot can hold still returns the declared type:
//! a function whose parameters the slot's match answers its own
//! checked return, any other one goes through the static return sniff
//! the any-widen lane uses ([`super::any_widen_ret`]) with its
//! parameters read at the slot's types. A store the sniff cannot see
//! (a function value of unknown origin, a capture, a function the
//! enclosing one also hands out as a value) leaves the call `any`.
//!
//! The slot is the binding the callee resolves to, as the scope lookup
//! marks it ([`crate::check::FnSlot`]); a name the scopes do not hold
//! resolves to a top-level declaration. A property or return slot of
//! the census (`ast::face_slots`) is read the same way ([`prop_ret`]).
//! A call typed `any` here is recorded in `fn_slot_ret_any`, which
//! lowering reads by `ExprId` (`ssa_lower_call_closure_local::slot_ret`).

use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, Expr, ExprId, Param, Stmt};
use crate::check::{Checker, FnSlot, Type};
use crate::check_type_ann::resolve_type_ann_full;

mod prop_ret;

/// What a census slot holds: the binding the callee resolves to, or the
/// stores of a property or return slot, already collected.
enum Held<'a> {
    Let(&'a str, ExprId),
    Param(&'a str, String),
    Stores(Option<Vec<(String, ExprId)>>),
}

/// The type of call `eid` through `callee`, whose type declares
/// return `ret`.
pub(super) fn call_ret_face(
    checker: &mut Checker,
    ast: &Ast,
    eid: ExprId,
    callee: ExprId,
    ret: Type,
) -> Type {
    if checker.widen_ret_declared
        || matches!(ret, Type::Void | Type::Never | Type::Undefined | Type::Any)
    {
        return ret;
    }
    let (key, held) = match ast.get_expr(callee) {
        Expr::Ident(n) => {
            let slot = match checker.lookup(n) {
                Some(info) => info.fn_slot,
                // a named body reaches a top-level binding as a global; a
                // declaration that took its value's face is a census slot
                // there too
                None => top_level_decl(ast, n).filter(|s| {
                    ast.fnsig_mismatch_bindings.contains(n)
                        || matches!(s, FnSlot::Let(init) if checker.let_face_anns.contains_key(init)
                            || checker.face_mismatch_lets.contains(init))
                }),
            };
            match slot {
                Some(FnSlot::Let(init)) => (format!("let:{}", init.0), Held::Let(n, init)),
                Some(FnSlot::Param(owner)) => (format!("param:{owner}:{n}"), Held::Param(n, owner)),
                None => return ret,
            }
        }
        // a property or return slot in the census
        _ => match prop_ret::slot(checker, ast, callee) {
            Some((key, stores)) => (key, Held::Stores(stores)),
            None => return ret,
        },
    };
    let kept = match checker.fn_slot_kept.get(&key) {
        Some(k) => *k,
        None => {
            let stores = match held {
                Held::Let(n, init) => let_stores(ast, n, init),
                Held::Param(n, owner) => param_stores(ast, &owner, n),
                Held::Stores(stores) => stores,
            };
            let k = stores.is_some_and(|stores| {
                !stores.is_empty()
                    && stores
                        .iter()
                        .all(|(slot, e)| returns_declared(checker, ast, slot, *e, &ret))
            });
            checker.fn_slot_kept.insert(key, k);
            k
        }
    };
    if kept {
        return ret;
    }
    checker.widen_ret_any.insert(eid);
    checker.fn_slot_ret_any.insert(eid);
    Type::Any
}

/// The top-level fn-typed declaration of `n`, if there is one.
fn top_level_decl(ast: &Ast, n: &str) -> Option<FnSlot> {
    ast.stmts.iter().find_map(|s| match s {
        Stmt::LetDecl {
            name,
            type_ann: Some(ann),
            init,
            ..
        } if name == n && crate::ast::is_fn_like_ann(ann) => Some(FnSlot::Let(*init)),
        _ => None,
    })
}

/// The slot's annotation paired with every value stored into the
/// binding `n` declared with initializer `init`: the initializer, and
/// for a mutable binding every assignment to the name.
fn let_stores(ast: &Ast, n: &str, init: ExprId) -> Option<Vec<(String, ExprId)>> {
    let (ann, mutable) = find_decl(&ast.stmts, init)?;
    let mut out = vec![(ann.to_string(), init)];
    if mutable {
        out.extend(assign_stores(ast, n).map(|e| (ann.to_string(), e)));
    }
    Some(out)
}

fn find_decl(stmts: &[Stmt], at: ExprId) -> Option<(&str, bool)> {
    let mut found = None;
    for s in stmts {
        if let Stmt::LetDecl {
            type_ann: Some(ann),
            init,
            mutable,
            ..
        } = s
            && *init == at
        {
            return Some((ann.as_str(), *mutable));
        }
        crate::ast::stmt_nested_lists::for_each_nested_list(s, &mut |inner| {
            if found.is_none() {
                found = find_decl(inner, at);
            }
        });
        if found.is_some() {
            return found;
        }
    }
    found
}

fn assign_stores<'a>(ast: &'a Ast, n: &'a str) -> impl Iterator<Item = ExprId> + 'a {
    ast.exprs.iter().filter_map(move |e| match e {
        Expr::Assign { target, value }
            if matches!(ast.get_expr(*target), Expr::Ident(t) if t == n) =>
        {
            Some(*value)
        }
        _ => None,
    })
}

/// The slot's annotation paired with every argument a direct call of
/// `f` passes for its parameter `n`, plus every assignment to the
/// name. `None` when `f` is also read as a value (its calls are then
/// not all in sight).
fn param_stores(ast: &Ast, f: &str, n: &str) -> Option<Vec<(String, ExprId)>> {
    let params = ast.stmts.iter().find_map(|s| match s {
        Stmt::FnDecl { name, params, .. } if name == f => Some(params.as_slice()),
        _ => None,
    })?;
    let user: Vec<&Param> = params
        .iter()
        .filter(|p| !p.name.starts_with("__"))
        .collect();
    let i = user.iter().position(|p| p.name == n)?;
    let ann = user[i].type_ann.as_deref()?;
    let mut callees: HashSet<ExprId> = HashSet::new();
    let mut out: Vec<(String, ExprId)> = Vec::new();
    for e in &ast.exprs {
        if let Expr::Call { callee, args } = e
            && matches!(ast.get_expr(*callee), Expr::Ident(g) if g == f)
        {
            callees.insert(*callee);
            out.extend(args.get(i).map(|a| (ann.to_string(), *a)));
        }
    }
    let escapes = ast.exprs.iter().enumerate().any(|(k, e)| {
        matches!(e, Expr::Ident(g) if g == f) && !callees.contains(&ExprId(k as u32))
    });
    if escapes {
        return None;
    }
    out.extend(assign_stores(ast, n).map(|e| (ann.to_string(), e)));
    Some(out)
}

/// Does the function stored by `store` still return `ret` when its
/// parameters are read with the types the slot `ann` declares?
fn returns_declared(checker: &Checker, ast: &Ast, ann: &str, store: ExprId, ret: &Type) -> bool {
    let name = match ast.get_expr(store) {
        Expr::Closure { fn_name, .. } => fn_name.as_str(),
        Expr::Ident(g) => g.as_str(),
        _ => return false,
    };
    if ast.async_fns.contains(name) || ast.async_generator_fns.contains(name) {
        return false;
    }
    let Some((params, body)) = ast.stmts.iter().find_map(|s| match s {
        Stmt::FnDecl {
            name: g,
            params,
            body,
            is_generator: false,
            ..
        } if g == name => Some((params, body)),
        _ => None,
    }) else {
        return false;
    };
    let Some(canon) = crate::num_width::fn_type_canon(ann) else {
        return false;
    };
    let Some((slot_ps, _)) = crate::num_width::split_fn_type(&canon) else {
        return false;
    };
    let slot_ps: Vec<&str> = slot_ps.into_iter().filter(|p| !p.is_empty()).collect();
    let user: Vec<&str> = params
        .iter()
        .filter(|p| !p.name.starts_with("__"))
        .map(|p| p.type_ann.as_deref().map_or("any", str::trim))
        .collect();
    // every argument fits the function's own parameters (the slot is
    // boxed for its argument vector, not for a type), so the call
    // answers what the function itself is typed to return
    if user.len() <= slot_ps.len() && user.iter().zip(&slot_ps).all(|(u, s)| u == s) {
        return matches!(checker.globals.get(name), Some(Type::Function(_, r)) if **r == *ret);
    }
    let mut slot_ps = slot_ps.into_iter();
    let mut params = params.clone();
    for p in params.iter_mut().filter(|p| !p.name.starts_with("__")) {
        // a parameter the slot does not fill is `undefined`, which
        // only `any` reads as itself
        p.type_ann = Some(slot_ps.next().unwrap_or("any").to_string());
    }
    crate::ast::infer_return_ann(&ast.exprs, body, &params, &HashMap::new())
        .and_then(|ann| {
            resolve_type_ann_full(&ann, &checker.aliases, &[], &checker.generic_alias_decls)
        })
        .is_some_and(|t| t == *ret)
}
