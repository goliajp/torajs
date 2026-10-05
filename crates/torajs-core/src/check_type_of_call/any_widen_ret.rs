//! The type of a call the any-widen lane retargets to a clone.
//!
//! The clone binds each widened argument as passed (§10.2.1.3), so a
//! body that hands a widened param back answers that value: `null`,
//! `undefined`, a number. Typing the call by the declared return would
//! make the clone convert it on the way out (ToString for a `string`
//! return), which no part of the spec asks for. So the call types
//! `any` and the clone answers `any` — unless the return does not
//! depend on the widened params at all, which the same static return
//! sniff the desugar pass uses settles: re-run it with the widened
//! annotations, and a return it still names as the declared type
//! keeps that type.
//!
//! The sniff gets no callee signatures: `function wrap(s: string) {
//! return id(s) }` must not read `id`'s typed return, since `id`
//! called from the clone is retargeted the same way.

use std::collections::HashMap;

use crate::ast::{Ast, ExprId, Stmt};
use crate::check::{Checker, Type};
use crate::check_type_ann::resolve_type_ann_full;

/// The type of call `eid`, whose callee declares return `ret`.
pub(super) fn call_ret_face(checker: &mut Checker, ast: &Ast, eid: ExprId, ret: Type) -> Type {
    if checker.widen_ret_declared
        || matches!(ret, Type::Void | Type::Never | Type::Undefined | Type::Any)
    {
        return ret;
    }
    let Some((name, idxs)) = checker.any_widen_mono_sites.get(&eid) else {
        return ret;
    };
    // the call runs the generator's twin, whose iterator class is its own
    if let Some(twin) = crate::ast::minted_generator_any_twin(ast, name)
        && let Some(Type::Function(_, twin_ret)) = checker.globals.get(&twin)
    {
        return (**twin_ret).clone();
    }
    if ast.async_fns.contains(name) || ast.async_generator_fns.contains(name) {
        return ret;
    }
    let Some((params, body)) = ast.stmts.iter().find_map(|s| match s {
        Stmt::FnDecl {
            name: n,
            params,
            body,
            ..
        } if n == name => Some((params, body)),
        _ => None,
    }) else {
        return ret;
    };
    // a lifted body's `__env` head is not a user param
    let head = usize::from(params.first().is_some_and(|p| p.name == "__env"));
    let mut params = params.clone();
    for &(i, target) in idxs {
        if let Some(p) = params.get_mut(i + head) {
            p.type_ann = Some(target.ann().to_string());
        }
    }
    let kept = crate::ast::infer_return_ann(&ast.exprs, body, &params, &HashMap::new())
        .and_then(|ann| {
            resolve_type_ann_full(&ann, &checker.aliases, &[], &checker.generic_alias_decls)
        })
        .is_some_and(|t| t == ret);
    if kept {
        return ret;
    }
    checker.widen_ret_any.insert(eid);
    Type::Any
}
