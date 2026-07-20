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
        "__torajs_error_proto_install" => try_lower_error_proto_install(ctx, args),
        "__torajs_error_is_error" => try_lower_error_is_error(ctx, args),
        "__torajs_static_method_reify" => try_lower_static_method_reify(ctx, args),
        "__torajs_class_accessor_reify" => try_lower_class_accessor_reify(ctx, args, false),
        "__torajs_class_static_accessor_reify" => try_lower_class_accessor_reify(ctx, args, true),
        "__torajs_register_native_error" => try_lower_register_native_error(ctx, args),
        "__torajs_undef_str" => crate::ssa_lower_call_error_magic::try_lower_undef_str(ctx, args),
        "__torajs_ctor_no_super_throw" => {
            crate::ssa_lower_call_error_magic::try_lower_ctor_no_super_throw(ctx, args)
        }
        "__torajs_error_stack" => {
            crate::ssa_lower_call_error_magic::try_lower_error_stack(ctx, args)
        }
        "__torajs_my_class_ref" => try_lower_my_class_ref(ctx, args),
        "__torajs_arguments_materialize" => try_lower_arguments_materialize(ctx, args),
        "__torajs_genfn_chain" => try_lower_genfn_chain(ctx, args),
        _ => None,
    }
}

/// RFC 20260713 blade 5 cut 4 —
/// `__torajs_genfn_chain(__proto_<cls>, <kind>)`, emitted by
/// synthesize_class_globals for each generator class: chains the
/// per-generator prototype object to the shared %GeneratorPrototype%
/// of `kind` (0 = generator, 1 = async generator). The proto operand
/// is a borrow (runtime reads + writes its `__proto__` slot; the
/// module-scope binding keeps the ref).
fn try_lower_genfn_chain(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Option<Operand> {
    if args.len() != 2 {
        return None;
    }
    let Expr::Number(kind) = ctx.ast.get_expr(args[1]) else {
        return None;
    };
    let kind = *kind as i64;
    let proto_op = ctx.lower_expr(args[0]);
    let cur_block = ctx.cur_block;
    let genfn_chain = ctx.intrinsics.genfn_chain;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(genfn_chain, vec![proto_op, Operand::ConstI64(kind)]),
        Type::I64,
        None,
    );
    Some(Operand::Value(v))
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

/// `__torajs_error_proto_install("<C>")` (RFC 20260718 刀 1) —
/// resolve the injected error class's tag and hand runtime the
/// (tag, name Str) pair; it defines the §20.5.6.3/6.4 own `name` /
/// `message` data properties on `__proto_<C>`. Dropout (no tag)
/// lowers to nothing.
fn try_lower_error_proto_install(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Option<Operand> {
    if args.len() != 1 {
        return None;
    }
    let Expr::String(cname) = ctx.ast.get_expr(args[0]) else {
        return None;
    };
    let cname = cname.clone();
    let Some(tag) = ctx.class_name_to_tag.get(&cname).copied() else {
        return Some(Operand::ConstI64(0));
    };
    let name_op = ctx.lower_expr(args[0]);
    let cur_block = ctx.cur_block;
    let install = ctx.intrinsics.error_proto_install;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(
            install,
            vec![Operand::ConstI64(tag as i64), name_op.clone()],
        ),
    );
    // The lowered Str literal is a caller-owned temp — the runtime
    // entries take their own stakes (rc_inc on the name value).
    let ty = ctx.operand_ty(&name_op);
    ctx.emit_drop_value(name_op, ty);
    Some(Operand::ConstI64(0))
}

/// `__torajs_error_is_error(x)` (RFC 20260718 刀 3) — the injected
/// `Error.isError` static-method body: one Any operand in, Bool out.
/// The operand is borrowed by the runtime probe (a flag read), so no
/// ownership traffic — mirror of the genfn_chain arm's shape.
fn try_lower_error_is_error(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Option<Operand> {
    if args.len() != 1 {
        return None;
    }
    let v_op = ctx.lower_expr(args[0]);
    let cur_block = ctx.cur_block;
    let probe = ctx.intrinsics.error_is_error;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(probe, vec![v_op]),
        Type::Bool,
        None,
    );
    Some(Operand::Value(v))
}

/// Knife B cut 2 (RFC 20260717-class-first-class-value) —
/// `__torajs_static_method_reify("<C>", "<M>")`: resolve
/// `__sm_<C>__<M>`'s boxed adapter and hand the runtime the
/// `(tag, name-Str, adapter-vaddr)` triple so the class object gets
/// its own `<M>` function entry. An adapter-synthesis dropout
/// (unboxable signature) skips the define — the member read keeps
/// its current answer instead of minting an uncallable cell.
fn try_lower_static_method_reify(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Option<Operand> {
    if args.len() != 2 {
        return None;
    }
    let Expr::String(cname) = ctx.ast.get_expr(args[0]) else {
        return None;
    };
    let Expr::String(mname) = ctx.ast.get_expr(args[1]) else {
        return None;
    };
    let cname = cname.clone();
    let mname = mname.clone();
    let Some(tag) = ctx.class_name_to_tag.get(&cname).copied() else {
        return Some(Operand::ConstI64(0));
    };
    let body = format!("__sm_{cname}__{mname}");
    let Some(&body_fid) = ctx.fn_table.get(body.as_str()) else {
        return Some(Operand::ConstI64(0));
    };
    let Some(&(adapter_fid, adapter_sig)) = ctx.boxed_entries.get(&body_fid) else {
        return Some(Operand::ConstI64(0));
    };
    let name_op = ctx.lower_expr(args[1]);
    let cur_block = ctx.cur_block;
    let adapter = ctx.f.append_inst(
        cur_block,
        InstKind::FnAddr(adapter_fid),
        Type::FnSig(adapter_sig),
        None,
    );
    let define = ctx.intrinsics.static_method_define;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(
            define,
            vec![
                Operand::ConstI64(tag as i64),
                name_op.clone(),
                Operand::Value(adapter),
            ],
        ),
    );
    // The lowered Str literal is a caller-owned temp — the runtime
    // key copy is the define's own (dynobj_define rc_incs the key).
    let ty = ctx.operand_ty(&name_op);
    ctx.emit_drop_value(name_op, ty);
    Some(Operand::ConstI64(0))
}

/// RFC 20260718-accessor-reify 刀 2+3 —
/// `__torajs_class_accessor_reify("<C>", "<p>")` (instance,
/// `__cm_` faces onto the prototype) and its static twin
/// (`__sm_` faces onto the class object): resolves the face
/// bodies' boxed adapters (either may be absent for a get-/set-only
/// accessor) and hands runtime the `(tag, name-Str, get-vaddr,
/// set-vaddr)` quad. Both-adapters dropout skips the define.
fn try_lower_class_accessor_reify(
    ctx: &mut LowerCtx<'_>,
    args: &[ExprId],
    is_static: bool,
) -> Option<Operand> {
    if args.len() != 2 {
        return None;
    }
    let Expr::String(cname) = ctx.ast.get_expr(args[0]) else {
        return None;
    };
    let Expr::String(pname) = ctx.ast.get_expr(args[1]) else {
        return None;
    };
    let cname = cname.clone();
    let pname = pname.clone();
    let Some(tag) = ctx.class_name_to_tag.get(&cname).copied() else {
        return Some(Operand::ConstI64(0));
    };
    let body_prefix = if is_static { "__sm_" } else { "__cm_" };
    let face = |suffix: &str| -> Option<(crate::ssa::FuncId, crate::ssa::SigId)> {
        let body = format!("{body_prefix}{cname}__{pname}{suffix}");
        let body_fid = ctx.fn_table.get(body.as_str()).copied()?;
        ctx.boxed_entries.get(&body_fid).copied()
    };
    let get = face("_get");
    let set = face("_set");
    if get.is_none() && set.is_none() {
        return Some(Operand::ConstI64(0));
    }
    let cur_block = ctx.cur_block;
    let mut vaddr = |f: Option<(crate::ssa::FuncId, crate::ssa::SigId)>| match f {
        Some((fid, sig)) => Operand::Value(ctx.f.append_inst(
            cur_block,
            InstKind::FnAddr(fid),
            Type::FnSig(sig),
            None,
        )),
        None => Operand::ConstI64(0),
    };
    let get_op = vaddr(get);
    let set_op = vaddr(set);
    let name_op = ctx.lower_expr(args[1]);
    let define = if is_static {
        ctx.intrinsics.class_static_accessor_define
    } else {
        ctx.intrinsics.class_accessor_define
    };
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(
            define,
            vec![
                Operand::ConstI64(tag as i64),
                name_op.clone(),
                get_op,
                set_op,
            ],
        ),
    );
    // The lowered Str literal is a caller-owned temp — the runtime
    // key copy is the define's own (dynobj_define rc_incs the key).
    let ty = ctx.operand_ty(&name_op);
    ctx.emit_drop_value(name_op, ty);
    Some(Operand::ConstI64(0))
}

fn try_lower_class_register(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Option<Operand> {
    if args.len() != 3 {
        return None;
    }
    let Expr::String(cname) = ctx.ast.get_expr(args[1]) else {
        return None;
    };
    // Compile-time constant flagging a desugar-synthesized generator
    // class — the runtime skips the first-class MakeConstructor
    // wiring for those (§27.3.3.2: a generator fn's `.prototype`
    // object carries no own `constructor`).
    let Expr::Number(is_gen) = ctx.ast.get_expr(args[2]) else {
        return None;
    };
    let is_gen = *is_gen as i64;
    let cname = cname.clone();
    // §15.7.14 class heritage (RFC 20260718 刀 1) — resolve the
    // parent class's tag at compile time so the runtime wire can
    // link `[[Prototype]](Sub) = Super` (a root class, or a parent
    // with no tag, passes -1 → %Function.prototype%).
    let parent_tag = ctx
        .ast
        .class_parents
        .get(&cname)
        .cloned()
        .flatten()
        .and_then(|p| ctx.class_name_to_tag.get(&p).copied())
        .map_or(-1, |t| t as i64);
    let class_op = ctx.lower_expr(args[0]);
    let cur_block = ctx.cur_block;
    if let Some(tag) = ctx.class_name_to_tag.get(&cname).copied() {
        let class_register = ctx.intrinsics.class_register;
        ctx.f.append_void(
            cur_block,
            InstKind::Call(
                class_register,
                vec![
                    Operand::ConstI64(tag as i64),
                    class_op,
                    Operand::ConstI64(is_gen),
                    Operand::ConstI64(parent_tag),
                ],
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
        // RFC 20260718-error-message-own-prop 刀 3 — the
        // derived-ctor no-super ReferenceError factory.
        "ReferenceError" => 3,
        // RFC 20260720 刀 5b — the StringToBigInt parse-failure
        // SyntaxError factory.
        "SyntaxError" => 4,
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
