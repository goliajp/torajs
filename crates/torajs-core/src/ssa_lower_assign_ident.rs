//! `Expr::Assign { target: Expr::Ident(name), value }` lowering pulled
//! out of [`crate::ssa_lower::lower_expr_inner`]'s `Expr::Assign`
//! match arm as chunk-79a of the decomp (chunks 1-78 = ... + `Expr::New`
//! 6-class ctor cluster).
//!
//! Two assignment paths (plus the RFC 20260730 undeclared-write
//! ReferenceError lane, keyed by the target eid the checker marked —
//! see [`lower_undeclared_write_throw`]):
//!
//! 1. **K.3 module-level data global** — `globals.contains(name)`:
//!    `GlobalRef + Store` to the slot pointer. Primitive Copy types
//!    have no old-value drop dance; every refcounted slot runs the
//!    borrow-inc → load-old → store-new → drop-old sequence (no cell
//!    moves under its own methods, so no slot writeback is needed).
//!    P11.2-A1 type-check rejects silent `f64 → i64` slot stores;
//!    permissible coercions:
//!    - `slot F64 + value I64` → `coerce_to_f64`
//!    - `slot {I64|F64} + value Any` → `coerce_any_to_number` (catch-
//!      block narrow)
//!    - `slot Str + value Any` → `coerce_to_str`
//!    - `slot I64 + value I32` / `slot I32 + value I64`
//!    - `slot Ptr || value Ptr` (free)
//! 2. **Local binding** — `locals.contains(name)`:
//!    Lower rhs FIRST (`s = s + "x"` consumes the lhs binding inside
//!    the BinOp; the drop-old gate below skips it). For refcounted
//!    borrow rhs (Member / container Index / named Ident),
//!    `emit_rc_inc` since lhs + rhs share ownership — assignment is
//!    a SHARE, never a move (chunk 564). Type-check with
//!    permissible coercions:
//!    - `slot Any + value !Any` → `box_to_any`
//!    - `slot F64 + value I64` → `coerce_to_f64`
//!    - `slot {I64|F64} + value Any` → `coerce_any_to_number`
//!    - `slot Str + value Any` → `coerce_to_str` (catch-block
//!      `saved = e` shape)
//!    - `slot I64 + value I32` / `slot I32 + value I64` / Ptr-free
//!      Drop old slot value if non-Copy AND not moved out by RHS
//!      consume. Store new value; clear `moved` flag so subsequent
//!      reads work and end-of-fn drop fires.
//!
//! Returns `Operand` directly (TS assignment-as-expression yields
//! the assigned value).
//!
//! ## The returned value is OWNED, not a slot borrow
//!
//! Both lanes answer the very operand they stored — a borrow of the
//! slot that now owns it. A consumer that keeps the value (`b = (a =
//! [1,2,3])`, `var b = (a = ...)`, `{ k: (a = ...) }`, `return (a =
//! ...)`) stores it into a second place, and at scope end BOTH places
//! release: one `rc_inc`, two `rc_dec` → refcount underflow (`0 - 1`
//! wraps to `0xffffffff` in release) on already-freed memory, and the
//! corpse stays in the cycle-root buffer until `__torajs_cycle_at_exit
//! _drain` walks it and segfaults (rotation 323).
//!
//! Fixing it consumer-side would mean adding `Expr::Assign` to every
//! borrow-needs-inc whitelist (`borrow::apply_borrow_rc_inc`, the
//! let-decl alias gate, `ssa_lower_array`'s element loop,
//! `ssa_lower_object_lit`'s …) — whack-a-mole, and the ones that
//! transfer instead of inc'ing (array elements) would still steal the
//! slot's only stake. So [`mint_consumer_stake`] takes the `+1` HERE,
//! once, and records the eid so [`crate::ssa_lower::LowerCtx::
//! expr_owned_shape`] answers owned. That puts assignment-as-value on
//! the same contract as `Expr::Call` / `Expr::Array`: consumers that
//! keep it transfer without inc'ing, and discard sites
//! (`release_owned_temp`, `Stmt::Expr`) release it.
//!
//! Only the Ident-target lane mints. The Member / Index target lanes
//! do NOT answer a uniform slot borrow (the setter-call arm and the
//! `Str ← Any` index arm already mint fresh owned values), so they
//! stay off `owned_member_reads` and keep their current contract.

mod borrow;
mod throw;

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;
use borrow::apply_borrow_rc_inc;
use throw::{lower_immutable_write_throw, lower_undeclared_write_throw};

use crate::ssa_lower_assign_slot_fit::{
    check_local_coercion, coerce_for_global, coerce_for_local, global_coercion_compatible,
    mismatch_hint,
};

pub(crate) fn lower(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    target: ExprId,
    name: String,
    value: ExprId,
) -> Operand {
    // §8.1.1.1 / §6.2.5.6 — PutValue on a binding still in its dead
    // zone. Ahead of the unresolvable-Reference arm below because the
    // two marks can both be present: the checker files a write it
    // cannot resolve, and a dead-zone binding with a non-literal
    // initializer is exactly such a write. The name IS declared, so
    // this is the truer of the two.
    if ctx.ast.tdz_reads.contains(&target) {
        return lower_undeclared_write_throw(ctx, &name, value, true);
    }
    if ctx.ast.undeclared_reads.contains_key(&target) {
        // §9.1.1.4.6 SetMutableBinding with `S` = false — under the
        // SLOPPY goal an assignment to an unresolvable name CREATES
        // the property on the global object rather than throwing, and
        // whether the name is already there is a runtime fact (after
        // `desugar_with` splits a body reference, only the scope
        // object decides which arm runs). Strict / module code keeps
        // the throw: §6.2.5.6 PutValue with `S` = true raises.
        // …except where the statements came out of STRICT eval text.
        // §19.2.1.1 gives eval code its own strictness, and strict code
        // creates no globals; after `desugar_eval` splices those
        // statements in, nothing downstream can tell them from the
        // sloppy program around them, so the name is what carries the
        // verdict (`ast.strict_eval_assign_names`, filled there for
        // exactly this).
        // `cur_fn_is_sloppy`, not the PROGRAM's goal: §11.2.2 gives a
        // `"use strict"` body its own strictness inside a sloppy
        // Script, and class code is always strict. Keying on the
        // program alone sent a strict function's unresolvable write to
        // the object, and 14 cases that assert the ReferenceError went
        // from pass to bug in one sweep — the same cut the [[Set]]
        // flavor lane records next door.
        if crate::ssa_lower_set_flavor::cur_fn_is_sloppy(ctx)
            && !ctx.ast.strict_eval_assign_names.contains(&name)
        {
            return lower_global_ident_set(ctx, &name, value);
        }
        // The throw makes the value unobservable and the lane past
        // the check is unreachable — nothing to hand a consumer.
        return lower_undeclared_write_throw(ctx, &name, value, false);
    }
    if ctx.ast.const_writes.contains(&target) {
        // §9.1.1.1.5 SetMutableBinding step 5 forces S to true for a
        // strict immutable binding, so this one does NOT get the
        // sloppy no-op the self-name lane below takes — a write to a
        // `const` throws in sloppy code too.
        return lower_immutable_write_throw(ctx, value);
    }
    if ctx.ast.self_name_writes.contains(&target) {
        // §15.5.5 (RFC 20260810) — write to the enclosing
        // fn-expression's self-name: an immutable function-env
        // binding, so a strict-mode write raises TypeError. Same
        // kernel and message as bun's JSC ("Attempted to assign to
        // readonly property."). Under a SLOPPY function (§11.2.2 —
        // the function's own strictness, not the program's) PutValue
        // with S = false on an immutable binding is a no-op: the RHS
        // is evaluated and is the expression's value, nothing is
        // stored (rotation 635; test262 no-strict-reassign-fn-name).
        if crate::ssa_lower_set_flavor::cur_fn_is_sloppy(ctx) {
            let v = ctx.lower_expr(value);
            mint_consumer_stake(ctx, eid, &v);
            ctx.release_owned_temp(value, &v);
            return v;
        }
        return lower_immutable_write_throw(ctx, value);
    }
    // `m = re.exec(t)` over a binding held as an exec record refills
    // the record (`ssa_lower_exec_record`).
    if let Some(v) = crate::ssa_lower_exec_record::try_assign(ctx, &name, value) {
        return v;
    }
    let v = if !ctx.locals.contains_key(&name)
        && let Some(slot_ty) = ctx.globals.get(&name).copied()
    {
        lower_global_assign(ctx, name, slot_ty, value)
    } else {
        lower_local_assign(ctx, name, value)
    };
    mint_consumer_stake(ctx, eid, &v);
    v
}

/// Take the `+1` that turns the stored-slot borrow into a value the
/// consumer owns, and record `eid` so `expr_owned_shape` answers
/// owned (module doc: why here and not in each consumer's whitelist).
/// Copy-typed slots mint nothing. Neither does a statement-position
/// assignment (`p = p.then(f);`, a `for` step): its only consumer is
/// the discarding release, which keys off the recorded eid, so the
/// slot alone keeps the reference — the inc/dec pair's net result.
fn mint_consumer_stake(ctx: &mut LowerCtx<'_>, eid: ExprId, v: &Operand) {
    if !ctx.operand_ty(v).is_refcounted() || ctx.discarded_stmt_expr == Some(eid) {
        return;
    }
    ctx.emit_rc_inc(*v);
    ctx.owned_member_reads.insert(eid);
}

/// §9.1.1.4.6 SetMutableBinding, sloppy — the write lands on the
/// global object, and the assignment still answers the value it wrote
/// (§13.15.2 step 5). The kernel mints the singleton if it has to;
/// that is the one operation in this family that does.
fn lower_global_ident_set(ctx: &mut LowerCtx<'_>, name: &str, value: ExprId) -> Operand {
    let v = ctx.lower_expr(value);
    let boxed = ctx.box_to_any(v);
    let name_str = ctx.intern_string_literal(name);
    let f = ctx.intrinsics.global_ident_set;
    let cur_block = ctx.cur_block;
    let out = ctx.f.append_inst(
        cur_block,
        InstKind::Call(f, vec![Operand::Value(name_str), boxed]),
        Type::Any,
        None,
    );
    ctx.emit_throw_check(None);
    Operand::Value(out)
}

fn lower_global_assign(
    ctx: &mut LowerCtx<'_>,
    name: String,
    slot_ty: Type,
    value: ExprId,
) -> Operand {
    // Every refcounted slot rides drop-old/store-new. No heap cell
    // moves under its own methods (an array grows its spilled data
    // buffer, a Map / Set its table, a Date / RegExp / Promise / weak
    // collection mutates in place), so reassignment needs no slot
    // writeback: load the old value, store the new, release the old.
    let drop_old_slot = slot_ty.is_refcounted();
    crate::ssa_lower_binop::str_check::defer_rhs(ctx, &name, value);
    let v = lower_assign_rhs(ctx, slot_ty, value, (&name, true));
    // Chunk 558 — mutable Str globals; chunk 730 (RFC
    // 20260709-closure-global) — mutable Closure globals ride the
    // same sequence (a fresh `(x) => ...` rhs is an owned env mint
    // and transfers; strings have no in-place mutation methods and a
    // closure's env is opaque to user code, so the K.6 writeback
    // concern exists for neither). RHS lowers FIRST (`g = g + "x"`
    // reads the old slot value); borrow-shaped rhs (Member / Index /
    // unmoved Ident, including reads of another global slot) takes +1
    // since the slot and the rhs home share ownership. Then
    // load-old → store-new → drop-old; self-assign `g = g` is safe
    // because the borrow inc lands before the old value's dec.
    let v_ty = ctx.operand_ty(&v);
    // A view into a Str slot is copied out below and takes no share
    // (an inline view's own count is never read by its drop).
    if drop_old_slot && !(slot_ty == Type::Str && v_ty == Type::Substr) {
        apply_borrow_rc_inc(ctx, &v, value);
    }
    if !global_coercion_compatible(ctx, &name, slot_ty, v_ty) {
        panic!(
            "ssa-lower: assignment to global `{name}` mismatch — slot is {slot_ty:?} but value is {v_ty:?}{}",
            mismatch_hint(&slot_ty, &v_ty),
        );
    }
    // Chunk 809 — a concrete rhs boxes into the Any slot (the borrow
    // inc above, or a named function's box, supplies the stake the
    // slot takes; expr-aware so undefined/null keep their tags).
    let v = if slot_ty == Type::Any && v_ty != Type::Any {
        ctx.box_owned_from_expr(value, v, false)
    } else {
        coerce_for_global(ctx, &name, slot_ty, v_ty, v, value)
    };
    let cur_block = ctx.cur_block;
    let ptr = ctx.f.append_inst(
        cur_block,
        InstKind::GlobalRef(name.clone()),
        Type::Ptr,
        None,
    );
    // The `x = x` the `var` hoist mints for an init-less eval /
    // Script `var` stores nothing the program meant, so over an
    // existing property the property's value is the binding's; any
    // other store IS the value (`BindSite`).
    let site = if matches!(ctx.ast.get_expr(value), Expr::Ident(n) if *n == name) {
        crate::ssa_lower_stmt_let_decl_global::BindSite::Write
    } else {
        crate::ssa_lower_stmt_let_decl_global::BindSite::WriteAssigned
    };
    if drop_old_slot {
        let cur_block = ctx.cur_block;
        let old = ctx.f.append_inst(
            cur_block,
            InstKind::Load(slot_ty, Operand::Value(ptr), 0),
            slot_ty,
            None,
        );
        let cur_block = ctx.cur_block;
        ctx.f
            .append_void(cur_block, InstKind::Store(v, Operand::Value(ptr), 0));
        ctx.emit_drop_value(Operand::Value(old), slot_ty);
        // A synthesized implicit global's property is created by the
        // write (RFC 20260912-global-var-cell, `BindSite`).
        crate::ssa_lower_stmt_let_decl_global::emit_script_global_var_bind(
            ctx, &name, slot_ty, ptr, site,
        );
        crate::ssa_lower_binop::str_check::emit_deferred(ctx);
        return v;
    }
    let cur_block = ctx.cur_block;
    ctx.f
        .append_void(cur_block, InstKind::Store(v, Operand::Value(ptr), 0));
    crate::ssa_lower_stmt_let_decl_global::emit_script_global_var_bind(
        ctx, &name, slot_ty, ptr, site,
    );
    crate::ssa_lower_binop::str_check::emit_deferred(ctx);
    let cur_block = ctx.cur_block;
    let r = ctx.f.append_inst(
        cur_block,
        InstKind::Load(slot_ty, Operand::Value(ptr), 0),
        slot_ty,
        None,
    );
    Operand::Value(r)
}

fn lower_local_assign(ctx: &mut LowerCtx<'_>, name: String, value: ExprId) -> Operand {
    let snapshot = match ctx.locals.get(&name) {
        Some(i) => *i,
        None => panic!("ssa-lower: assign to unknown ident `{name}`"),
    };
    crate::ssa_lower_binop::str_check::defer_rhs(ctx, &name, value);
    let v = lower_assign_rhs(ctx, snapshot.ty, value, (&name, false));
    let v_ty = ctx.operand_ty(&v);
    // Same carve-out as the global lane: a view into a Str slot is
    // copied out and takes no share.
    if !(snapshot.ty == Type::Str && v_ty == Type::Substr) {
        apply_borrow_rc_inc(ctx, &v, value);
    }
    check_local_coercion(ctx, &name, snapshot.ty, v_ty);
    let v = coerce_for_local(ctx, &name, snapshot.ty, v_ty, v, value);
    let post_rhs = *ctx.locals.get(&name).unwrap_or(&snapshot);
    // RFC 20260710 — a promoted mutable capture binding is
    // moved-marked (its stake lives in the capture box), but the box
    // DOES own the old value: the overwrite must release it, and the
    // moved mark must survive the assign (the box-drop sites release
    // the stake, not the plain drop walk).
    let is_boxed = ctx.boxed_noncopy_lets.contains(&name);
    if !snapshot.ty.is_copy() && (!post_rhs.moved || is_boxed) {
        let cur_block = ctx.cur_block;
        let old = ctx.f.append_inst(
            cur_block,
            InstKind::Load(snapshot.ty, Operand::Value(snapshot.slot), 0),
            snapshot.ty,
            None,
        );
        ctx.emit_drop_value(Operand::Value(old), snapshot.ty);
    }
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Store(v, Operand::Value(snapshot.slot), 0),
    );
    if let Some(info) = ctx.locals.get_mut(&name)
        && !is_boxed
    {
        info.moved = false;
    }
    crate::ssa_lower_binop::str_check::emit_deferred(ctx);
    v
}

/// RHS lowering for an ident assignment — an ObjectLit entering an
/// `any` slot promotes to the dynobj lane (mirror of `lower_as_cast`'s
/// RFC 20260717-objlit-anylane-recv knife 2 promote and the let-decl
/// P3.2 `let x: any = {...}` route): the struct lane would box an anon
/// static-layout cell into the Any face, and every downstream Any
/// consumer — defineProperty's kernel dynobj walk (silent corruption
/// on the foreign layout), descriptor walks, any-member dispatch —
/// needs the dynobj shape. The init and reassign forms of the same
/// expression must land in the same lane.
/// `binding` is the assigned name, and whether it is a module global.
fn lower_assign_rhs(
    ctx: &mut LowerCtx<'_>,
    slot_ty: Type,
    value: ExprId,
    (name, global): (&str, bool),
) -> Operand {
    // Rotation 630 — an EMPTY array literal mints its block at the
    // SLOT's layout: the assignment face of the anchored empty mint
    // in `ssa_lower_array::lower_no_spread_element`. `lower_empty`
    // otherwise answers an any-flavored block whose ArrId is not the
    // slot's, and the coercion check refused the store outright —
    // `var x = [1, 2]; x = []; x[0] = 5;` (the test262
    // Array/prototype/{join,pop,shift,toString} A1.2 shape) died
    // where bun runs it. An empty block holds no element, so there is
    // no element type to disagree about; a NON-empty literal keeps
    // the loud reject, since its elements are the question.
    if let Type::Arr(sid) = slot_ty
        && matches!(ctx.ast.get_expr(value), Expr::Array(els) if els.is_empty())
    {
        // The FLAVOR follows the layout, not just the ArrId: an
        // `any`-element block has to be minted any-flavored, or the
        // first store through it meets "push through an any[] view
        // would change the typed array's element kind" (`var x = new
        // Array(); x = []; x[0] = 1` — rotation 630's first cut took
        // the typed mint unconditionally and regressed 20 Array cases).
        let alloc = if ctx.arr_layouts[sid.0 as usize] == Type::Any {
            ctx.intrinsics.arr_alloc_any
        } else {
            ctx.intrinsics.arr_alloc
        };
        let cur_block = ctx.cur_block;
        let v = ctx.f.append_inst(
            cur_block,
            InstKind::Call(alloc, vec![Operand::ConstI64(0)]),
            Type::Arr(sid),
            None,
        );
        return Operand::Value(v);
    }
    if slot_ty == Type::Any && matches!(ctx.ast.get_expr(value), Expr::ObjectLit { .. }) {
        let dynobj = ctx.lower_dynobj_init(value);
        return ctx.box_to_any(dynobj);
    }
    // RFC 20260725 follow-up — pin the slot's struct layout for a
    // direct ObjectLit rhs (`s = { a: 9 }` into an Obj global/local):
    // without the hint the literal resolves its own anon layout and
    // the sid mismatch trips the coercion check (mirror of the
    // let-decl chunk 780 hint; consumed take-once).
    if let Type::Obj(sid) = slot_ty
        && matches!(ctx.ast.get_expr(value), Expr::ObjectLit { .. })
    {
        let ann = ctx.binding_ann(name, global);
        ctx.pin_declared_obj(sid, ann.as_deref());
    }
    ctx.lower_expr(value)
}
