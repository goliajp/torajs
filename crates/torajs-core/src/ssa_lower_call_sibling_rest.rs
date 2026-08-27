//! The one direct call site with no packing behind it.
//!
//! A rest parameter is a single array, and every ordinary call site
//! builds that array at AST level: `apply_rest_args` rewrites
//! `f(a, b, c)` into `f(a, [b, c])` when `f` declares `...rest`. It
//! only ever sees an `Ident` callee, though, and a `Member`-shape
//! call survives desugar whenever several unrelated classes declare
//! the name — the receiver's class is not known until
//! [`crate::ssa_lower_call_sibling_class_dispatch`] resolves it.
//!
//! So that lane handed a rest-declaring body its trailing arguments
//! one per register, and the parameter read a scalar as an array
//! pointer. Nothing announced it: the ABI check downstream compares
//! MACHINE shapes, and a scalar and an array are both one word — the
//! program simply died with a segfault, and did so for every shape in
//! the family (a call with too few arguments got the `undefined` pad
//! in the tail's register instead, which is the same crash by a
//! different route).
//!
//! The collection is built as `Arr<Any>` because that is what an
//! unannotated `...rest` is; a TYPED rest converts through the same
//! assign-boundary kernel a typed `let` uses, so a mismatched element
//! raises the catchable TypeError there rather than being read as the
//! wrong thing here.

use crate::ast::ExprId;
use crate::ssa::{Operand, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_stmt_let_decl_convert::arr_elem_kind_const;

/// Whether the tail this lane would build can reach the rest slot.
///
/// Asked BEFORE any argument is lowered. The lane's decline protocol
/// re-parks only the receiver, so a decline taken after the arguments
/// were lowered would have the next dispatcher lower them a second
/// time — and a declining lane is the right answer here: the runtime
/// dispatch behind it packs the tail out of argv correctly, which is
/// why an `any`-typed receiver never had this crash.
pub(crate) fn packable(ctx: &LowerCtx<'_>, slot: Option<&Type>) -> bool {
    let Some(Type::Arr(id)) = slot else {
        return false;
    };
    let elem = &ctx.arr_layouts[id.0 as usize];
    *elem == Type::Any || arr_elem_kind_const(elem).is_some()
}

/// Collect a call's trailing arguments into the one array its
/// callee's rest parameter is. Answers the operand for that slot and
/// the temp the calling lane releases after the call — the callee
/// borrows it, the same contract an AST-packed array literal arrives
/// under.
pub(crate) fn pack(
    ctx: &mut LowerCtx<'_>,
    tail: &[ExprId],
    slot: Option<Type>,
) -> (Operand, (Operand, Type)) {
    // §10.2.1.3 — the rest is a fresh Array of the remaining
    // arguments, spreads inside it included (`lower_array_any_literal`
    // routes those through `arr_extend_any`).
    let any_arr = ctx.lower_array_any_literal(tail);
    let any_ty = ctx.operand_ty(&any_arr);
    let (Some(Type::Arr(id)), Type::Arr(_)) = (slot, any_ty) else {
        return (any_arr.clone(), (any_arr, any_ty));
    };
    let elem = ctx.arr_layouts[id.0 as usize];
    if elem == Type::Any {
        return (any_arr.clone(), (any_arr, any_ty));
    }
    let Some(kind) = arr_elem_kind_const(&elem) else {
        return (any_arr.clone(), (any_arr, any_ty));
    };
    let fid = *ctx
        .fn_table
        .get("__torajs_arr_any_to_typed")
        .expect("__torajs_arr_any_to_typed intrinsic missing");
    let slot_ty = Type::Arr(id);
    let typed = ctx.f.append_inst(
        ctx.cur_block,
        crate::ssa::InstKind::Call(fid, vec![any_arr.clone(), Operand::ConstI64(kind)]),
        slot_ty,
        None,
    );
    // A mismatched element arms the catchable TypeError and answers
    // NULL; the check leaves for the handler before anything reads it.
    ctx.emit_throw_check(None);
    // The typed copy owns the elements' new stakes, so the collection
    // this lane minted is done.
    ctx.emit_drop_value(any_arr, any_ty);
    let typed = Operand::Value(typed);
    (typed.clone(), (typed, slot_ty))
}
