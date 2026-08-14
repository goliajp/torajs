//! K.3/K.4 top-level-global and M2 fn-addr let-decl lanes of
//! [`crate::ssa_lower_stmt_let_decl`] (chunk 734 file-size split —
//! chunk 732's mutable re-repr pushed the host past the 500-line
//! hard limit unrecorded; bodies verbatim).
//!
//! - `try_lower_global_let` — main-fn binding whose name registered
//!   as a module global: GlobalRef + Store with the K.6 empty-array
//!   fast path, chunk-698 any-to-typed decode-copy, and the chunk-627
//!   ident-alias borrow-inc admission rules.
//! - `try_lower_fn_addr_let` — `let f = global_fn`: FnSig slot with
//!   FnAddr store (direct-dispatch home for immutable named-fn
//!   references).

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{LocalInfo, LowerCtx};

use crate::ssa_lower_stmt_let_decl::{maybe_any_to_typed_scalar, maybe_arr_any_to_typed};

/// K.3 / K.4 — top-level data global: main fn + name in `globals` emits
/// GlobalRef + Store (K.6 empty-array fast path; refcount slots require
/// fresh-heap init). Returns false when `name` is not a module global.
pub(crate) fn try_lower_global_let(
    ctx: &mut LowerCtx,
    name: &str,
    type_ann: Option<&String>,
    init: ExprId,
) -> bool {
    if !ctx.is_main_fn {
        return false;
    }
    let Some(slot_ty) = ctx.globals.get(name).copied() else {
        return false;
    };
    let init_val = if let Expr::Array(els) = ctx.ast.get_expr(init)
        && els.is_empty()
        && matches!(slot_ty, Type::Arr(_))
    {
        let alloc_fn = if let Type::Arr(arr_id) = slot_ty
            && ctx.arr_layouts[arr_id.0 as usize] == Type::Any
        {
            ctx.intrinsics.arr_alloc_any
        } else {
            ctx.intrinsics.arr_alloc
        };
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(alloc_fn, vec![Operand::ConstI64(0)]),
            slot_ty,
            None,
        );
        Operand::Value(v)
    } else if let Expr::Array(els) = ctx.ast.get_expr(init)
        && let Type::Arr(arr_id) = slot_ty
        && ctx.arr_layouts[arr_id.0 as usize] == Type::Any
    {
        let ids: Vec<ExprId> = els.clone();
        ctx.lower_array_any_literal(&ids)
    } else if slot_ty == Type::Any && matches!(ctx.ast.get_expr(init), Expr::ObjectLit { .. }) {
        // Rotation 204 — mirror of the fn-scope P3.2 arm
        // (`lower_let_init_val`): an ObjectLit init in an Any slot
        // lowers through the dynobj lane, NOT the anon-struct lane —
        // the define/expando write-back contract requires a dynobj
        // cell. The raw Ptr result rides the chunk-809 box station
        // below (Ptr is not refcounted at the SSA level, so no
        // borrow-inc fires; the fresh dynobj's +1 transfers into the
        // tag-4 box and K.4's fresh-heap-init holds).
        ctx.lower_dynobj_init(init)
    } else {
        // Chunk 780 — pin the declared struct layout for a direct
        // ObjectLit init (global-lane mirror of the fn-scope hint;
        // see `let_declared_obj_layout` field doc). Consumed by
        // `resolve_objlit_layout` via take-once.
        if let Type::Obj(sid) = slot_ty
            && matches!(ctx.ast.get_expr(init), Expr::ObjectLit { .. })
        {
            ctx.let_declared_obj_layout = Some(sid);
        }
        ctx.lower_expr(init)
    };
    // Chunk 698 — general-path mirror: an `Array<Any>` init bound
    // to a typed `Array<T>` global decode-copies into a fresh typed
    // block (the global slot takes the fresh cell's only stake, so
    // the borrow-inc below is skipped for converted inits).
    let (init_val, converted) = maybe_arr_any_to_typed(ctx, slot_ty, init, init_val);
    // RFC 20260710 C1, at the GLOBAL position — `undefined` into a
    // sentinel-capable slot binds that width's immortal cell. Without
    // it the init is an `Expr::Ident`, which the borrow gate below
    // reads as an alias of another binding and refuses for a type
    // mismatch (`const a: string | undefined = undefined` was a K.4
    // "init shape is not yet supported"); the same declaration
    // spelled `let` has worked since this rotation's second blade,
    // and the two should not disagree about one line of source.
    //
    // `converted` is the existing channel for "ownership is already
    // settled, skip the borrow gate", and a sentinel qualifies for a
    // reason the other two users do not share: the cell is immortal
    // (FLAG_STATIC_LITERAL — rc and drop are no-ops), so there is no
    // stake to settle. Answers None for Any, which boxes to
    // ANY_UNDEF through the arm below instead.
    let (init_val, converted) = if !converted
        && matches!(
            ctx.expr_types.get(&init),
            Some(crate::check::Type::Undefined)
        )
        && let Some(sentinel) = ctx.str_undef_sentinel_for(slot_ty)
    {
        (sentinel, true)
    } else {
        (init_val, converted)
    };
    // The scalar mirror of the chunk-698 crossing above, and the same
    // one the fn-scope lane has always made: an `any` init under a
    // `number` / `string` / `boolean` annotation decodes at the binding
    // boundary. Missing here, a Call-shaped init (not a borrow shape,
    // so the gate below waved it through) stored the BOX BITS into a
    // `str` global and the next read deref'd them as a Str pointer —
    // `function g(): any { return "lit" }` + `const s: string = g()`
    // was a silent SIGSEGV, while the same line spelled `let` (fn-scope
    // lane) printed `lit`. Runs after the sentinel arm: `undefined`
    // must bind the immortal cell, not ToString into "undefined".
    let (init_val, converted) = if converted {
        (init_val, converted)
    } else {
        maybe_any_to_typed_scalar(ctx, type_ann, slot_ty, init, init_val)
    };
    // Chunk 809 — an Any slot boxes a concrete init value: a
    // borrowed source rc_incs first (`box_to_any` TRANSFERS the
    // stake into the box), so the fresh box is the slot's own ref
    // and K.4's fresh-heap-init holds. Skips the borrow gate below —
    // the box already settled ownership.
    let (init_val, converted) =
        if slot_ty == Type::Any && ctx.operand_ty(&init_val) != Type::Any && !converted {
            let got = ctx.operand_ty(&init_val);
            if got.is_refcounted()
                && matches!(
                    ctx.ast.get_expr(init),
                    Expr::Ident(_) | Expr::Member { .. } | Expr::Index { .. }
                )
            {
                ctx.emit_rc_inc(init_val.clone());
            }
            (ctx.box_to_any_from_expr(init, init_val), true)
        } else {
            (init_val, converted)
        };
    if slot_ty.is_refcounted() && !converted {
        let init_is_borrow = matches!(
            ctx.ast.get_expr(init),
            Expr::Ident(_) | Expr::Member { .. } | Expr::Index { .. }
        );
        if init_is_borrow {
            // RFC 20260707 chunk 627 — Ident-shaped init (alias of
            // another binding, a guaranteed +0 borrow): the global
            // slot takes its own stake; the source binding keeps its
            // own. Only same-type, Arr↔Arr (T-11 container widen)
            // and Any→Arr (chunk 708 — the JS reference-alias shape
            // `const t: number[] = src` with `src: any`; a heap box
            // is the raw cell ptr bits, elem reads are kind-aware,
            // same lane the fn-scope path rides) aliases are
            // admitted — other Any↔concrete mismatches need a
            // box/unbox station and stay loud rather than storing
            // wrong-repr bits. Member/Index inits stay loud too:
            // their ownership shape is lane-dependent (arr_index_get
            // answers +1), so a blanket inc would double-count.
            let got = ctx.operand_ty(&init_val);
            let ident_alias = matches!(ctx.ast.get_expr(init), Expr::Ident(_));
            let compatible = got == slot_ty
                || matches!(
                    (&slot_ty, &got),
                    (Type::Arr(_), Type::Arr(_)) | (Type::Arr(_), Type::Any)
                );
            if !ident_alias || !compatible {
                panic!(
                    "ssa-lower: K.4 refcount global `{name}` requires fresh-heap or same-type ident-alias init; this init shape is not yet supported"
                );
            }
            ctx.emit_owned_result_inc(init_val, got);
        }
    }
    let coerced = if slot_ty == Type::F64 && ctx.operand_ty(&init_val) == Type::I64 {
        ctx.coerce_to_f64(init_val)
    } else {
        init_val
    };
    // RFC 20260707 chunk 627 — a typed array stored into an Arr<Any>
    // global slot (T-11 widen) marks the block's elem kind for the
    // kind-aware Arr<Any> readers (621 let-decl general-path mirror;
    // self-gating no-op for non-array / Arr<Any> values).
    ctx.emit_arr_mark_kind(&coerced);
    let ptr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::GlobalRef(name.to_string()),
        Type::Ptr,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(coerced, Operand::Value(ptr), 0),
    );
    true
}

/// M2 Phase B Stage 4 — `let f = global_fn`: FnSig slot with FnAddr
/// store. Returns false when init is not a bare global-fn ident.
pub(crate) fn try_lower_fn_addr_let(ctx: &mut LowerCtx, name: &str, init: ExprId) -> bool {
    let Expr::Ident(src_name) = ctx.ast.get_expr(init) else {
        return false;
    };
    if ctx.locals.get(src_name).is_some() {
        return false;
    }
    let Some(fid) = ctx.fn_table.get(src_name).copied() else {
        return false;
    };
    let Some(sig_id) = ctx.fn_sig_ids.get(&fid).copied() else {
        return false;
    };
    let ty = Type::FnSig(sig_id);
    let slot = ctx.binding_slot_alloca(ty, name);
    let v = ctx
        .f
        .append_inst(ctx.cur_block, InstKind::FnAddr(fid), ty, None);
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(v), Operand::Value(slot), 0),
    );
    let cur_depth = ctx.scope_stack.len() - 1;
    ctx.locals.insert(
        name.to_string(),
        LocalInfo {
            slot,
            ty,
            moved: false,
            borrowed: false,
            scope_depth: cur_depth,
        },
    );
    let top = ctx.scope_stack.last_mut().expect("scope frame");
    top.push(name.to_string());
    true
}
