//! Class-synthesis register globals
//! (`__torajs_proto_register` / `__torajs_class_register` /
//! `__torajs_register_native_error` / `__torajs_my_class_ref`)
//! pulled out of [`crate::ssa_lower::lower_expr_inner`] `Expr::Call`
//! dispatch as chunk-25 of the `Expr::Call` god-arm decomp (chunks
//! 1-24 = Arr higher-order + Map dispatch + Set dispatch + Arr.push +
//! Number instance methods + bare-name globals + Str regex methods +
//! Number namespace + Array.from + Arr predicate iter + Arr.flatMap +
//! Object.entries + fn-indirect + Number/String/Boolean coercion +
//! universal methods + closure-local + Object.values + Object.keys +
//! Object.getPrototypeOf + Object.assign + Bun runtime cluster +
//! Reflect.get + Symbol.for/keyFor + Object.hasOwn/Reflect.has).
//!
//! Four arms share the same shape:
//! - Bare `Expr::Ident(callee_name)` callee, a known synthesis sentinel
//! - String-literal class-name arg (resolved via `class_name_to_tag`)
//! - Resolves to an `intrinsics` call, falls back to a void / Any-box
//!   on unknown class so misordered toolchain runs don't crash
//!
//! Arms:
//! - `__torajs_proto_register(__proto_<C>, "<C>")` — P4.2 Phase B+C.
//!   synthesize_class_globals emits this at module init; resolve `<C>`
//!   to its runtime tag and call `intrinsics.proto_register`.
//! - `__torajs_class_register(__class_<C>, "<C>")` — P4.5. Same shape
//!   as proto_register, populates the classes-by-tag side table read
//!   by `class_get` inside `__new_<C>` factory bodies for new.target.
//! - `__torajs_register_native_error("<C>")` — P7.4-a-2. Map name to
//!   FIXED runtime-error slot (0=Error / 1=TypeError / 2=RangeError —
//!   NOT a per-program class tag) and to the `__new_<C>` factory
//!   address. Runtime stores the fn-ptr and calls it on a native-error
//!   throw to build a catchable instance. Unknown name / missing
//!   factory / missing sig → silent no-op (runtime falls back to bare
//!   string error).
//! - `__torajs_my_class_ref("<C>")` — P4.5. Emitted by
//!   ast::desugar_classes inside `__new_<C>` factory bodies. Resolves
//!   at lowering time to a runtime `class_get(<tag>)` call returning
//!   the class's Any-box (rc-bumped). Compile-time class name
//!   resolution avoids per-instance side-table lookups when the
//!   factory is class-specific anyway. Unknown tag → ANY_UNDEF Any-box.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Ident(name) = ctx.ast.get_expr(callee) else {
        return None;
    };
    let name = name.clone();
    match name.as_str() {
        "__torajs_proto_register" => try_lower_proto_register(ctx, args),
        "__torajs_class_register" => try_lower_class_register(ctx, args),
        "__torajs_register_native_error" => try_lower_register_native_error(ctx, args),
        "__torajs_my_class_ref" => try_lower_my_class_ref(ctx, args),
        "__torajs_arguments_materialize" => try_lower_arguments_materialize(ctx, args),
        _ => None,
    }
}

/// RFC 20260708-closure-argv-face — expand the synthetic
/// `__torajs_arguments_materialize(__torajs_argv, __torajs_real_argc)`
/// call (desugar_arguments_object prepends it to full-arguments
/// closure bodies) into `arr_alloc_any(argc)` + `arr_any_push(arr,
/// argv, argc, NULL)`. cap == argc so the push never relocates; the
/// runtime incs every stored heap cell (argv slots stay borrowed by
/// the adapter's caller). Result is the fresh Arr<Any> the
/// `__torajs_arguments: any[]` binding owns (scope drop reclaims).
fn try_lower_arguments_materialize(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Option<Operand> {
    if args.len() != 2 {
        return None;
    }
    let argv = ctx.lower_expr(args[0]);
    let argc = ctx.lower_expr(args[1]);
    let arr_id = crate::ssa_lower::intern_arr_layout(ctx.arr_layouts, Type::Any);
    let arr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.arr_alloc_any, vec![argc.clone()]),
        Type::Arr(arr_id),
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.arr_any_push,
            vec![Operand::Value(arr), argv, argc, Operand::ConstPtrNull],
        ),
    );
    Some(Operand::Value(arr))
}

fn try_lower_proto_register(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Option<Operand> {
    if args.len() != 2 {
        return None;
    }
    let Expr::String(cname) = ctx.ast.get_expr(args[1]) else {
        return None;
    };
    let cname = cname.clone();
    let proto_op = ctx.lower_expr(args[0]);
    let cur_block = ctx.cur_block;
    if let Some(tag) = ctx.class_name_to_tag.get(&cname).copied() {
        let proto_register = ctx.intrinsics.proto_register;
        ctx.f.append_void(
            cur_block,
            InstKind::Call(
                proto_register,
                vec![Operand::ConstI64(tag as i64), proto_op],
            ),
        );
    } else {
        // Drop the lowered proto operand if no tag — keeps the path
        // well-typed if it ever fires.
        let proto_ty = ctx.operand_ty(&proto_op);
        ctx.emit_drop_value(proto_op, proto_ty);
    }
    Some(Operand::ConstI64(0))
}

fn try_lower_class_register(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Option<Operand> {
    if args.len() != 2 {
        return None;
    }
    let Expr::String(cname) = ctx.ast.get_expr(args[1]) else {
        return None;
    };
    let cname = cname.clone();
    let class_op = ctx.lower_expr(args[0]);
    let cur_block = ctx.cur_block;
    if let Some(tag) = ctx.class_name_to_tag.get(&cname).copied() {
        let class_register = ctx.intrinsics.class_register;
        ctx.f.append_void(
            cur_block,
            InstKind::Call(
                class_register,
                vec![Operand::ConstI64(tag as i64), class_op],
            ),
        );
    } else {
        let ty = ctx.operand_ty(&class_op);
        ctx.emit_drop_value(class_op, ty);
    }
    Some(Operand::ConstI64(0))
}

fn try_lower_register_native_error(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Option<Operand> {
    if args.len() != 1 {
        return None;
    }
    let Expr::String(cname) = ctx.ast.get_expr(args[0]) else {
        return None;
    };
    let cname = cname.clone();
    let slot: i64 = match cname.as_str() {
        "Error" => 0,
        "TypeError" => 1,
        "RangeError" => 2,
        _ => return Some(Operand::ConstI64(0)),
    };
    let factory = format!("__new_{cname}");
    if let Some(fid) = ctx.fn_table.get(&factory).copied()
        && let Some(sig) = ctx.fn_sig_ids.get(&fid).copied()
    {
        let cur_block = ctx.cur_block;
        let register_native_error = ctx.intrinsics.register_native_error;
        let fnaddr = ctx
            .f
            .append_inst(cur_block, InstKind::FnAddr(fid), Type::FnSig(sig), None);
        ctx.f.append_void(
            cur_block,
            InstKind::Call(
                register_native_error,
                vec![Operand::ConstI64(slot), Operand::Value(fnaddr)],
            ),
        );
    }
    Some(Operand::ConstI64(0))
}

fn try_lower_my_class_ref(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Option<Operand> {
    if args.len() != 1 {
        return None;
    }
    let Expr::String(cname) = ctx.ast.get_expr(args[0]) else {
        return None;
    };
    let cname = cname.clone();
    let cur_block = ctx.cur_block;
    if let Some(tag) = ctx.class_name_to_tag.get(&cname).copied() {
        let class_get = ctx.intrinsics.class_get;
        let v = ctx.f.append_inst(
            cur_block,
            InstKind::Call(class_get, vec![Operand::ConstI64(tag as i64)]),
            Type::Any,
            None,
        );
        return Some(Operand::Value(v));
    }
    // No tag → return ANY_UNDEF Any-box as fallback.
    let any_box = ctx.intrinsics.any_box;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(any_box, vec![Operand::ConstI64(5), Operand::ConstI64(0)]),
        Type::Any,
        None,
    );
    Some(Operand::Value(v))
}
