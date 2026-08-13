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

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type, ValueId};
use crate::ssa_lower::LowerCtx;

use super::builtin_type_tag;

/// §13.10.2 for the shapes whose answer the parent's folds cannot
/// know — see [`dynamic_target_name`] for which those are.
///
/// A Closure binding is declined here and handled by
/// [`super::try_lower_fn_value`] — not because it lacks a handler
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
    if !has_any_encoding(ctx.operand_ty(&v)) {
        return None;
    }
    let target_name = dynamic_target_name(ctx, class_name)?;
    let (slot, ty) = resolve_value_binding(ctx, &target_name)?;
    if !has_any_encoding(ty) {
        return None;
    }
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

/// Does [`LowerCtx::box_to_any`] have an encoding for this type?
///
/// Its match ends in a panic, and a panic in the lowerer rejects the
/// whole program — so every operand handed to the runtime operator
/// has to be checked first, on BOTH sides. `FnSig` is the one that
/// bites: a bare function value that never went through the closure
/// construction site has no any-encoding (see
/// `ast_collect_fn_closure`'s wrap), and
/// `genFn instanceof GeneratorFunction` is exactly that shape — the
/// left operand, which is why checking only the target missed it.
///
/// Mirrors the arms of that match — keep the two in step. Distinct
/// from `ssa_lower_boxed_entry`'s same-shaped question about the
/// boxed-entry ABI: that one admits `Ptr` and refuses `Substr`.
fn has_any_encoding(ty: Type) -> bool {
    ty.is_refcounted()
        || matches!(
            ty,
            Type::I64 | Type::I32 | Type::F64 | Type::Bool | Type::Str | Type::Substr | Type::Void
        )
}

/// Which binding the runtime operator should ask, or `None` to leave
/// the answer to the parent's compile-time ladder.
///
/// - A **declared class** normally keeps its descendant-tag fold —
///   that fold IS the spec answer, and it is the fast one. The
///   exception is a class that declares `static [Symbol.hasInstance]`
///   (§15.7): the handler then decides, for any operand, so the
///   target becomes the class object binding `__class_<C>` that
///   `class_globals` emits. Only a member written with that computed
///   key lands in `class_computed_keys`, so this costs one side-table
///   scan and never fires for an ordinary class.
/// - A **builtin constructor** keeps its fold. Installing a handler
///   onto `Array` / `Map` / … is possible but nothing here can see
///   it; recorded residue rather than a fold worth losing.
/// - **Everything else with a binding** goes to the operator: an
///   object literal's computed key and a `defineProperty` install
///   both answer through the same symbol face `N[Symbol.hasInstance]`
///   resolves, where the empty descendant-tag set used to
///   constant-fold `false`.
fn dynamic_target_name(ctx: &LowerCtx<'_>, class_name: &str) -> Option<String> {
    if ctx.ast.class_parents.contains_key(class_name) {
        return class_declares_has_instance(ctx, class_name)
            .then(|| format!("__class_{class_name}"));
    }
    if builtin_type_tag(class_name).is_some()
        || matches!(class_name, "Object" | "Iterator" | "BigInt" | "Symbol")
    {
        return None;
    }
    Some(class_name.to_string())
}

/// Does this class — or anything it extends — write a member under
/// the computed key `[Symbol.hasInstance]`? The parser folds only
/// `Symbol.iterator` to a synthetic name; every other `Symbol.<x>`
/// member key is stashed as the key expression itself, so the test
/// is on its shape.
///
/// The walk up `class_parents` is what makes `x instanceof Sub`
/// answer through a handler `Sub` inherits: a derived constructor's
/// [[Prototype]] is its base, so GetMethod finds the base's handler
/// even though the subclass declares nothing. The depth cap mirrors
/// [`super::compute_descendant_tags`] — a malformed cycle in the
/// side table must not hang the compiler.
fn class_declares_has_instance(ctx: &LowerCtx<'_>, class_name: &str) -> bool {
    let mut cur = Some(class_name.to_string());
    let mut depth = 0u32;
    while let Some(name) = cur {
        if depth > 64 {
            return false;
        }
        if ctx
            .ast
            .class_computed_keys
            .iter()
            .any(|((c, _), &key)| *c == name && is_symbol_has_instance(ctx, key))
        {
            return true;
        }
        cur = ctx.ast.class_parents.get(&name).and_then(|p| p.clone());
        depth += 1;
    }
    false
}

fn is_symbol_has_instance(ctx: &LowerCtx<'_>, key: ExprId) -> bool {
    let Expr::Member { obj, name } = ctx.ast.get_expr(key) else {
        return false;
    };
    name == "hasInstance" && matches!(ctx.ast.get_expr(*obj), Expr::Ident(n) if n == "Symbol")
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
///
/// The operator needs the target as an `Any`, and a `FnSig`-typed
/// slot has no such encoding — asking for one rejects the whole
/// program at compile time. So the boxable cells take the operator
/// and a `FnSig` keeps the ordinary walk it always had. Recorded
/// residue: a handler installed on a value held in such a slot is
/// not consulted. Losing the answer is worse than losing the
/// handler.
pub(super) fn emit_has_instance(
    ctx: &mut LowerCtx<'_>,
    v_any: Operand,
    cell: Operand,
    release: Option<Type>,
) -> Operand {
    if !has_any_encoding(ctx.operand_ty(&cell)) {
        return emit_ordinary_walk(ctx, v_any, cell, release);
    }
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

/// §7.3.22 alone, for a cell the any-encoding cannot carry. Same
/// throw check and same release story as the operator path above.
fn emit_ordinary_walk(
    ctx: &mut LowerCtx<'_>,
    v_any: Operand,
    cell: Operand,
    release: Option<Type>,
) -> Operand {
    let cur_block = ctx.cur_block;
    let r = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.instanceof_fn_value,
            vec![v_any, cell.clone()],
        ),
        Type::Bool,
        None,
    );
    ctx.emit_throw_check(None);
    if let Some(ty) = release {
        ctx.emit_drop_value(cell, ty);
    }
    Operand::Value(r)
}
