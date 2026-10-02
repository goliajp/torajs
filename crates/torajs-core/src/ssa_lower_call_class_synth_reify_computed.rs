//! The computed-key reification arms (`__torajs_class_computed_reify`
//! / `__torajs_class_computed_key`), split out of
//! [`crate::ssa_lower_call_class_synth_reify`] when K4 (static
//! members of a unit class) needed room there (file-size limit).
//! Bodies verbatim.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_call_class_synth_reify::static_this_free;
use crate::ssa_lower_call_class_synth_writeback::emit_class_binding_writeback;

pub(super) fn try_lower_class_computed_reify(
    ctx: &mut LowerCtx<'_>,
    args: &[ExprId],
) -> Option<Operand> {
    if args.len() != 5 {
        return None;
    }
    let Expr::String(cname) = ctx.ast.get_expr(args[0]) else {
        return None;
    };
    let cname = cname.to_string_lossy_owned();
    let Expr::String(sentinel) = ctx.ast.get_expr(args[1]) else {
        return None;
    };
    let Expr::Number(kind) = ctx.ast.get_expr(args[3]) else {
        return None;
    };
    let Expr::Number(is_static) = ctx.ast.get_expr(args[4]) else {
        return None;
    };
    let cname = cname.clone();
    let sentinel = sentinel.clone();
    let kind = *kind as i64;
    let is_static = *is_static as i64;
    // §15.4 evaluation order — the ComputedPropertyName evaluates
    // unconditionally (side effects + throws are observable), even
    // when a tag / adapter dropout skips the define below.
    let (key_op, key_owned) = crate::ssa_lower_object_define::lower_key(
        ctx,
        &crate::ssa_lower_object_define::DefineKey::Expr(args[2]),
    );
    let tag = ctx.class_name_to_tag.get(&cname).copied();
    let prefix = if is_static != 0 { "__sm_" } else { "__cm_" };
    // 563-04 / 402-01 — inside a GENERIC class the member's original
    // never lowers; its all-`any` mono instance (`$$anywv`, pre-seeded
    // by `monomorphize_and_check`) is the body the reified face
    // wraps. Without the fallback every computed member of a generic
    // class dropped out here and never reached the prototype at all:
    // `class J<T> { m(x: T) {} [k]() {} }` answered
    // ["constructor","m"].
    let body_symbol = |suffix: &str| -> String {
        let body = format!("{prefix}{cname}__{sentinel}{suffix}");
        if ctx.fn_table.contains_key(body.as_str()) {
            body
        } else {
            format!("{body}$$anywv")
        }
    };
    let adapter = |suffix: &str| -> Option<(crate::ssa::FuncId, crate::ssa::SigId)> {
        let body_fid = ctx.fn_table.get(body_symbol(suffix).as_str()).copied()?;
        ctx.boxed_entries.get(&body_fid).copied()
    };
    let resolved = match kind {
        0 => adapter(""),
        1 => adapter("_get"),
        _ => adapter("_set"),
    };
    // Statics only. The instance-side verdict needs the
    // `fn_ignores_receiver` proof that lives in
    // `ssa_lower_module_metadata`, which this site cannot see, so a
    // computed instance method keeps today's conservative 0.
    let body_name = body_symbol("");
    let this_free = kind == 0
        && is_static != 0
        && ctx
            .fn_table
            .get(body_name.as_str())
            .copied()
            .is_some_and(|fid| static_this_free(ctx, &body_name, fid));
    if let (Some(tag), Some((fid, sig))) = (tag, resolved) {
        let cur_block = ctx.cur_block;
        let vaddr = ctx
            .f
            .append_inst(cur_block, InstKind::FnAddr(fid), Type::FnSig(sig), None);
        let (define, def_args) = if kind == 0 {
            (
                ctx.intrinsics.class_computed_method_define,
                vec![
                    Operand::ConstI64(tag as i64),
                    key_op,
                    Operand::Value(vaddr),
                    Operand::ConstI64(is_static),
                    Operand::ConstI64(i64::from(this_free)),
                ],
            )
        } else {
            let (g, s) = if kind == 1 {
                (Operand::Value(vaddr), Operand::ConstI64(0))
            } else {
                (Operand::ConstI64(0), Operand::Value(vaddr))
            };
            (
                ctx.intrinsics.class_computed_accessor_define,
                vec![
                    Operand::ConstI64(tag as i64),
                    key_op,
                    g,
                    s,
                    Operand::ConstI64(is_static),
                ],
            )
        };
        ctx.f
            .append_void(cur_block, InstKind::Call(define, def_args));
    }
    crate::ssa_lower_object_define::emit_key_release(ctx, key_op, key_owned);
    if let Some(tag) = tag {
        emit_class_binding_writeback(ctx, &cname, tag, is_static == 0);
    }
    Some(Operand::ConstI64(0))
}

/// 419-01 — `__torajs_class_computed_key(<key expr>)`: the FIELD
/// lane's half of §15.7.14, answering the ToPropertyKey'd key as an
/// `any` for the `__ccmk_<C>_<n>` module global to hold.
///
/// The method / accessor faces reach ToPropertyKey through the reify
/// above, which hands `lower_key`'s answer straight to a define. A
/// field has no define at the class-decl position — its key parks in
/// a global the ctor prefix reads per construction — so the desugar
/// used to park `<key> as any`, an unconverted box. The conversion
/// then happened at the keyed write instead: `class D { [obj]; }`
/// called `obj.toString()` once per `new D()` and never at the class
/// definition, where §15.7.14 step 1 puts it (ClassFieldDefinition-
/// Evaluation step 1-2 evaluate ClassElementName and ReturnIfAbrupt).
/// A throwing `toString` therefore escaped entirely for a class that
/// is defined and never constructed — the shape test262's
/// `class/elements/evaluation-error/computed-name-tostring-err`
/// tests.
///
/// `lower_key` already carries every branch of §7.1.19 (Symbol
/// pass-through, Str identity, Any kernel, ToString for the rest)
/// together with the throw checks each one needs, so this only adds
/// the two things a long-lived global wants: a share of its own when
/// the answer was borrowed, and the tag-4 encode that puts the key
/// cell in an `any` slot. The answer is always a heap cell (Str or
/// Symbol), which is exactly what tag 4 spells.
pub(super) fn try_lower_class_computed_key(
    ctx: &mut LowerCtx<'_>,
    args: &[ExprId],
) -> Option<Operand> {
    if args.len() != 1 {
        return None;
    }
    let (key_op, key_owned) = crate::ssa_lower_object_define::lower_key(
        ctx,
        &crate::ssa_lower_object_define::DefineKey::Expr(args[0]),
    );
    if !key_owned {
        // Borrowed from the source binding — the global outlives the
        // expression, so it needs a stake rather than a view.
        ctx.emit_rc_inc(key_op);
    }
    let cur_block = ctx.cur_block;
    let raw = ctx
        .f
        .append_inst(cur_block, InstKind::PtrToInt(key_op), Type::I64, None);
    let boxed = ctx.f.append_inst(
        cur_block,
        InstKind::BoxAny(4, Operand::Value(raw)),
        Type::Any,
        None,
    );
    Some(Operand::Value(boxed))
}
