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
            // RFC 20260807-global-object G2 — a member STORE through
            // bare `globalThis` stays loud: the write would land on
            // the runtime singleton while every bare-name read keeps
            // its static resolution (`globalThis.Array = x` then
            // `Array` reading the original ctor is the exact silent
            // wrong G2 refuses; expando globals `globalThis.g = 1`
            // then bare `g` diverge the same way). A user binding
            // named globalThis shadows through the lookup gate.
            reject_globalthis_mutation(checker, ast, obj)?;
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
            crate::check_assign_target::check_member(checker, ast, obj, field, value)
        }
        Expr::Index { obj, index } => {
            // G2 — the Index spelling of the globalThis member store
            // (`globalThis["x"] = v` / computed key) rejects for the
            // same reason as the Member arm above.
            reject_globalthis_mutation(checker, ast, obj)?;
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
            crate::check_assign_target::check_index(checker, ast, obj, index, value)
        }
        _ => Err("invalid assignment target".into()),
    }
}

/// RFC 20260807-global-object G2 — property mutation through bare
/// `globalThis` (no user binding shadowing it) is a typecheck error:
/// the singleton is read-only surface until global-binding reflection
/// lands (G4). Alias writes (`var g = globalThis; g.x = 1`) are out
/// of this gate's reach and land on the singleton's dynobj lanes —
/// the same exposure the `Math` ns-object accepted (RFC 20260801).
pub(crate) fn reject_globalthis_mutation(
    checker: &Checker,
    ast: &Ast,
    obj: ExprId,
) -> Result<(), String> {
    if matches!(ast.get_expr(obj), Expr::Ident(n) if n == "globalThis")
        && checker.lookup("globalThis").is_none()
    {
        return Err("not yet supported: assigning to / deleting a property of `globalThis`".into());
    }
    Ok(())
}
