//! `emit_drop_value` extracted from [`crate::ssa_lower`] (chunk 159).
//!
//! Pre-extract this method was 358 LOC on `LowerCtx`. Becomes a
//! free fn here; `LowerCtx::emit_drop_value` becomes a thin
//! pub(crate) wrapper because the body recurses on itself (Obj
//! field walk) + many ssa-lower sites call it as a method.
//!
//! Per-type drop dispatch — emit the IR for releasing one owned
//! value at scope close. Bodies preserved verbatim:
//!
//! - **Str / Substr** → simple `str_drop` / `substr_drop` call
//!   (refcount + parent chain handled in the runtime helper).
//! - **Obj(sid)** — V3-05 self-referential class layouts: first
//!   inline frame inserts `sid` into `drop_inline_stack`;
//!   recursive children of the same sid route through the runtime
//!   tag-dispatched `value_drop_heap` instead. Body inlines
//!   `if (val != null) { if (--rc == 0) { walk_fields; free } }`:
//!   - NULL guard (nullable Obj patterns)
//!   - `emit_rc_dec_inline` returns new rc
//!   - T-26.C: class-sid hits `cycle_buffer` when rc didn't reach 0
//!     (Bacon-Rajan collector cycle root); buffered flag in runtime
//!     prevents dup-buffering
//!   - On rc==0: T-26 clear WeakRefs for class instances,
//!     recurse on owned fields, T-26.B `cycle_unbuffer` scrub
//!     (skipped for anon-structs to keep generic-pair-1m fast)
//!   - Sized `obj_drop_sized(val, OBJ_HEADER_SIZE + N*8)`
//! - **Arr(arr_id)** — NULL guard (regex/match no-match yields null),
//!   then per-elem-ty path: Any → `arr_drop_any` (16-byte tagged
//!   slots); refcounted elems → `emit_arr_rc_drop_range` to dec
//!   each element, then `arr_drop`.
//! - **Closure** — rc-gated (chunk 553): inline dec, on hit-zero
//!   load drop_fn ptr from `CLOSURE_DROP_FN_OFF` and indirect-call;
//!   the synthesized drop fn (Pass 2.5, no rc gate of its own) walks
//!   env captures and frees the env block.
//! - **RegExp / Date / Any / Symbol / Promise / BigInt / WeakRef /
//!   WeakMap / WeakSet / Map / Set / MapIter / ArrIter** — all
//!   route through their type-specific `__torajs_*_drop` runtime
//!   helper (rc-aware: dec, free at zero, recursive child drops as
//!   needed).
//! - **Copy** types — no-op (caller normally filters; defensive).
//! - other — panic.

use crate::ssa::{BinOp as SsaBinOp, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{ARR_LEN_OFF, CLOSURE_DROP_FN_OFF, LowerCtx, intern_fn_sig};

mod obj;

/// RFC 20260710 C2b — nullish skip condition for the inline drop
/// stations: a refcounted pointer slot legitimately holds NULL (JS
/// null) or the immortal generic undefined cell; both must skip the
/// inline rc-dec (the runtime FFI paths are FLAG_STATIC-gated, but
/// the IR-level `emit_rc_dec_inline` writes the header directly —
/// dec'ing the static cell would write rodata). Two cmps + or.
pub(super) fn nullish_skip_cond(ctx: &mut LowerCtx, val: &Operand) -> Operand {
    let val_ty = ctx.operand_ty(val);
    let is_null = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Eq, val.clone(), Operand::ConstPtrNull),
        Type::Bool,
        None,
    );
    let sentinel = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::GlobalRef(crate::ssa_lower_binop_null_undef::UNDEF_CELL_SYM.to_string()),
        val_ty,
        None,
    );
    let is_undef = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Eq, val.clone(), Operand::Value(sentinel)),
        Type::Bool,
        None,
    );
    let nullish = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(
            SsaBinOp::Or,
            Operand::Value(is_null),
            Operand::Value(is_undef),
        ),
        Type::Bool,
        None,
    );
    Operand::Value(nullish)
}

pub(crate) fn emit(ctx: &mut LowerCtx, val: Operand, ty: Type) {
    // three structural shapes get their own emit fn; everything else
    // is a single type-specific `__torajs_*_drop` helper call.
    let drop_fid = match ty {
        Type::Obj(sid) => return obj::emit_drop_obj(ctx, val, sid),
        Type::Arr(arr_id) => return emit_drop_arr(ctx, val, arr_id),
        Type::Closure(_) => return emit_drop_closure(ctx, val),
        Type::Str => ctx.intrinsics.str_drop,
        Type::Substr => ctx.intrinsics.substr_drop,
        Type::RegExp => ctx.intrinsics.regex_drop,
        Type::Date => ctx.intrinsics.date_drop,
        Type::Any => ctx.intrinsics.any_box_drop,
        Type::Symbol => ctx.intrinsics.symbol_drop,
        Type::Promise => ctx.intrinsics.promise_drop,
        Type::BigInt => ctx.intrinsics.bigint_drop_rc,
        Type::WeakRef => ctx.intrinsics.weakref_drop,
        Type::WeakMap => ctx.intrinsics.weakmap_drop,
        Type::WeakSet => ctx.intrinsics.weakset_drop,
        Type::Map | Type::Set => ctx.intrinsics.map_drop,
        Type::MapIter => ctx.intrinsics.map_iter_drop,
        Type::ArrIter => ctx.intrinsics.arr_iter_drop,
        other if other.is_copy() => return,
        other => panic!("ssa-lower: no drop sequence for type {other:?}"),
    };
    ctx.f
        .append_void(ctx.cur_block, InstKind::Call(drop_fid, vec![val]));
}

/// `Type::Arr` drop — NULL guard (regex/match no-match yields null),
/// then Any-slot / refcounted-elem / plain paths.
fn emit_drop_arr(ctx: &mut LowerCtx, val: Operand, arr_id: crate::ssa::ArrId) {
    let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
    let body_blk = ctx.f.add_block();
    let after = ctx.f.add_block();
    let skip_check = nullish_skip_cond(ctx, &val);
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: skip_check,
            then_blk: after,
            else_blk: body_blk,
        },
    );
    ctx.cur_block = body_blk;
    // RFC 20260821 A2 — Str/Substr elements go out through one call.
    // The SSA-emitted per-slot walk cost more in loop bookkeeping than
    // in the calls it made (measured: 0.7 ms of bookkeeping against
    // 0.4 ms of calls on `split-only-100k`), so emitting a cheaper
    // per-slot body could not have fixed it; the loop itself is what
    // moves into the kernel. The `rc == 1` gate and the trailing
    // `arr_drop` move with it.
    if matches!(elem_ty, Type::Str | Type::Substr) {
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.arr_drop_str_elems, vec![val]),
        );
        ctx.f.set_term(ctx.cur_block, Terminator::Br(after));
        ctx.cur_block = after;
        return;
    }
    if elem_ty == Type::Any {
        let drop_fid = ctx.intrinsics.arr_drop_any;
        ctx.f
            .append_void(ctx.cur_block, InstKind::Call(drop_fid, vec![val]));
    } else {
        if elem_ty.is_refcounted() {
            // RFC 20260704 S6 — element references belong to the
            // array block, so only the LAST owner walks them (rc==1
            // here means the arr_drop below is the hit-zero dec).
            // When the array also crossed into `any` (any_box inc'd
            // the block) and the Any reference outlives this site,
            // the runtime walker (`value_drop_heap` Tag::Arr →
            // `arr_drop_heap` via the elem-kind header field) walks
            // instead; the unconditional pre-S6 walk would leave the
            // Any side's slots dangling.
            let rc_now = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::I32, val.clone(), 0),
                Type::I32,
                None,
            );
            let is_last = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::ICmp(IPred::Eq, Operand::Value(rc_now), Operand::ConstI32(1)),
                Type::Bool,
                None,
            );
            let walk_blk = ctx.f.add_block();
            let walk_done = ctx.f.add_block();
            ctx.f.set_term(
                ctx.cur_block,
                Terminator::CondBr {
                    cond: Operand::Value(is_last),
                    then_blk: walk_blk,
                    else_blk: walk_done,
                },
            );
            ctx.cur_block = walk_blk;
            let len_v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::I64, val.clone(), ARR_LEN_OFF),
                Type::I64,
                None,
            );
            ctx.emit_arr_rc_drop_range(
                val.clone(),
                elem_ty,
                Operand::ConstI64(0),
                Operand::Value(len_v),
            );
            ctx.f.set_term(ctx.cur_block, Terminator::Br(walk_done));
            ctx.cur_block = walk_done;
        }
        // r500 A4' — a scalar-kind array (the any lane coerces or
        // refuses a kind-mismatched store, it never re-kinds) takes
        // the kernel with no element walk and no cycle-buffer hook.
        let drop_fid = if matches!(elem_ty, Type::I64 | Type::I32 | Type::F64 | Type::Bool) {
            ctx.intrinsics.arr_drop_scalar
        } else {
            ctx.intrinsics.arr_drop
        };
        ctx.f
            .append_void(ctx.cur_block, InstKind::Call(drop_fid, vec![val]));
    }
    ctx.f.set_term(ctx.cur_block, Terminator::Br(after));
    ctx.cur_block = after;
}

/// `Type::Closure` drop — rc-gated release: dec the env header's
/// refcount inline and only on hit-zero load the synthesized drop fn
/// ptr from `CLOSURE_DROP_FN_OFF` and indirect-call it. Mirrors
/// `__torajs_value_drop_heap`'s `Tag::Closure` arm — the synthesized
/// `__env_drop_*` body carries no rc gate of its own, so the dec gate
/// lives at every release site.
///
/// RFC 20260705 chunk 553 — the pre-553 direct finalizer call assumed
/// typed sites own the env's single reference. A closure passed
/// through a `__cls` param and returned (`keep(f) { return f }`)
/// legitimately carries rc > 1 (the return retain + the caller's arg
/// temp); the unconditional finalize freed the shared env from under
/// the live binding and the next alloc aliased it.
///
/// Chunk 738 — NULL guard mirrors `emit_drop_obj`: a nullable
/// closure slot (`let h: (() => T) | null`) legitimately holds the
/// in-band null sentinel; the reassign drop-old and scope-close
/// paths must not rc-dec through it. RFC 20260710 C2b upgraded the
/// guard to nullish (NULL or the generic undefined cell).
fn emit_drop_closure(ctx: &mut LowerCtx, val: Operand) {
    let flags_blk = ctx.f.add_block();
    let dec_blk = ctx.f.add_block();
    let skip = ctx.f.add_block();
    let skip_check = nullish_skip_cond(ctx, &val);
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: skip_check,
            then_blk: skip,
            else_blk: flags_blk,
        },
    );
    // An immortal static cell (ns-static / reified builtin-method —
    // FLAG_STATIC_LITERAL) skips the whole dec: its drop_fn slot is
    // 0, so letting the inline dec reach 0 jumps to address zero
    // (`Object.getPrototypeOf(Math.max)` crashed exactly there — the
    // non-ident arg's temp release was the first consumer to inline-
    // drop one of these cells). Header +4 packs tag:u16 | flags:u16.
    ctx.cur_block = flags_blk;
    let tag_flags = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I32, val.clone(), 4),
        Type::I32,
        None,
    );
    let static_bit = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(
            SsaBinOp::And,
            Operand::Value(tag_flags),
            Operand::ConstI32((torajs_rc::FLAG_STATIC_LITERAL as i32) << 16),
        ),
        Type::I32,
        None,
    );
    let is_static = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Ne, Operand::Value(static_bit), Operand::ConstI32(0)),
        Type::Bool,
        None,
    );
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(is_static),
            then_blk: skip,
            else_blk: dec_blk,
        },
    );
    ctx.cur_block = dec_blk;
    let rc_new = ctx.emit_rc_dec_inline(val.clone());
    let is_zero = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Eq, rc_new, Operand::ConstI32(0)),
        Type::Bool,
        None,
    );
    let drop_blk = ctx.f.add_block();
    let after = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(is_zero),
            then_blk: drop_blk,
            else_blk: after,
        },
    );
    ctx.cur_block = drop_blk;
    let drop_fn_ptr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Ptr, val.clone(), CLOSURE_DROP_FN_OFF),
        Type::Ptr,
        None,
    );
    let drop_void_sig = intern_fn_sig(ctx.fn_sigs, vec![Type::Ptr], Type::Void);
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::CallIndirect(drop_void_sig, Operand::Value(drop_fn_ptr), vec![val]),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(after));
    ctx.f.set_term(skip, Terminator::Br(after));
    ctx.cur_block = after;
}
