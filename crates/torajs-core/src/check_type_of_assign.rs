//! `Expr::Assign` typecheck pulled out of
//! [`crate::check::Checker::type_of_inner`]'s `Expr::Assign` arm
//! as chunk-313 of the type_of_inner decomp.
//!
//! Assignment target shape dispatch — all three legal target
//! forms delegate to their existing sibling:
//!
//! - `Ident(name)` → [`crate::check_assign_ident::check`] (K.3
//!   top-level data global vs local binding lookup).
//! - `Member { obj, name }` →
//!   [`crate::check_assign_target::check_member`] (M1.4 / P3.2 /
//!   T-27 / T-29 / P9.4 / P8.2 / V3-05 / V3-06 / M-OO.5 readonly).
//! - `Index { obj, index }` →
//!   [`crate::check_assign_target::check_index`] (M1.4
//!   `arr[i] = value` with Number-index + Array<T> elem type
//!   match).
//!
//! Any other target shape is a typecheck error (`invalid
//! assignment target`).

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn check(
    checker: &mut Checker,
    ast: &Ast,
    eid: ExprId,
    target: ExprId,
    value: ExprId,
) -> Result<Type, String> {
    match ast.get_expr(target).clone() {
        Expr::Ident(name) => {
            // RFC 20260710 C5 — rebinding the receiver invalidates
            // every member-path narrow rooted at it (chunk 789:
            // keys are canonical paths, so "h" kills "h.o.cb" too).
            checker
                .member_narrows
                .retain(|(recv, _), _| !crate::check_assigns_to::path_overlaps(recv, &name));
            crate::check_assign_ident::check(checker, ast, target, name, value)
        }
        Expr::Member { obj, name: field } => {
            // G2.5 — a member STORE through bare `globalThis` is loud
            // only for known-builtin names (silent divergence from
            // bare-name static resolution); expando names land on the
            // singleton's dynobj lanes. A user binding named
            // globalThis shadows through the lookup gate.
            reject_globalthis_mutation(checker, ast, obj, Some(field.as_str()))?;
            // RFC 20260710 C5 — writing the member invalidates its
            // own narrow entry and every narrow rooted deeper in it
            // (chunk 789: `h.o = ...` kills ("h.o", cb) AND
            // "h.o.p"-rooted keys).
            if let Some(path) = crate::check_assigns_to::member_path(ast, obj) {
                let full = format!("{path}.{field}");
                checker.member_narrows.retain(|(recv, f), _| {
                    !(recv == &path && f == &field)
                        && !crate::check_assigns_to::path_overlaps(recv, &full)
                });
            }
            crate::check_assign_target::check_member(checker, ast, eid, obj, field, value)
        }
        Expr::Index { obj, index } => {
            // G2.5 — the Index spelling: a string-literal key takes
            // the same builtin-name gate as the Member arm; computed
            // keys pass (alias-write exposure).
            reject_globalthis_mutation(checker, ast, obj, literal_index_key(ast, index))?;
            // Chunk 790 — an element write invalidates narrows rooted
            // at that element path; a computed index (`arr[i] = ...`)
            // conservatively kills every index-rooted narrow under
            // the same base (it may alias any of them).
            if let Some(tp) = crate::check_assigns_to::member_path(ast, target) {
                checker
                    .member_narrows
                    .retain(|(recv, _), _| !crate::check_assigns_to::path_overlaps(recv, &tp));
            } else if let Some(bp) = crate::check_assigns_to::member_path(ast, obj) {
                let base = format!("{bp}[");
                checker
                    .member_narrows
                    .retain(|(recv, _), _| !recv.starts_with(&base));
            }
            crate::check_assign_target::check_index(checker, ast, eid, obj, index, value)
        }
        _ => Err("invalid assignment target".into()),
    }
}

/// RFC 20260807-global-object G2.5 — property mutation through bare
/// `globalThis` (no user binding shadowing it) is loud only when the
/// key names a KNOWN builtin global: that write would land on the
/// runtime singleton while bare-name reads keep static resolution
/// (`globalThis.Array = x` then `Array` reading the original ctor is
/// the silent wrong G2 refuses). Expando keys pass — a bare read of
/// such a name is a compile-time unknown-identifier reject, so the
/// write/read pair through `globalThis` can never diverge silently.
/// Computed / symbol keys pass on the same grounds as alias writes
/// (`var g = globalThis; g.x = 1`): the singleton's dynobj lanes,
/// the exposure the `Math` ns-object accepted (RFC 20260801).
pub(crate) fn reject_globalthis_mutation(
    checker: &Checker,
    ast: &Ast,
    obj: ExprId,
    key: Option<&str>,
) -> Result<(), String> {
    if !matches!(ast.get_expr(obj), Expr::Ident(n) if n == "globalThis")
        || checker.lookup("globalThis").is_some()
    {
        return Ok(());
    }
    match key {
        Some(name) if crate::check_js_semantics::is_known_builtin_global(name) => Err(format!(
            "not yet supported: overriding builtin global `{name}` through `globalThis`"
        )),
        _ => Ok(()),
    }
}

/// The statically-decidable spelling of an Index key — a string
/// literal (`globalThis["x"]`) resolves to the same name domain as
/// the Member form and takes the same builtin-name gate above.
pub(crate) fn literal_index_key(ast: &Ast, index: ExprId) -> Option<&str> {
    match ast.get_expr(index) {
        Expr::String(s) => s.as_str(),
        _ => None,
    }
}
