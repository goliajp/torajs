//! The any-widen wedge family shared by
//! [`super::general::arg_admitted`] — the r380 heap-typed half of TS
//! any-assignability at the call boundary, the r381 `as`-cast peel in
//! front of it, and the site recorder + clonability gate both the
//! `Array(Any)` and the plain-`Any` wedges write through.
//!
//! Split out of `general.rs` (r381): the `as`-cast peel pushed that
//! file past the 500-line limit, and this family is the one part of it
//! that answers a single question — may this call site be retargeted
//! to a callee clone with param `i` widened?

use crate::ast::{Ast, ExprId};
use crate::check::{Checker, Type, WidenTarget};

/// r380 — the heap-typed half of TS any-assignability at the call
/// boundary (`const a: any = fn; takes(a)` against
/// `takes(f: () => void)`). A caller-side unbox is the wrong shape
/// here: an `Any` slot holding a Function / Struct / Map has no
/// scalar to coerce into, and taxing every typed reader with a kind
/// check costs the hot path. So the CALLEE moves instead — the same
/// monomorph channel the `Array(Any)` wedge rides, with the param
/// widened to plain `any` so the clone's body re-checks and re-lowers
/// through the any-tier lanes (probe: Function / Array / Struct /
/// Map / Set / class instance all answer bun-equal once the param is
/// spelled `any` by hand, and a non-callable behind `any` still
/// throws where bun throws). Typed callers keep the original decl —
/// zero hot-path cost.
///
/// The excluded param types are the ones that must NOT reach here:
/// scalars ride the caller-side coerce above, `Any` needs nothing,
/// and `Rest` / `TypeVar` name shapes the clone lane cannot serve
/// (`widenable_fn_decl` rejects rest params and generic decls
/// anyway — the match keeps the intent readable at the gate).
#[allow(clippy::too_many_arguments)]
pub(super) fn any_into_heap_param(
    checker: &mut Checker,
    ast: &Ast,
    eid: ExprId,
    callee: &ExprId,
    arg_ty: &Type,
    param_ty: &Type,
    i: usize,
) -> bool {
    if !matches!(arg_ty, Type::Any)
        || matches!(
            param_ty,
            Type::Any
                | Type::Number
                | Type::String
                | Type::Boolean
                | Type::BigInt
                | Type::Void
                | Type::Rest(_)
                | Type::TypeVar(_)
        )
    {
        return false;
    }
    let crate::ast::Expr::Ident(n) = ast.get_expr(*callee) else {
        return false;
    };
    let Some(n) = clonable_callee(checker, ast, n, i) else {
        return false;
    };
    record_widen_site(checker, eid, &n, i, WidenTarget::Scalar);
    true
}

/// RFC 20260802 `Array(Any)` → typed `Array(T)` wedge, the container
/// twin of [`any_into_heap_param`]: `var seq = []; seq.push(1);
/// f(seq)` against `f(xs: number[])` is TS any-assignability. Same
/// monomorph backing — the site is recorded and the callee clone's
/// param is widened to `any[]`, so a typed reader never faces the
/// NaN-boxed block. Gated to the one callee shape the clone+retarget
/// lane handles; everything else stays loud.
#[allow(clippy::too_many_arguments)]
pub(super) fn arr_any_into_typed_arr_param(
    checker: &mut Checker,
    ast: &Ast,
    eid: ExprId,
    callee: &ExprId,
    arg_ty: &Type,
    param_ty: &Type,
    i: usize,
) -> bool {
    if !matches!(param_ty, Type::Array(el) if !matches!(**el, Type::Any))
        || !matches!(arg_ty, Type::Array(el) if matches!(**el, Type::Any))
    {
        return false;
    }
    let crate::ast::Expr::Ident(n) = ast.get_expr(*callee) else {
        return false;
    };
    let Some(n) = clonable_callee(checker, ast, n, i) else {
        return false;
    };
    record_widen_site(checker, eid, &n, i, WidenTarget::Arr);
    true
}

/// The type UNDER an `as` cast, or None when the argument is not one.
///
/// TS treats the assertion as erasing nothing at runtime, and neither
/// does tr: the operand `lower_as_cast` hands on for a non-scalar
/// annotation is the value itself. So the cast must not buy the
/// argument past a repr gate its uncast twin has to clear — both
/// widen wedges re-run against what is really underneath.
pub(super) fn as_cast_inner_ty(checker: &mut Checker, ast: &Ast, arg_id: &ExprId) -> Option<Type> {
    let crate::ast::Expr::As { expr, .. } = ast.get_expr(*arg_id) else {
        return None;
    };
    checker.type_of(ast, *expr).ok()
}

/// Record one param's widen plan on the call site both any-widen
/// wedges share. First writer per index wins: an index already
/// planned cannot be re-targeted, and the two wedges are mutually
/// exclusive on `arg_ty` anyway (`Array(Any)` vs plain `Any`).
pub(super) fn record_widen_site(
    checker: &mut Checker,
    eid: ExprId,
    callee_name: &str,
    i: usize,
    target: WidenTarget,
) {
    let name = callee_name.to_string();
    let entry = checker
        .any_widen_mono_sites
        .entry(eid)
        .or_insert_with(|| (name, Vec::new()));
    if !entry.1.iter().any(|(j, _)| *j == i) {
        entry.1.push((i, target));
    }
}

/// The FnDecl the any-widen lane would clone for a call through
/// `callee`. `None` when the lane cannot serve the callee: a generic,
/// a lifted body named directly, or a decl [`widenable_fn_decl`]
/// rejects.
///
/// 551-03 — a call through a top-level `const f = (…) => …` binding
/// names the BINDING, not the lifted decl behind it; the alias
/// resolves it (`lifted_closure_alias`) and the clone then carries
/// the `__env` head, which the clone lane serves through the same
/// path the hand-spelled `(x: any) => …` closure takes today.
fn clonable_callee(checker: &Checker, ast: &Ast, n: &str, i: usize) -> Option<String> {
    let (name, lifted) = match lifted_closure_alias(checker, ast, n) {
        Some(f) => (f, true),
        None if checker.closure_fn_names.contains(n) => return None,
        None => (n.to_string(), false),
    };
    let generic = !checker
        .generic_type_params
        .get(&name)
        .is_none_or(|tp| tp.is_empty());
    (!generic && widenable_fn_decl(ast, &name, i, lifted)).then_some(name)
}

/// 551-03 — the lifted decl behind a top-level `const` closure
/// binding, when the clone lane can stand in for it at this call.
/// The name must reach THAT binding: at top level, main's own frame
/// with no inner shadow; inside a body, either no local of the name
/// (the globals path) or a capture the checker marked as carrying
/// the top-level binding (`toplevel_captures`). The closure's own
/// captures must have resolved at its construction site (recorded
/// there, a promoted global, or a class sentinel) — the clone is
/// re-checked at that same level. And the body is a plain arrow: no
/// receiver channel, no argv face, no self-name, not async.
fn lifted_closure_alias(checker: &Checker, ast: &Ast, name: &str) -> Option<String> {
    let (fn_name, captures) = ast.stmts.iter().find_map(|s| match s {
        crate::ast::Stmt::LetDecl {
            mutable: false,
            is_var: false,
            name: n,
            init,
            ..
        } if n == name => match ast.get_expr(*init) {
            crate::ast::Expr::Closure { fn_name, captures } => {
                Some((fn_name.clone(), captures.clone()))
            }
            _ => None,
        },
        _ => None,
    })?;
    let in_fn = checker.expected_return.is_some();
    let frames_with = checker
        .scopes
        .iter()
        .filter(|s| s.contains_key(name))
        .count();
    let names_const = match (in_fn, frames_with) {
        (false, 1) => checker.scopes.first().is_some_and(|s| s.contains_key(name)),
        (true, 0) => true,
        (true, 1) => checker
            .toplevel_captures
            .last()
            .is_some_and(|s| s.contains(name)),
        _ => false,
    };
    if !names_const {
        return None;
    }
    let resolved = checker.closure_captures.get(&fn_name)?;
    let resolvable = captures.iter().all(|c| {
        resolved.iter().any(|(n, _)| n == c)
            || checker.globals.contains_key(c)
            || c.starts_with("__class_")
            || c.starts_with("__proto_")
    });
    let plain = !ast.fnexpr_recv_fns.contains(&fn_name)
        && !ast.closure_argv_fns.contains(&fn_name)
        && !ast.async_fns.contains(&fn_name)
        && !ast.async_generator_fns.contains(&fn_name)
        && !ast.closure_self_names.contains_key(&fn_name);
    (resolvable && plain).then_some(fn_name)
}

/// RFC 20260802 gate — can the any-widen lane clone this callee with
/// param `i` widened? Requires a top-level non-generic non-generator
/// FnDecl that declares param `i` (T-28 pad / arity wedges can shift
/// counts) as a non-rest slot. `lifted` says the decl is a closure
/// body: its `__env` head is not a user param, so user index `i` is
/// AST slot `i + 1` (the head is what `check_pipeline` skips when it
/// builds the closure's fn type). A plain decl must not carry the
/// head, a lifted one must.
pub(super) fn widenable_fn_decl(ast: &Ast, name: &str, i: usize, lifted: bool) -> bool {
    let slot = i + usize::from(lifted);
    ast.stmts.iter().any(|s| {
        matches!(s, crate::ast::Stmt::FnDecl { name: n, type_params, is_generator, params, .. }
            if n == name
                && type_params.is_empty()
                && !*is_generator
                && params.first().is_some_and(|p| p.name == "__env") == lifted
                // Only the slot being widened has to be non-rest: its
                // annotation is rewritten to a scalar `any` / `any[]`,
                // which is not what a rest slot means. The others ride
                // the clone unchanged, and that shape is one the lane
                // already serves — spelling the widened slot `any` by
                // hand next to a `...rest` param works today, which is
                // exactly what the clone produces.
                && params.get(slot).is_some_and(|p| !p.is_rest))
    })
}
