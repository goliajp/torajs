//! A `let` / `const` whose initializer is a closure that captures the
//! binding being declared — `const f = function (n) { … f(n - 1) … }`
//! and the arrow form.
//!
//! Every other init lowers value-first: produce the operand, then give
//! the binding a slot to hold it. That order cannot work here. The
//! closure's env has to hold the binding at mint time, and the binding
//! has nothing to hold until the mint is done. So this lane inverts it:
//!
//! 1. Mint the capture box holding null and register the binding on it.
//!    Recording the name in `boxed_noncopy_lets` is what makes the
//!    capture write take the byref path — the env stores the BOX
//!    pointer and takes a share of the box, rather than snapshotting a
//!    value that does not exist yet.
//! 2. Lower the closure. Its capture write now finds the box.
//! 3. Store the closure into the box. Every read of the name, including
//!    the ones inside the body, goes through that single cell — which
//!    is also what ES §9.1 asks for: a closure captures the BINDING.
//!
//! The env holds the box and the box holds the env: a genuine reference
//! cycle, and precisely the one the collector exists for. A byref
//! non-Copy slot answers `cap_is_traceable`, so the mint emits a real
//! `__env_trace_<fn>` and `collect_white` can walk in and break the
//! edge (RFC 20260717 closure-env-cycle). Refcounting alone will never
//! reclaim a self-referential closure — that is a property of the
//! shape, not a gap in this lane.
//!
//! Narrow by construction: only a capture of the name being declared,
//! and only when the binding types as a `Closure` slot. A closure that
//! captures a LATER binding (two arrows calling each other) is not this
//! lane's shape and still reports the unknown identifier.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{LocalInfo, LowerCtx};

pub(crate) fn try_lower(
    ctx: &mut LowerCtx,
    name: &str,
    type_ann: Option<&String>,
    init: ExprId,
    mutable: bool,
) -> bool {
    let Expr::Closure { fn_name, captures } = ctx.ast.get_expr(init) else {
        return false;
    };
    if !captures.iter().any(|c| c == name) {
        return false;
    }
    let fn_name = fn_name.clone();
    // An annotated binding keeps its declared signature — that is what
    // the call sites type against. An inferred one takes exactly what
    // the mint is about to produce, read off the same function the mint
    // itself reads so the box and its content cannot disagree.
    let ty = if type_ann.is_some() {
        crate::ssa_lower_stmt_let_decl_general::initial_let_ty(ctx, name, type_ann, init, mutable)
    } else {
        crate::ssa_lower_closure::closure_value_ty(ctx, &fn_name)
    };
    if !matches!(ty, Type::Closure(_)) {
        return false;
    }
    crate::ssa_lower_stmt_let_decl_general::record_binding_flags(ctx, name, type_ann, init);
    let cur_depth = ctx.scope_stack.len() - 1;
    let slot = ctx.emit_capture_boxed(ty, Operand::ConstPtrNull);
    ctx.boxed_noncopy_lets.insert(name.to_string());
    if let Some(prev) = ctx.locals.get(name).copied()
        && prev.scope_depth < cur_depth
    {
        let top_shadow = ctx.shadow_stack.last_mut().expect("shadow frame");
        top_shadow.push((name.to_string(), prev));
    }
    ctx.locals.insert(
        name.to_string(),
        LocalInfo {
            slot,
            ty,
            // The box owns the closure's stake and this frame owns the
            // box, so the scope-close walk has to reach it (the
            // `boxed_noncopy_lets` arm of the drop walk releases it).
            moved: true,
            borrowed: false,
            scope_depth: cur_depth,
        },
    );
    ctx.scope_stack
        .last_mut()
        .expect("scope frame")
        .push(name.to_string());
    let init_val = ctx.lower_expr(init);
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Store(init_val, Operand::Value(slot), 0),
    );
    true
}
