//! `Object.values(obj)` namespace static method pulled out of
//! [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` dispatch as chunk-17
//! of the `Expr::Call` god-arm decomp (chunks 1-16 = Arr higher-order +
//! Map dispatch + Set dispatch + Arr.push + Number instance methods +
//! bare-name globals + Str regex methods + Number namespace + Array.from +
//! Arr predicate iter + Arr.flatMap + Object.entries + fn-indirect +
//! Number/String/Boolean coercion + universal methods + closure-local).
//!
//! Four receiver type routes (mirror of [`crate::ssa_lower_call_object_entries`]
//! but emitting `Arr<V>` instead of `Arr<[K, V]>` pairs):
//! - `Type::Arr(_)` — fresh shallow clone via `__torajs_arr_slice` + a
//!   per-element rc_inc range when the element type is refcounted. This
//!   reuses the deep-clone pattern used elsewhere for typed-struct Arr-
//!   field copy — production-tested rc balance.
//! - `Type::Str` — per-char Str array (spec §22.1.5.2 + §20.1.2.20) via
//!   `__torajs_str_to_char_arr`, identical to W-M-rest materialize so the
//!   resulting Strs round-trip through console.log / dynobj stores.
//! - `Type::Any` (W-J Phase C2) — `__torajs_anyv_struct_values` runtime
//!   helper; struct identity is only known at runtime, so the helper
//!   reads each field slot and boxes it per its metadata `type_tag`
//!   into an `Arr<Any>`. Non-struct cells throw loudly — propagated via
//!   `emit_throw_check`.
//! - `Type::Obj(struct)` — compile-time unfold using `struct_layouts`.
//!   Homogeneous-field fast path emits one `Load(elem_ty, obj, off)` per
//!   field into an `Arr<elem_ty>`. **S132 heterogeneous-via-`as any`
//!   guard**: when not all field types match `layout[0].1`, the fast
//!   path would emit wrong-typed Loads (e.g. reading a Str ptr through
//!   I64 returns the raw VA as a Number). Detect the mismatch +
//!   box_to_any the receiver + route through the W-J Any-arm walker —
//!   the helper per-field decodes via the field_metadata's type_tag.
//!
//! S256 + S258 + S297 — ES §20.1.2.23 trailing-arg ignore: widens the
//! `args.len() == 1` gate to `>= 1` and lower-and-drops `args[1..]` for
//! spec L-to-R side-effect order (check.rs already typecheck-drops them).
//!
//! Returns `Some(result)` when callee matches `Object.values` with a
//! non-empty args list; non-Arr/Str/Any/Obj receivers panic at SSA
//! lower-time (preserving the original block's behavior). `None` lets
//! the caller fall through to the next arm.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{ARR_DATA_OFF, ARR_LEN_OFF, LowerCtx, OBJ_HEADER_SIZE, intern_arr_layout};

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let (ns_id, m_name) = match ctx.ast.get_expr(callee) {
        Expr::Member { obj, name } => (*obj, name.clone()),
        _ => return None,
    };
    if m_name != "values" {
        return None;
    }
    let Expr::Ident(ns) = ctx.ast.get_expr(ns_id) else {
        return None;
    };
    if ns != "Object" {
        return None;
    }
    // S258 — widen `== 1` → `>= 1` per ES §20.1.2.23 trailing-arg
    // ignore. Lower only args[0]; trailing dropped at lower-time
    // (check.rs S256/S258 extended arm type_of'd them).
    if args.is_empty() {
        return None;
    }
    let arg_op = ctx.lower_expr(args[0]);
    // S297 — lower-and-drop trailing args past the 1 useful obj slot
    // per S258 (S272 idiom). check.rs already type_of'd them.
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    let arg_ty = ctx.operand_ty(&arg_op);

    // W-O — Array receiver: bun returns a fresh shallow array of slot
    // values. Reuses the deep-clone pattern (arr_slice + per-element
    // rc_inc for refcounted elem types) that powers typed-struct Arr-
    // field copy — production-tested rc balance.
    if let Type::Arr(arr_id) = arg_ty {
        let len = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, arg_op, ARR_LEN_OFF),
            Type::I64,
            None,
        );
        let cloned = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.arr_slice,
                vec![arg_op, Operand::ConstI64(0), Operand::Value(len)],
            ),
            Type::Arr(arr_id),
            None,
        );
        let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
        if elem_ty.is_refcounted() {
            let cloned_len = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::I64, Operand::Value(cloned), ARR_LEN_OFF),
                Type::I64,
                None,
            );
            ctx.emit_arr_rc_inc_range(
                Operand::Value(cloned),
                Operand::ConstI64(0),
                Operand::Value(cloned_len),
            );
        }
        return Some(Operand::Value(cloned));
    }

    // W-O-2 — String receiver: per-char Str array (spec §22.1.5.2 +
    // §20.1.2.20). Loops `__torajs_str_at` to mint one fresh Str per
    // code unit; same materialize path as W-M-rest so the resulting
    // Strs round-trip through console.log / dynobj stores.
    if matches!(arg_ty, Type::Str) {
        let arr_id = intern_arr_layout(ctx.arr_layouts, Type::Str);
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.str_to_char_arr, vec![arg_op]),
            Type::Arr(arr_id),
            None,
        );
        return Some(Operand::Value(v));
    }

    // W-J Phase C2 — Any receiver: struct identity is only known at
    // runtime. Route through `__torajs_anyv_struct_values`, which reads
    // each field slot and boxes it per its metadata type_tag into an
    // `Arr<Any>`. A non-struct cell throws loudly inside the helper —
    // propagate it.
    if matches!(arg_ty, Type::Any) {
        let arr_id = intern_arr_layout(ctx.arr_layouts, Type::Any);
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.anyv_struct_values, vec![arg_op]),
            Type::Arr(arr_id),
            None,
        );
        ctx.emit_throw_check(None);
        return Some(Operand::Value(v));
    }

    let Type::Obj(sid) = arg_ty else {
        panic!("ssa-lower: Object.values requires a struct arg, got {arg_ty:?}");
    };
    let layout = ctx.struct_layouts[sid.0 as usize].clone();
    let n = layout.len() as i64;
    let elem_ty = layout[0].1;
    // S132 — heterogeneous-via-`as any` guard. check.rs enforces
    // homogeneous fields when arg type is Struct (5172 reject-loud),
    // but `<typed> as any` makes check see Type::Any (走 Array<Any> arm)
    // while ssa-lower still sees Type::Obj(sid) because `lower_as_cast`
    // is a no-op for refcounted inner types. Falling through the
    // homogeneous fast path (`elem_ty = layout[0].1`) emits a wrong-
    // type Load for every field — `Object.values(mx as any)` where
    // `class Mixed { n: number; s: string }` reads the str ptr through
    // layout[0]=I64 and returns the raw VA as a Number. Detect the
    // mismatch + box the typed operand + route through the W-J walker
    // (the same Any-arm above), which per-field decodes via the
    // field_metadata's type_tag — the correct heterogeneous path.
    if !layout.iter().all(|(_, t)| *t == elem_ty) {
        let boxed = ctx.box_to_any(arg_op);
        let arr_id = intern_arr_layout(ctx.arr_layouts, Type::Any);
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.anyv_struct_values, vec![boxed]),
            Type::Arr(arr_id),
            None,
        );
        ctx.emit_throw_check(None);
        return Some(Operand::Value(v));
    }

    let arr_id = intern_arr_layout(ctx.arr_layouts, elem_ty);
    let arr_ptr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.arr_alloc, vec![Operand::ConstI64(n)]),
        Type::Arr(arr_id),
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::ConstI64(n), Operand::Value(arr_ptr), ARR_LEN_OFF),
    );
    for (i, _) in layout.iter().enumerate() {
        let field_off = OBJ_HEADER_SIZE + (i as u64) * 8;
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(elem_ty, arg_op, field_off),
            elem_ty,
            None,
        );
        let arr_off = ARR_DATA_OFF + (i as u64) * 8;
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(Operand::Value(v), Operand::Value(arr_ptr), arr_off),
        );
    }
    Some(Operand::Value(arr_ptr))
}
