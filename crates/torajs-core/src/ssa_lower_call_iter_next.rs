//! `iter.next()` on a `Type::MapIter` / `Type::ArrIter` receiver
//! pulled out of [`crate::ssa_lower::lower_expr_inner`] `Expr::Call`
//! dispatch as chunk-33 of the `Expr::Call` god-arm decomp
//! (chunks 1-32 = ... + Phase I.1 sibling-class static dispatch).
//!
//! **P6.4b / P6.4c-C3** — receiver-type-peeked dispatch picks the
//! step intrinsic:
//!
//!   - `Type::MapIter` → `__torajs_map_iter_step`
//!   - `Type::ArrIter` → `__torajs_arr_iter_step`
//!
//! Both fill out-slots `(tag, payload)`, return `live: i64` (1 =
//! more values, 0 = end). The arm then synthesizes the spec-shaped
//! `IteratorResult<any> = { value: any, done: boolean }` struct
//! and returns it:
//!
//!   - `value` field — `any_box(tag, payload)` (the runtime
//!     handles ANY_HEAP rc_inc; for non-live, tag is ANY_UNDEF and
//!     box wraps undefined)
//!   - `done` field — `live == 0`
//!
//! The struct layout is interned into `ctx.struct_layouts` matching
//! the layout `check.rs`'s `(Type::MapIter, "next")` return shape,
//! so the `StructId` aligns with the caller's typing.
//!
//! Returns `Some(op)` on hit; `None` on miss (callee not a
//! Member-call, method name not `"next"`, args non-empty, receiver
//! not a `Type::MapIter` / `Type::ArrIter` local) so the caller
//! falls through to the next arm.

use crate::ast::PropKey;
use crate::ast::{Expr, ExprId};
use crate::ssa;
use crate::ssa::{IPred, InstKind, Operand, Type};
use crate::ssa_lower::{LowerCtx, OBJ_CLASS_TAG_OFF, OBJ_HEADER_SIZE, OBJ_VTABLE_OFF};

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Member { obj, name } = ctx.ast.get_expr(callee) else {
        return None;
    };
    // Rotation 545 — args are lowered-and-dropped, not a miss:
    // %MapIteratorPrototype%.next / %ArrayIteratorPrototype%.next
    // take no parameters and ignore extras (eval-then-discard, the
    // S272 trailing idiom). The load-bearing case is not even a user
    // spelling: a module containing any generator registers a 1-param
    // `next` in `apply_default_args`' name-keyed table, which pads
    // `m.next()` on an unresolvable receiver to `m.next(<default>)` —
    // the old `!args.is_empty()` bail then dropped the whole claim
    // and the cascade's terminal panicked on a call the compiler
    // wrote itself.
    if name != "next" {
        return None;
    }
    // Prefer the checker's inferred type on the receiver expression so
    // chained shapes like `xs.values().next()` (receiver is Call, not
    // Ident) resolve — the Ident-only fallback still covers via-var
    // usage and pre-checker code paths.
    let step_fid = match ctx.expr_types.get(obj) {
        Some(crate::check::Type::MapIter) => Some(ctx.intrinsics.map_iter_step),
        Some(crate::check::Type::ArrIter) => Some(ctx.intrinsics.arr_iter_step),
        _ => {
            let recv_ty_hint = match ctx.ast.get_expr(*obj) {
                Expr::Ident(n) => ctx.locals.get(n).map(|info| info.ty),
                _ => None,
            };
            match recv_ty_hint {
                Some(Type::MapIter) => Some(ctx.intrinsics.map_iter_step),
                Some(Type::ArrIter) => Some(ctx.intrinsics.arr_iter_step),
                _ => None,
            }
        }
    }?;

    let recv_id = *obj;
    let recv_op = ctx.lower_expr(recv_id);
    crate::ssa_lower_nullable_guard::emit_undefable_heap_guard(ctx, recv_id, &recv_op);
    // Eval-then-discard the ignored arguments (spec order: after the
    // receiver; they evaluate even though the built-in next never
    // reads them).
    for &a in args {
        let _ = ctx.lower_expr(a);
    }
    let tag_slot = ctx.alloca(Type::I64, Some("__iter_tag"));
    let val_slot = ctx.alloca(Type::I64, Some("__iter_val"));
    let cur_block = ctx.cur_block;
    let live = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            step_fid,
            vec![
                recv_op.clone(),
                Operand::Value(tag_slot),
                Operand::Value(val_slot),
            ],
        ),
        Type::I64,
        None,
    );
    let tag_v = ctx.f.append_inst(
        cur_block,
        InstKind::Load(Type::I64, Operand::Value(tag_slot), 0),
        Type::I64,
        None,
    );
    let val_v = ctx.f.append_inst(
        cur_block,
        InstKind::Load(Type::I64, Operand::Value(val_slot), 0),
        Type::I64,
        None,
    );
    let value_box = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.any_box,
            vec![Operand::Value(tag_v), Operand::Value(val_v)],
        ),
        Type::Any,
        None,
    );
    // The receiver is only borrowed for the step, so an owned-temp
    // receiver has to be released here — a chained `xs.entries()
    // .next()` mints a fresh iterator per evaluation and nothing else
    // ever drops it (300k chained steps held 24MB against a 6MB flat
    // baseline; the same loop over a hoisted `it` was already flat,
    // which is what pinned it to the receiver rather than to the
    // IteratorResult). An Ident receiver is a slot borrow and
    // `release_owned_temp` no-ops on it.
    //
    // After the value box, not before: for VALUES the out-payload is a
    // borrowed element of the iterator's source array, so the release
    // must not be the thing that frees the source out from under a
    // payload nobody has adopted yet.
    ctx.release_owned_temp(recv_id, &recv_op);
    let done_bool = ctx.f.append_inst(
        cur_block,
        InstKind::ICmp(IPred::Eq, Operand::Value(live), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );

    // Intern IteratorResult<any> struct layout (matches check.rs's
    // (Type::MapIter, "next") / (Type::ArrIter, "next") return
    // shape: `{ value: any, done: boolean }`).
    let iter_result_fields: Vec<(PropKey, Type)> =
        vec![("value".into(), Type::Any), ("done".into(), Type::Bool)];
    let sid = match ctx
        .struct_layouts
        .iter()
        .position(|f| f == &iter_result_fields)
    {
        Some(i) => ssa::StructId(i as u32),
        None => {
            let new_id = ssa::StructId(ctx.struct_layouts.len() as u32);
            ctx.struct_layouts.push(iter_result_fields);
            new_id
        }
    };
    let size = (OBJ_HEADER_SIZE as i64) + 2 * 8;
    let obj_ptr = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.obj_alloc, vec![Operand::ConstI64(size)]),
        Type::Obj(sid),
        None,
    );
    ctx.emit_obj_header_init(Operand::Value(obj_ptr));
    // Rotation 545 — anon-stamp the synthesized IteratorResult.
    // The old `class_tag = 0` stamp fell out of every reflection
    // consumer: `struct_layout_lookup(0)` is NULL, so the Tag::Obj
    // print walker answered `{}` for `console.log(it.next())` where
    // bun prints `{ value: …, done: … }` (the exact fall-through
    // `ssa_lower_anon_stamp`'s module doc names).
    let class_tag = ctx.anon_stamp_pool.borrow_mut().assign_or_get(sid);
    ctx.f.append_void(
        cur_block,
        InstKind::Store(
            Operand::ConstI64(i64::from(class_tag)),
            Operand::Value(obj_ptr),
            OBJ_CLASS_TAG_OFF,
        ),
    );
    // vtable = null.
    ctx.f.append_void(
        cur_block,
        InstKind::Store(
            Operand::ConstPtrNull,
            Operand::Value(obj_ptr),
            OBJ_VTABLE_OFF,
        ),
    );
    // Field 0 — "value" at OBJ_HEADER_SIZE.
    ctx.f.append_void(
        cur_block,
        InstKind::Store(
            Operand::Value(value_box),
            Operand::Value(obj_ptr),
            OBJ_HEADER_SIZE,
        ),
    );
    // Field 1 — "done" at OBJ_HEADER_SIZE + 8.
    ctx.f.append_void(
        cur_block,
        InstKind::Store(
            Operand::Value(done_bool),
            Operand::Value(obj_ptr),
            OBJ_HEADER_SIZE + 8,
        ),
    );
    Some(Operand::Value(obj_ptr))
}
