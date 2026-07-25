//! A `let` / `const` whose closure initializer needs the binding to
//! already exist — either because the closure names the binding itself
//! (`const f = function (n) { … f(n - 1) … }`) or because a peer
//! declared earlier in the same statement list names it (two arrows
//! that call each other).
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
//! Step 1 splits by which shape brought us here. Self-reference is
//! visible from the declaration alone, so [`try_lower`] does it inline.
//! A forward reference is not — the statement that needs the box comes
//! BEFORE the one that declares it — so [`hoist_forward_boxes`] runs
//! over the whole list first and leaves the box waiting; `try_lower`
//! then finds it and goes straight to steps 2 and 3.
//!
//! The env holds the box and the box holds the env: a genuine reference
//! cycle, and precisely the one the collector exists for. A byref
//! non-Copy slot answers `cap_is_traceable`, so the mint emits a real
//! `__env_trace_<fn>` and `collect_white` can walk in and break the
//! edge (RFC 20260717 closure-env-cycle). Refcounting alone will never
//! reclaim a self-referential closure — that is a property of the
//! shape, not a gap in this lane.
//!
//! Narrow by construction: only closure initializers, and only when the
//! binding takes a `Closure` slot. An `any`-annotated binding takes an
//! any slot instead, and the checker declines it in step with this.

use std::collections::HashSet;

use crate::ast::{Expr, ExprId, Stmt};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{LocalInfo, LowerCtx};

/// Give a binding a capture box holding null, register it, and answer
/// the box's value-slot pointer. Shared by both entry shapes so the
/// ownership bookkeeping is written once.
fn open_box(ctx: &mut LowerCtx, name: &str, ty: Type) -> crate::ssa::ValueId {
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
    slot
}

/// The binding's slot type: an annotated binding keeps its declared
/// signature — that is what the call sites type against — and an
/// inferred one takes exactly what the mint is about to produce, read
/// off the same function the mint itself reads so the box and its
/// content cannot disagree. `None` when the binding would not take a
/// closure slot at all.
fn box_ty(
    ctx: &mut LowerCtx,
    name: &str,
    type_ann: Option<&String>,
    init: ExprId,
    fn_name: &str,
    mutable: bool,
) -> Option<Type> {
    let ty = if type_ann.is_some() {
        crate::ssa_lower_stmt_let_decl_general::initial_let_ty(ctx, name, type_ann, init, mutable)
    } else {
        crate::ssa_lower_closure::closure_value_ty(ctx, fn_name)
    };
    matches!(ty, Type::Closure(_)).then_some(ty)
}

/// Open the box for every binding in `stmts` that a closure declared
/// EARLIER in the same list captures. Runs once per statement list,
/// before any of it lowers, so a mutually recursive pair finds each
/// other; `try_lower` recognizes the waiting box by the name being in
/// `hoisted_closure_lets`.
///
/// Only earlier captures: a self-capture is `try_lower`'s inline case,
/// and a LATER peer capturing an EARLIER binding needs nothing — by
/// then the binding is ordinary and already there.
pub(crate) fn hoist_forward_boxes<'s>(ctx: &mut LowerCtx, stmts: impl Iterator<Item = &'s Stmt>) {
    let mut seen_captures: HashSet<String> = HashSet::new();
    let mut plan: Vec<(String, Option<String>, ExprId, String, bool)> = Vec::new();
    collect(ctx, stmts, &mut seen_captures, &mut plan);
    for (name, type_ann, init, fn_name, mutable) in plan {
        let Some(ty) = box_ty(ctx, &name, type_ann.as_ref(), init, &fn_name, mutable) else {
            continue;
        };
        crate::ssa_lower_stmt_let_decl_general::record_binding_flags(
            ctx,
            &name,
            type_ann.as_ref(),
            init,
        );
        open_box(ctx, &name, ty);
        ctx.hoisted_closure_lets.insert(name);
    }
}

/// Walk the list in order, accumulating the names captured so far and
/// picking out each closure-initialized binding one of them already
/// named. `Stmt::Multi` is a transparent grouping sharing the
/// surrounding scope, so it flattens in; anything opening a scope of
/// its own does not — that block runs this pass itself.
fn collect<'s>(
    ctx: &LowerCtx,
    stmts: impl Iterator<Item = &'s Stmt>,
    seen_captures: &mut HashSet<String>,
    plan: &mut Vec<(String, Option<String>, ExprId, String, bool)>,
) {
    for s in stmts {
        match s {
            Stmt::LetDecl {
                name,
                type_ann,
                init,
                mutable,
                ..
            } => {
                let Expr::Closure { fn_name, captures } = ctx.ast.get_expr(*init) else {
                    continue;
                };
                if seen_captures.contains(name) {
                    plan.push((
                        name.clone(),
                        type_ann.clone(),
                        *init,
                        fn_name.clone(),
                        *mutable,
                    ));
                }
                for c in captures {
                    seen_captures.insert(c.clone());
                }
            }
            Stmt::Multi(inner) => collect(ctx, inner.iter(), seen_captures, plan),
            _ => {}
        }
    }
}

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
    let fn_name = fn_name.clone();
    let self_capture = captures.iter().any(|c| c == name);
    // The box may already be waiting — `hoist_forward_boxes` opened it
    // for this list because an earlier closure captured this name.
    if ctx.hoisted_closure_lets.remove(name) {
        let slot = ctx
            .locals
            .get(name)
            .expect("hoisted binding is registered")
            .slot;
        let init_val = ctx.lower_expr(init);
        let cur_block = ctx.cur_block;
        ctx.f.append_void(
            cur_block,
            InstKind::Store(init_val, Operand::Value(slot), 0),
        );
        return true;
    }
    if !self_capture {
        return false;
    }
    let Some(ty) = box_ty(ctx, name, type_ann, init, &fn_name, mutable) else {
        return false;
    };
    crate::ssa_lower_stmt_let_decl_general::record_binding_flags(ctx, name, type_ann, init);
    let slot = open_box(ctx, name, ty);
    let init_val = ctx.lower_expr(init);
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Store(init_val, Operand::Value(slot), 0),
    );
    true
}
