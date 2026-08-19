//! RegExp instance method dispatch (`re.test(s)` / `re.exec(s)` /
//! `re.toString()`) pulled out of
//! [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` dispatch as
//! chunk-35 of the `Expr::Call` god-arm decomp (chunks 1-34 = ... +
//! Date instance method dispatch).
//!
//! v0.2 #1 — `re.method(args)` for the RegExp stdlib slice. Receiver
//! type-gated on `Type::RegExp`; methods route to the matching
//! `__torajs_regex_*` runtime intrinsic. Args are borrow-shaped
//! (the runtime helpers don't take ownership — the caller's drop
//! walk handles it).
//!
//! - `test(s)` — bool result (`__torajs_regex_test`); S266 trailing
//!   args silent-drop per ES §22.2.6.16.
//! - `exec(s)` — `Array<string>` result (`__torajs_regex_exec`,
//!   interned `Type::Arr(elem=Str)`); S266 trailing args silent-drop
//!   per ES §22.2.6.2.
//! - `toString()` — `/source/flags` per ES §22.2.6.13; one
//!   runtime alloc avoids the SSA-level concat overhead (`.source` /
//!   `.flags` each would otherwise allocate an intermediate Str +
//!   need explicit drops). S266 trailing args silent-drop.
//!
//! - `compile(pattern?, flags?)` — annexB §B.2.4.1 in-place
//!   receiver re-init (rotation 447); both operands ride as boxed
//!   AnyValues into `__torajs_regex_compile_inplace`.
//!
//! Returns `Some(op)` on hit; `None` on miss (callee not a
//! Member-call, method name not in {test, exec, toString, compile},
//! or receiver isn't `Type::RegExp`) so the caller falls through to
//! the `<Str>.{replace|split|match|matchAll}` regex-receiver arm and
//! beyond.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{LowerCtx, intern_arr_layout};

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Member { obj, name } = ctx.ast.get_expr(callee) else {
        return None;
    };
    if !matches!(name.as_str(), "test" | "exec" | "toString" | "compile") {
        return None;
    }
    let recv_id = *obj;
    let method = name.clone();
    let recv_op = ctx.lower_expr(recv_id);
    let recv_ty = ctx.operand_ty(&recv_op);
    if recv_ty != Type::RegExp {
        // RFC 20260705 chunk 555 — the receiver is already lowered;
        // park the operand for the next cascade arm (single-eval).
        ctx.redispatch_lowered = Some((recv_id, recv_op));
        return None;
    }
    crate::ssa_lower_nullable_guard::emit_undefable_heap_guard(ctx, recv_id, &recv_op);

    // The Call must land in `ctx.cur_block` AS OF after the args are
    // lowered — a branching arg expression (ternary) splits blocks and
    // moves cur_block to the merge block. A pre-lower snapshot appended
    // the call into the already-terminated pre-branch block: the call
    // then executed before the branch with the merge block's operand —
    // garbage haystack pointer (per-iter leak + SIGBUS, chunk 656).
    match method.as_str() {
        "toString" => {
            // S266 — trailing args silent-drop per ES §22.2.6.16.
            for a in args.iter() {
                let _ = ctx.lower_expr(*a);
            }
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.regex_to_string, vec![recv_op]),
                Type::Str,
                None,
            );
            Some(Operand::Value(v))
        }
        "test" => {
            // S266 — trailing args silent-drop per ES §22.2.6.16.
            debug_assert!(!args.is_empty());
            let (s, s_owned) = lower_haystack(ctx, args[0]);
            for a in args.iter().skip(1) {
                let _ = ctx.lower_expr(*a);
            }
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.regex_test, vec![recv_op, s]),
                Type::Bool,
                None,
            );
            if s_owned {
                ctx.emit_drop_value(s, Type::Str);
            }
            Some(Operand::Value(v))
        }
        "exec" => {
            // S266 — trailing args silent-drop per ES §22.2.6.2.
            debug_assert!(!args.is_empty());
            let (s, s_owned) = lower_haystack(ctx, args[0]);
            for a in args.iter().skip(1) {
                let _ = ctx.lower_expr(*a);
            }
            let arr_id = intern_arr_layout(ctx.arr_layouts, Type::Str);
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.regex_exec, vec![recv_op, s]),
                Type::Arr(arr_id),
                None,
            );
            if s_owned {
                ctx.emit_drop_value(s, Type::Str);
            }
            Some(Operand::Value(v))
        }
        "compile" => {
            // annexB §B.2.4.1 (rotation 447) — in-place receiver
            // re-init. The kernel owns the whole argument story
            // (RegExp donor / ToString coercion / TypeError /
            // SyntaxError all record pending throws BEFORE the
            // receiver changes); absent arguments ride as boxed
            // undefined, which the kernel maps to §22.2.3.2's
            // empty string.
            let pat = match args.first() {
                Some(&a) => {
                    let raw = ctx.lower_expr(a);
                    ctx.box_to_any_from_expr(a, raw)
                }
                None => Operand::Value(crate::ssa_lower_call_arr_ho_loop::emit_undef_any_box(ctx)),
            };
            let fl = match args.get(1) {
                Some(&a) => {
                    let raw = ctx.lower_expr(a);
                    ctx.box_to_any_from_expr(a, raw)
                }
                None => Operand::Value(crate::ssa_lower_call_arr_ho_loop::emit_undef_any_box(ctx)),
            };
            for a in args.iter().skip(2) {
                let _ = ctx.lower_expr(*a);
            }
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.regex_compile_inplace, vec![recv_op, pat, fl]),
                Type::RegExp,
                None,
            );
            ctx.emit_throw_check(None);
            Some(Operand::Value(v))
        }
        _ => unreachable!("regex method `{method}` not yet wired"),
    }
}

/// Chunk 699 — lower the haystack arg for test/exec. A Substr view
/// (a for-of-str binding, an exec capture) is a 16-byte
/// parent-pointer block the regex byte reader would misread as an
/// owned Str (probe: `/a/.test(ch)` answered false on a matching
/// char; the recorded risk was a UTF-16 SIGBUS on the same shape),
/// so it materializes through substr_to_owned — a fresh rc=1 temp
/// the caller drops after the call. Owned-Str args pass through.
///
/// RFC 20260716 刀 19 — checker relaxed the arg sig from
/// `Type::String` to `Type::Any` per ES §22.2.6.16 step 3
/// ToString(str). A StringWrapper / Number / Boolean / etc. arg
/// routes through `ssa_lower_call_coercion::emit_to_string` which
/// dispatches per SSA type (any_to_str's TAG_STRING_WRAPPER arm for
/// StringWrapper, i64_to_str / bool_to_str for primitives). The
/// coerced result is owned so the caller's drop fires after the
/// helper borrow read (same pattern as the Substr path).
fn lower_haystack(ctx: &mut LowerCtx<'_>, arg: ExprId) -> (Operand, bool) {
    let s = ctx.lower_expr(arg);
    let ty = ctx.operand_ty(&s);
    match ty {
        Type::Str => (s, false),
        Type::Substr => {
            let owned = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.substr_to_owned, vec![s]),
                Type::Str,
                None,
            );
            (Operand::Value(owned), true)
        }
        _ => {
            let coerced = crate::ssa_lower_call_coercion::emit_to_string(ctx, arg, s, ty, false);
            (coerced, true)
        }
    }
}
