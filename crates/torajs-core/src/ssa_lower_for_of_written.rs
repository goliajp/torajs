//! Does a for-of body WRITE its loop variable? (rotation 638, plan
//! 636-15 ①: `for (let z of xs) { z = 10 }` was refused by the checker
//! as an assignment to a const.)
//!
//! The answer decides the binding's ownership. The array and
//! iterator-protocol lanes bind the loop variable as a BORROWED alias
//! of the element (`moved: true, borrowed: true` — no retain at init,
//! no drop at the iteration's end), which is exact as long as nothing
//! stores into it. A body write stores an owned value: the assign
//! lowering skips dropping the alias (the moved mark) and clears the
//! mark, so the frame close would then drop the slot — on EVERY
//! iteration, including the ones whose write sat in an untaken branch
//! and left the borrowed alias in the slot. That is a release the
//! element's owner never granted. A loop variable the body writes is
//! therefore bound OWNED from the start: the alias is retained at
//! init and the slot dropped at each iteration's end, the same shape
//! a `let z = xs[i]` inside the body would take.
//!
//! Direct writes in the body (through nested blocks and loops, not into
//! closures) are what the question covers; a closure that writes a
//! captured loop variable takes the capture-box route, whose ownership
//! the box-drop sites settle.

use crate::ast::Stmt;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_stmt_for_of::bind_scoped_local;

pub(crate) fn body_writes_var(ctx: &LowerCtx, body: &Stmt, name: &str) -> bool {
    crate::ssa_lower_closure_captures::collect_assigned_names_scoped(
        ctx.ast,
        std::iter::once(body),
        &[],
    )
    .contains(name)
}

/// What the lane hands `bind_loop_var` as the iteration's value.
pub(crate) enum LoopVarInit {
    /// A borrowed alias of storage someone else owns (an array
    /// element, an iterator step's value).
    Alias,
    /// A value the loop already owns (the any lane's out-slot, a
    /// map-like lane's loaded entry).
    Owned,
}

/// Open the loop variable's slot for this iteration and bind it —
/// the one place every for-of lane decides ownership and boxing:
///
/// - captured by an escaping closure and (for a non-Copy) written:
///   a capture box, exactly as a `let` takes one
///   (`ssa_lower_stmt_let_decl_general`) — the closures share the
///   live per-iteration binding, and `escape_captured_lets` is
///   name-keyed, so a plain slot here would be read as a box;
/// - otherwise an alloca: OWNED when the body writes the variable
///   (an alias is retained first), a borrowed alias when it does not.
pub(crate) fn bind_loop_var(
    ctx: &mut LowerCtx,
    var_name: &str,
    v_val: Operand,
    v_ty: Type,
    body: &Stmt,
    init: LoopVarInit,
) {
    let (v_val, v_ty) = widen_to_class(ctx, var_name, v_val, v_ty);
    let alias = matches!(init, LoopVarInit::Alias);
    let escape = ctx.escape_captured_lets.contains(var_name);
    let boxed_noncopy = !v_ty.is_copy()
        && v_ty != Type::Substr
        && escape
        && ctx.mutated_captured_lets.contains(var_name);
    let escape_captured =
        (v_ty.is_copy() && escape && ctx.mutated_captured_lets.contains(var_name)) || boxed_noncopy;
    if escape_captured {
        if boxed_noncopy && alias {
            ctx.emit_owned_result_inc(v_val, v_ty);
        }
        let slot = ctx.emit_capture_boxed(v_ty, v_val);
        if boxed_noncopy {
            ctx.boxed_noncopy_lets.insert(var_name.to_string());
        }
        bind_scoped_local(ctx, var_name, slot, v_ty, true, false, true);
        return;
    }
    // An element the body may release before it reads the variable
    // again (`xs[0] = new N(); use(x)`, `xs.length = 0`) is retained
    // too: the borrow would name a freed cell. A string view's bytes
    // belong to its container, so a retain cannot keep one alive.
    let borrowed = alias
        && !body_writes_var(ctx, body, var_name)
        && (v_ty == Type::Substr
            || !crate::ssa_lower::alias_hazard::loop_var_read_after_release(ctx, body, var_name));
    if alias && !borrowed {
        ctx.emit_owned_result_inc(v_val, v_ty);
    }
    let slot = ctx.alloca(v_ty, Some(var_name));
    let cur_block = ctx.cur_block;
    ctx.f
        .append_void(cur_block, InstKind::Store(v_val, Operand::Value(slot), 0));
    bind_scoped_local(ctx, var_name, slot, v_ty, borrowed, borrowed, false);
}

/// An integer element bound to a variable the width analysis classes
/// F64 — a body write the element's own width does not cover — binds
/// as an f64, so the write it is classed for fits the slot.
fn widen_to_class(ctx: &mut LowerCtx, var_name: &str, v: Operand, ty: Type) -> (Operand, Type) {
    let fn_name = if ctx.is_main_fn {
        ""
    } else {
        ctx.f.name.as_str()
    };
    let keys = ctx.num_f64_slots.ident_keys(fn_name, var_name);
    if ty != Type::I64 || !keys.iter().any(|k| ctx.num_f64_slots.slot_is_f64(k)) {
        return (v, ty);
    }
    let f = ctx
        .f
        .append_inst(ctx.cur_block, InstKind::SiToFp(v), Type::F64, None);
    (Operand::Value(f), Type::F64)
}

/// A loop variable off a container some write filled with the
/// out-of-range sentinel can hold one, and the body reads it by
/// name — the same registry a `const x = ys[i]` init fills. Without
/// this `for (const x of ys) typeof x` answered "number" where the
/// identical `typeof ys[i]` at the same index answered "undefined".
/// A `string[]` hole reads as the Str sentinel the same way, and the
/// Str registry is `nullable_str_lets`.
pub(crate) fn note_sentinel_loop_var(
    ctx: &mut LowerCtx,
    var_name: &str,
    v_ty: Type,
    src_ref_eid: crate::ast::ExprId,
) {
    let crate::ast::Expr::Ident(src) = ctx.ast.get_expr(src_ref_eid) else {
        return;
    };
    if !ctx.num_f64_slots.container_elem_takes_sentinel(src) {
        return;
    }
    match v_ty {
        Type::F64 => ctx.undefable_f64_lets.insert(var_name.to_string()),
        Type::Str => ctx.nullable_str_lets.insert(var_name.to_string()),
        t if t.spells_undef_with_generic_cell() => {
            ctx.undefable_heap_lets.insert(var_name.to_string())
        }
        _ => false,
    };
}
