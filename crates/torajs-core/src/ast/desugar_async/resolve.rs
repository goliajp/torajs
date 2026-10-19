//! The values an async body's returns settle its promise with: the
//! `Promise.resolve(value)` each return hands back, and the undefined
//! its implicit tail completion settles with.

use super::super::{Ast, Expr, ExprId, default_init_for_type};
use super::UNDEF_SLOT_MARKER;

/// The value an async body's implicit tail completion settles with.
///
/// ES §10.2.1.4 step 11 makes that `undefined` whatever the return
/// annotation says, so this is deliberately NOT `default_init_for_type`
/// — that answers the *type's* zero (`0` for number, `""` for string),
/// which is a different value the awaiter cannot tell apart from a real
/// one. Only the annotations that already spell undefined as their zero
/// keep using it; every other width needs its own undefined sentinel,
/// which the `__undef_slot__<T>` marker asks the SSA lowering to
/// materialize (`ssa_lower_ident`, alongside the one an ordinary
/// function's fall-through path emits).
pub(super) fn fallthrough_undefined_for(inner_ty: &str) -> Expr {
    match inner_ty {
        "any" | "void" | "undefined" => default_init_for_type(inner_ty),
        // `number` spells undefined as an F64 sentinel, so it needs the
        // wide slot to carry it. Writing the sentinel as a literal is
        // what earns that: num_width reads a non-integral literal as
        // evidence the slot is F64 and widens the promise's value field
        // to match, which a marker ident would not do.
        "number" => Expr::Number(f64::from_bits(
            crate::ssa_lower_undef_f64_source::F64_UNDEF_SENTINEL_BITS,
        )),
        _ => Expr::Ident(format!("{UNDEF_SLOT_MARKER}{inner_ty}")),
    }
}

/// Build the `Promise.resolve(value)` an async return hands back, in the
/// shape the declared inner type asks for.
///
/// P10.7 — a default-Any async fn wraps the value in `Expr::As { …,
/// ty_ann: "any" }` so the call typechecks as `Promise<Any>` (matching the
/// declared fn return) instead of inferring `Promise<concrete>` from the
/// raw value's static type. Explicit-T async fns pass the value straight
/// through.
///
/// The tail-safety return needs the very same wrap. "Settled with
/// undefined" has two representations in tr — a `Promise<Undefined>`
/// carrying no value, and an Any box holding ANY_UNDEF — and `await`
/// decodes against the *declared* inner type. So an Any-returning function
/// that fell off its end used to hand back the first while its awaiter read
/// the second, answering `[unknown-any-tag]`.
pub(super) fn build_async_resolve(ast: &mut Ast, raw_value: ExprId, inner_ty: &str) -> ExprId {
    let value = if inner_ty == "any" {
        ast.add_expr(Expr::As {
            expr: raw_value,
            ty_ann: "any".into(),
        })
    } else {
        raw_value
    };
    let promise_ident = ast.add_expr(Expr::Ident("Promise".into()));
    let resolve_member = ast.add_expr(Expr::Member {
        obj: promise_ident,
        name: "resolve".into(),
    });
    let call = ast.add_expr(Expr::Call {
        callee: resolve_member,
        args: vec![value],
    });
    // Internal settle spelling — never the user's patchable slot
    // (see `Ast::synth_promise_static_calls`).
    ast.synth_promise_static_calls.insert(call);
    call
}
