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
//!    have no old-value drop dance; Str (chunk 558) / Obj (RFC
//!    20260725) / Arr (K.6 close, rotation 253 — B1 fixed the cell
//!    across growth so method mutation needs no slot writeback) run
//!    the borrow-inc → load-old → store-new → drop-old sequence;
//!    remaining refcount slot types are rejected loudly.
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
//!    Drop old slot value if non-Copy AND not moved out by RHS
//!    consume. Store new value; clear `moved` flag so subsequent
//!    reads work and end-of-fn drop fires.
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
//! borrow-needs-inc whitelist (`apply_borrow_rc_inc` here, the
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

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn lower(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    target: ExprId,
    name: String,
    value: ExprId,
) -> Operand {
    if ctx.ast.undeclared_reads.contains_key(&target) {
        // The throw makes the value unobservable and the lane past
        // the check is unreachable — nothing to hand a consumer.
        return lower_undeclared_write_throw(ctx, &name, value);
    }
    let v = if ctx.locals.get(&name).is_none()
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
/// owned. See the module doc for why this belongs here rather than in
/// each consumer's borrow whitelist.
///
/// Copy-typed slots (`i64`, `bool`, …) mint nothing: `emit_rc_inc`
/// has no work to do and `release_owned_temp` drops out on
/// `ty.is_copy()` before reaching a release site, so recording the
/// eid stays inert for them.
fn mint_consumer_stake(ctx: &mut LowerCtx<'_>, eid: ExprId, v: &Operand) {
    if !ctx.operand_ty(v).is_refcounted() {
        return;
    }
    ctx.emit_rc_inc(*v);
    ctx.owned_member_reads.insert(eid);
}

/// RFC 20260730-undeclared-ident, write position — §6.2.5.6 PutValue
/// on an unresolvable Reference (the checker marked the target eid).
/// The RHS lowers first (§13.15.2 rref before PutValue) and its value
/// drops — the throw makes it unobservable — then the ReferenceError
/// raiser fires with the same `<name> is not defined` kernel the read
/// side uses. The lane past the throw-check is unreachable;
/// `undefined`'s shape stands in so the enclosing expression still
/// types out.
fn lower_undeclared_write_throw(ctx: &mut LowerCtx<'_>, name: &str, value: ExprId) -> Operand {
    let v = ctx.lower_expr(value);
    let v_ty = ctx.operand_ty(&v);
    if !v_ty.is_copy() {
        ctx.emit_drop_value(v, v_ty);
    }
    let name_str = ctx.intern_string_literal(name);
    let raiser = ctx.intrinsics.throw_reference_error_name;
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(raiser, vec![Operand::Value(name_str)]),
    );
    ctx.emit_throw_check(None);
    Operand::ConstPtrNull
}

fn lower_global_assign(
    ctx: &mut LowerCtx<'_>,
    name: String,
    slot_ty: Type,
    value: ExprId,
) -> Operand {
    // Chunk 809 — Any slots ride the same drop-old/store-new
    // sequence (the old box releases through emit_drop_value's Any
    // arm; a concrete rhs boxes below). RFC 20260725 follow-up —
    // Obj slots join: a struct cell has no reallocating method
    // surface (field writes are in-place), so drop-old/store-new is
    // the complete mutation story.
    // Cluster #4 follow-up (rotation 235) — Symbol joins: no
    // in-place mutation surface (Str profile), a fresh `Symbol()`
    // rhs is an owned mint and transfers.
    // K.6 close (rotation 253) — Arr joins: B1 fixed the cell
    // across growth (push/grow realloc only the spilled data buffer
    // and return the same cell), so method mutation needs no slot
    // writeback and drop-old/store-new is the complete reassignment
    // story here too (emit_drop_value's Arr arm walks elements).
    let drop_old_slot = slot_ty == Type::Str
        || matches!(slot_ty, Type::Closure(_))
        || slot_ty == Type::Any
        || matches!(slot_ty, Type::Obj(_))
        || matches!(slot_ty, Type::Arr(_))
        || slot_ty == Type::Symbol;
    if slot_ty.is_refcounted() && !drop_old_slot {
        panic!("ssa-lower: assignment to refcount global `{name}` is not yet supported (K.6)");
    }
    let v = lower_assign_rhs(ctx, slot_ty, value);
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
    if drop_old_slot {
        apply_borrow_rc_inc(ctx, &v, value);
    }
    let v_ty = ctx.operand_ty(&v);
    if !global_coercion_compatible(slot_ty, v_ty) {
        panic!(
            "ssa-lower: assignment to global `{name}` mismatch — slot is {slot_ty:?} but value is {v_ty:?}; use `>>` for integer divide or annotate the slot as the appropriate numeric width",
        );
    }
    // Chunk 809 — a concrete rhs boxes into the Any slot (the
    // borrow inc above supplied the stake the box transfer
    // consumes; expr-aware so undefined/null keep their tags).
    let v = if slot_ty == Type::Any && v_ty != Type::Any {
        ctx.box_to_any_from_expr(value, v)
    } else {
        coerce_for_global(ctx, slot_ty, v_ty, v)
    };
    let cur_block = ctx.cur_block;
    let ptr = ctx
        .f
        .append_inst(cur_block, InstKind::GlobalRef(name), Type::Ptr, None);
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
        return v;
    }
    let cur_block = ctx.cur_block;
    ctx.f
        .append_void(cur_block, InstKind::Store(v, Operand::Value(ptr), 0));
    let cur_block = ctx.cur_block;
    let r = ctx.f.append_inst(
        cur_block,
        InstKind::Load(slot_ty, Operand::Value(ptr), 0),
        slot_ty,
        None,
    );
    Operand::Value(r)
}

fn global_coercion_compatible(slot_ty: Type, v_ty: Type) -> bool {
    v_ty == slot_ty
        // chunk 809 — an Any slot admits everything (boxed at the caller)
        || slot_ty == Type::Any
        || (slot_ty == Type::F64 && v_ty == Type::I64)
        || (slot_ty == Type::I64 && v_ty == Type::I32)
        || (slot_ty == Type::I32 && v_ty == Type::I64)
        || slot_ty == Type::Ptr
        || v_ty == Type::Ptr
        || (matches!(slot_ty, Type::I64 | Type::F64) && v_ty == Type::Any)
        || (slot_ty == Type::Str && v_ty == Type::Any)
        // L3b #4 — shorter-arity closures fit wider fn-typed slots
        // (see check_local_coercion's Closure arm).
        || (matches!(slot_ty, Type::Closure(_)) && matches!(v_ty, Type::Closure(_)))
}

fn coerce_for_global(ctx: &mut LowerCtx<'_>, slot_ty: Type, v_ty: Type, v: Operand) -> Operand {
    if slot_ty == Type::F64 && v_ty == Type::I64 {
        ctx.coerce_to_f64(v)
    } else if matches!(slot_ty, Type::I64 | Type::F64) && v_ty == Type::Any {
        ctx.coerce_any_to_number(v, slot_ty)
    } else if slot_ty == Type::Str && v_ty == Type::Any {
        ctx.coerce_to_str(v, Type::Any)
    } else {
        v
    }
}

fn lower_local_assign(ctx: &mut LowerCtx<'_>, name: String, value: ExprId) -> Operand {
    let snapshot = match ctx.locals.get(&name) {
        Some(i) => *i,
        None => panic!("ssa-lower: assign to unknown ident `{name}`"),
    };
    let v = lower_assign_rhs(ctx, snapshot.ty, value);
    apply_borrow_rc_inc(ctx, &v, value);
    let v_ty = ctx.operand_ty(&v);
    check_local_coercion(ctx, &name, snapshot.ty, v_ty);
    let v = coerce_for_local(ctx, snapshot.ty, v_ty, v, value);
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
fn lower_assign_rhs(ctx: &mut LowerCtx<'_>, slot_ty: Type, value: ExprId) -> Operand {
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
        ctx.let_declared_obj_layout = Some(sid);
    }
    ctx.lower_expr(value)
}

fn apply_borrow_rc_inc(ctx: &mut LowerCtx<'_>, v: &Operand, value: ExprId) {
    let v_is_refcounted = ctx.operand_ty(v).is_refcounted();
    if !v_is_refcounted {
        return;
    }
    let needs_inc = match ctx.ast.get_expr(value) {
        // String indexing emits a fresh owned Substr view, not an
        // element borrow — inc'ing it here left the extra reference
        // undropped (chunk 561, mirror of the let-decl alias fix).
        // Chunk 717 — a literal-key any-member read (`o["k"]`)
        // answers owned the same way; its eid is recorded in
        // `owned_member_reads`.
        Expr::Index { obj, .. } => {
            !matches!(ctx.expr_types.get(obj), Some(crate::check::Type::String))
                && !ctx.owned_member_reads.contains(&value)
        }
        // Chunk 637 — a Member read whose owned-receiver lowering
        // detached the result already carries this consumer's stake;
        // inc'ing again would strand the original (mirror of the
        // let-decl alias re-check).
        Expr::Member { .. } => !ctx.owned_member_reads.contains(&value),
        // Reading a named binding for an assignment is always a SHARE
        // (TS has no move semantics): the source keeps its stake and
        // stays readable, the target takes +1. This holds for alias
        // and previously-consumed bindings too — their cell is alive
        // (the canonical owner holds it), so the inc mints the
        // target's own stake (chunk 564; replaced the consume-then-
        // skip-inc transfer that let the target's drop-old steal the
        // source's stake — cross-scope UAF, asm-proven). Non-binding
        // Idents (lifted closures, fn names) mint owned values and
        // keep transferring. Global-slot reads borrow the same way
        // (chunk 558).
        Expr::Ident(src) => ctx.locals.contains_key(src) || ctx.globals.contains_key(src),
        // Hoisted regex-literal singleton (fn-scope LICM,
        // `ssa_lower_lit::lower_regex`) — the slot takes a share of
        // the fn-owned compile; without the +1 the slot's drop-old
        // stole the fn's stake (UAF on the next occurrence).
        Expr::Regex { .. } => true,
        _ => false,
    };
    if needs_inc {
        ctx.emit_rc_inc(*v);
    }
}

fn check_local_coercion(ctx: &LowerCtx<'_>, name: &str, snap_ty: Type, v_ty: Type) {
    if snap_ty == Type::Any && v_ty != Type::Any {
        return;
    }
    let direct_compatible = v_ty == snap_ty
        || (snap_ty == Type::F64 && v_ty == Type::I64)
        || (matches!(snap_ty, Type::I64 | Type::F64) && v_ty == Type::Any)
        || (snap_ty == Type::Str && v_ty == Type::Any)
        || (snap_ty == Type::Str && v_ty == Type::Substr);
    if direct_compatible {
        return;
    }
    let int_or_ptr_lax = (snap_ty == Type::I64 && v_ty == Type::I32)
        || (snap_ty == Type::I32 && v_ty == Type::I64)
        || snap_ty == Type::Ptr
        || v_ty == Type::Ptr;
    if int_or_ptr_lax {
        return;
    }
    // L3b #4 — a shorter-arity closure fits a wider fn-typed slot
    // (the checker's callback-subtype lattice is the sole admit
    // gate): both are env cells, the call site pushes the SLOT sig's
    // args and the callee reads its own shorter prefix, extra arg
    // registers are simply never read.
    if matches!(snap_ty, Type::Closure(_)) && matches!(v_ty, Type::Closure(_)) {
        return;
    }
    let _ = ctx;
    panic!(
        "ssa-lower: assignment to `{name}` mismatch — slot is {snap_ty:?} but value is {v_ty:?}; use `>>` for integer divide or annotate the slot as the appropriate numeric width",
    );
}

fn coerce_for_local(
    ctx: &mut LowerCtx<'_>,
    snap_ty: Type,
    v_ty: Type,
    v: Operand,
    value: ExprId,
) -> Operand {
    if snap_ty == Type::Any && v_ty != Type::Any {
        // S2.28 (RFC 20260727-dstr-assignment) — the eid-aware box,
        // same as the global lane and the let-init lane: a typed OOB
        // element read (`b = t[i]` past the end) carries the
        // undefined sentinel, and the eid-blind `box_to_any` was
        // boxing its raw bits as a Number (`let b: any = 0;
        // b = t[1];` answered NaN). The expr gate also keeps
        // `undefined` / `null` rhs tags apart, as the global path
        // has since chunk 809.
        ctx.box_to_any_from_expr(value, v)
    } else if snap_ty == Type::F64 && v_ty == Type::I64 {
        ctx.coerce_to_f64(v)
    } else if matches!(snap_ty, Type::I64 | Type::F64) && v_ty == Type::Any {
        ctx.coerce_any_to_number(v, snap_ty)
    } else if snap_ty == Type::Str && v_ty == Type::Any {
        ctx.coerce_to_str(v, Type::Any)
    } else if snap_ty == Type::Str && v_ty == Type::Substr {
        // Chunk 561 — a Substr view assigned into an owned-Str slot
        // materializes (mirror of materialize_call_args); the view
        // reference (fresh from string indexing, or the +1 an alias
        // rhs took above) releases after the copy, so both shapes
        // balance.
        let cur_block = ctx.cur_block;
        let owned = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.substr_to_owned, vec![v.clone()]),
            Type::Str,
            None,
        );
        ctx.emit_drop_value(v, Type::Substr);
        Operand::Value(owned)
    } else {
        v
    }
}
