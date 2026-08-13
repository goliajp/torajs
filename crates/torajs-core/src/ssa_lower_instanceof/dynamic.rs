//! The §13.10.2 runtime lane of `instanceof` — the half that cannot
//! be answered from a heap tag.
//!
//! [`super`] answers class membership at compile time: it computes a
//! class's descendant tag set and emits an equality chain. That is
//! right whenever the operator's answer is fixed by the program's
//! class hierarchy — and wrong the moment a target carries its own
//! `@@hasInstance` handler, because then the answer is whatever that
//! handler returns, for any operand, including primitives the
//! ordinary walk would reject outright.
//!
//! So these two entry points sit AHEAD of every fold in the parent:
//!
//! - [`try_lower_dynamic_target`] — the name resolves to a plain
//!   value (an object literal, a `defineProperty` target) rather
//!   than a declared class or builtin constructor.
//! - [`emit_has_instance`] — the callable lane's call site, reached
//!   once the parent has resolved the canonical cell and knows the
//!   stake story for it.
//!
//! Both end in `__torajs_instanceof_dynamic`, which probes for the
//! handler and otherwise falls through to OrdinaryHasInstance — so
//! declining to route a shape here never loses an answer, it only
//! keeps the faster fold.

use crate::ssa::{InstKind, Operand, Type, ValueId};
use crate::ssa_lower::LowerCtx;

use super::builtin_type_tag;

/// §13.10.2 for `x instanceof N` where `N` names a plain VALUE — not
/// a declared class, not a builtin constructor. Those two keep their
/// compile-time tag folds (a class's own `@@hasInstance` is the next
/// knife; a builtin's is the one after). Everything else that has a
/// binding is handed to the runtime operator, which reads the
/// handler through the same symbol face `N[Symbol.hasInstance]`
/// resolves — so an object literal's computed key and a
/// `defineProperty` install both answer, where the empty
/// descendant-tag set used to constant-fold `false`.
///
/// A Closure binding is declined here and handled by
/// [`try_lower_fn_value`] below — not because it lacks a handler
/// (the symbol face serves callables fine), but because that lane
/// already resolves the canonical `__fncell_*` cell every channel
/// shares and owns the stake story for it. It calls the same runtime
/// operator.
///
/// `None` = not this shape; the caller continues down the static
/// ladder.
pub(super) fn try_lower_dynamic_target(
    ctx: &mut LowerCtx<'_>,
    v: Operand,
    class_name: &str,
) -> Option<Operand> {
    if ctx.ast.class_parents.contains_key(class_name)
        || builtin_type_tag(class_name).is_some()
        || matches!(class_name, "Object" | "Iterator" | "BigInt" | "Symbol")
    {
        return None;
    }
    let (slot, ty) = resolve_value_binding(ctx, class_name)?;
    if matches!(ty, Type::Closure(_) | Type::FnSig(_)) {
        return None;
    }
    let cur_block = ctx.cur_block;
    let target = ctx.f.append_inst(
        cur_block,
        InstKind::Load(ty, Operand::Value(slot), 0),
        ty,
        None,
    );
    let target_any = ctx.box_to_any(Operand::Value(target));
    let v_any = ctx.box_to_any(v);
    let cur_block = ctx.cur_block;
    let r = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.instanceof_dynamic, vec![v_any, target_any]),
        Type::Bool,
        None,
    );
    // Steps 1 and 4 throw, and so can the handler body.
    ctx.emit_throw_check(None);
    Some(Operand::Value(r))
}

/// The name's storage slot — a local binding directly, a module
/// global through a `GlobalRef` (the shape
/// [`crate::ssa_lower_call_closure_local::resolve_closure_binding`]
/// uses for the callable case).
fn resolve_value_binding(ctx: &mut LowerCtx<'_>, name: &str) -> Option<(ValueId, Type)> {
    if let Some(i) = ctx.locals.get(name).copied() {
        return Some((i.slot, i.ty));
    }
    let ty = *ctx.globals.get(name)?;
    let cur_block = ctx.cur_block;
    let g = ctx.f.append_inst(
        cur_block,
        InstKind::GlobalRef(name.to_string()),
        Type::Ptr,
        None,
    );
    Some((g, ty))
}

/// The §13.10.2 runtime call + throw check; `release` carries the
/// cell's type when the operand arrived OWNED (the canonical mint's
/// +1 per use) and needs a post-call drop — the binding lane's Load
/// answers a borrow and passes `None`.
///
/// The operator, not the bare §7.3.22 walk: a callable can carry its
/// own `@@hasInstance` (installed by `defineProperty` — the property
/// on `Function.prototype` is non-writable, so assignment is not the
/// spelling), and the walk alone would ignore it. When no handler is
/// installed the operator falls through to exactly the walk this
/// used to call, so every answer it already gave is preserved.
/// Boxing the cell is a pure encode (RC-neutral), leaving the
/// `release` story below untouched.
pub(super) fn emit_has_instance(
    ctx: &mut LowerCtx<'_>,
    v_any: Operand,
    cell: Operand,
    release: Option<Type>,
) -> Operand {
    let cell_any = ctx.box_to_any(cell.clone());
    let cur_block = ctx.cur_block;
    let r = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.instanceof_dynamic, vec![v_any, cell_any]),
        Type::Bool,
        None,
    );
    ctx.emit_throw_check(None);
    if let Some(ty) = release {
        ctx.emit_drop_value(cell, ty);
    }
    Operand::Value(r)
}
