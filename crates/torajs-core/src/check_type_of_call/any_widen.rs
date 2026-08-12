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
    if checker.closure_fn_names.contains(n)
        || !checker
            .generic_type_params
            .get(n)
            .is_none_or(|tp| tp.is_empty())
        || !widenable_fn_decl(ast, n, i)
    {
        return false;
    }
    record_widen_site(checker, eid, n, i, WidenTarget::Scalar);
    true
}

/// Is this argument an `as` cast written over a value that is `any`
/// underneath? TS treats the assertion as erasing nothing at runtime,
/// and neither does tr: the operand `lower_as_cast` hands on for a
/// non-scalar annotation is the NaN box itself. So the cast must not
/// buy the argument past a repr gate its uncast twin has to clear.
pub(super) fn as_cast_over_any(checker: &mut Checker, ast: &Ast, arg_id: &ExprId) -> bool {
    let crate::ast::Expr::As { expr, .. } = ast.get_expr(*arg_id) else {
        return false;
    };
    checker.type_of(ast, *expr) == Ok(Type::Any)
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

/// RFC 20260802 gate — can the any-widen lane clone this callee with
/// param `i` widened? Requires a top-level non-generic non-generator
/// FnDecl that is not a lifted closure (`__env` first param), has no
/// rest param (the rest stretch remaps arg indexes past the real
/// param list, so an override index would land on the wrong slot),
/// and declares param `i` (T-28 pad / arity wedges can shift counts).
pub(super) fn widenable_fn_decl(ast: &Ast, name: &str, i: usize) -> bool {
    ast.stmts.iter().any(|s| {
        matches!(s, crate::ast::Stmt::FnDecl { name: n, type_params, is_generator, params, .. }
            if n == name
                && type_params.is_empty()
                && !*is_generator
                && params.first().is_none_or(|p| p.name != "__env")
                && params.iter().all(|p| !p.is_rest)
                && i < params.len())
    })
}
