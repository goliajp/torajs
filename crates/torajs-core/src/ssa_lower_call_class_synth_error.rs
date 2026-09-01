//! Error-family synthesis magics, carved out of
//! `ssa_lower_call_class_synth.rs` when the RFC 20260808 刀-4
//! species mark pushed the parent past the 500-line hard limit —
//! verbatim move. The four arms are the §20.5 surface: injected
//! error class prototype installs (`name` / `message` own data
//! properties), §20.5.8.1 InstallErrorCause, §20.5.2.1
//! Error.isError, and the native-error registry wire.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_call_class_synth_reify as reify;

/// `__torajs_error_install_cause(this, options.cause)` (§20.5.8.1) —
/// the injected ctors' `cause` install. Spelled as a call rather than
/// an assignment because CreateNonEnumerableDataPropertyOrThrow wants
/// `{W:1, E:0, C:1}`, and an assignment can only produce the ordinary
/// enumerable entry (which is still what a user's own `err.cause = x`
/// after construction must produce — so only the ctor moves).
pub(crate) fn try_lower_error_install_cause(
    ctx: &mut LowerCtx<'_>,
    args: &[ExprId],
) -> Option<Operand> {
    if args.len() != 2 {
        return None;
    }
    let recv = ctx.lower_expr(args[0]);
    let recv_any = if matches!(ctx.operand_ty(&recv), Type::Any) {
        recv
    } else {
        ctx.box_to_any(recv)
    };
    let val = ctx.lower_expr(args[1]);
    let val_any = if matches!(ctx.operand_ty(&val), Type::Any) {
        val
    } else {
        ctx.box_to_any(val)
    };
    let cur_block = ctx.cur_block;
    let install = ctx.intrinsics.error_install_cause;
    ctx.f
        .append_void(cur_block, InstKind::Call(install, vec![recv_any, val_any]));
    Some(Operand::ConstI64(0))
}

/// `__torajs_error_proto_install("<C>")` (RFC 20260718 刀 1) —
/// resolve the injected error class's tag and hand runtime the
/// (tag, name Str) pair; it defines the §20.5.6.3/6.4 own `name` /
/// `message` data properties on `__proto_<C>`. Dropout (no tag)
/// lowers to nothing.
pub(crate) fn try_lower_error_proto_install(
    ctx: &mut LowerCtx<'_>,
    args: &[ExprId],
) -> Option<Operand> {
    if args.len() != 1 {
        return None;
    }
    let Expr::String(cname) = ctx.ast.get_expr(args[0]) else {
        return None;
    };
    let cname = cname.to_string_lossy_owned();
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
    // rotation 186 — the install defines name/message/toString onto
    // the prototype dynobj (may resize); refresh the module binding
    // from the written-back table.
    reify::emit_class_binding_writeback(ctx, &cname, tag, true);
    Some(Operand::ConstI64(0))
}

/// `__torajs_error_is_error(x)` (RFC 20260718 刀 3) — the injected
/// `Error.isError` static-method body: one Any operand in, Bool out.
/// The operand is borrowed by the runtime probe (a flag read), so no
/// ownership traffic — mirror of the genfn_chain arm's shape.
pub(crate) fn try_lower_error_is_error(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Option<Operand> {
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

pub(crate) fn try_lower_register_native_error(
    ctx: &mut LowerCtx<'_>,
    args: &[ExprId],
) -> Option<Operand> {
    if args.len() != 1 {
        return None;
    }
    let Expr::String(cname) = ctx.ast.get_expr(args[0]) else {
        return None;
    };
    let cname = cname.to_string_lossy_owned();
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
        // §27.2.4.2 — the rejection an all-rejected `Promise.any`
        // answers. Its factory takes the `errors` array ahead of the
        // message, which is why torajs-throw reads this slot through
        // its own typed lookup.
        "AggregateError" => 5,
        // §19.2.6 — the URI kernels' malformed-input raise
        // (torajs-throw SLOT_URI_ERROR).
        "URIError" => 6,
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
