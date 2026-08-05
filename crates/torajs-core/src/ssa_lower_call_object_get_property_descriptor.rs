//! P3.getOwnPropertyDescriptor —
//! `Object.getOwnPropertyDescriptor(obj, key)` pulled out of
//! [`crate::ssa_lower::lower_expr_inner`]'s `Expr::Call` god-arm as
//! chunk-53 of the decomp (chunks 1-52 = ... + Array.of literal-shape).
//!
//! Routes to a single runtime helper that constructs the descriptor
//! Any-box in one shot: reads bucket value+flag bits, alloc +
//! populate a new dynobj with the 4 data-descriptor fields, wrap in
//! Any-box. Missing key returns Any-boxed `undefined` per ES
//! §19.1.2.10.
//!
//! Four static fast paths short-circuit before the general dynobj
//! walk:
//!
//! - **W-M** — typed `Type::Str` `.length`. ES §22.1.5.1: String's
//!   `length` own prop is `{value, writable: false, enumerable:
//!   false, configurable: false}` (every flag `false` — unlike
//!   Array's writable length). Helper takes Str ptr and loads the
//!   u32 len internally.
//! - **W-M-rest** — typed `Type::Str` numeric-indexed access. ES
//!   §22.1.5.2: `s[i]` own is `{value: char_at(i), writable: false,
//!   enumerable: true, configurable: false}` for in-range `i`. Only
//!   canonical decimal-integer string keys (`"0".."N"` without
//!   leading zero except literal `"0"`) take the fast path; other
//!   shapes fall through.
//! - **RFC C5a (retired)** — the typed `Type::Arr(_)` `.length`
//!   fast path is gone (RFC 20260712-arr-exotic-define chunk D):
//!   the general helper's TAG_ARR arm answers the full §10.4.2.4
//!   descriptor including the length-lock writable bit.
//! - **S315** — trailing args past `(obj, key)` `lower_expr`'d
//!   (silent-drop per spec) so step()-style side-effect exprs fire.
//!   Placed after obj-lower but before key/dispatch so trailing
//!   args evaluate exactly once across all return paths.
//!
//! Non-`Any` receiver gets boxed to its spec-correct AnyValue
//! immediate before the general helper Call (so the runtime can
//! tag-discriminate the `ToObject(undefined|null) → throw TypeError`
//! case per RFC C4). An already-`Any` receiver passes through.
//! `emit_throw_check(None)` after the helper propagates the runtime
//! TypeError into the enclosing `try/catch`.
//!
//! Returns `Some(op)` on hit; `None` on miss (callee not the
//! `Object.getOwnPropertyDescriptor` Member-Ident shape, or args
//! fewer than 2).

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    if args.is_empty() {
        return None;
    }
    let Expr::Member {
        obj: ns_id,
        name: m_name,
    } = ctx.ast.get_expr(callee)
    else {
        return None;
    };
    // §20.1.2.9 — the plural takes one argument and answers a fresh
    // object of descriptors. It shares this entry because it shares
    // the namespace resolution and the receiver boxing; the walk
    // itself is a kernel, since OwnPropertyKeys is a runtime question.
    if m_name == "getOwnPropertyDescriptors" {
        return lower_plural(ctx, callee, args);
    }
    if m_name != "getOwnPropertyDescriptor" {
        return None;
    }
    if args.len() < 2 {
        return None;
    }
    let ns_id = *ns_id;
    let Expr::Ident(ns) = ctx.ast.get_expr(ns_id) else {
        return None;
    };
    // §28.1.5 Reflect.getOwnPropertyDescriptor (rotation 266 刀 R1)
    // shares this lowering: same descriptor kernel, but step 1 is a
    // strict IsObject gate (every primitive throws, no ToObject) —
    // emitted below via the define family's receiver guard. The
    // Object-flavored Str fast paths are skipped: a Str target under
    // Reflect must throw, not answer wrapper own-props.
    let is_reflect = ns == "Reflect";
    if ns != "Object" && !is_reflect {
        return None;
    }
    let obj_raw = ctx.lower_expr(args[0]);
    let obj_ty = ctx.operand_ty(&obj_raw);
    for &a in args.iter().skip(2) {
        let _ = ctx.lower_expr(a);
    }
    if is_reflect {
        crate::ssa_lower_object_define::emit_receiver_typecheck(
            ctx,
            args[0],
            &obj_raw,
            obj_ty.clone(),
        );
    }
    let key_expr = ctx.ast.get_expr(args[1]);
    let cur_block = ctx.cur_block;
    if !is_reflect
        && matches!(obj_ty, Type::Str)
        && let Expr::String(k) = key_expr
        && k == "length"
    {
        let v = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.str_length_descriptor, vec![obj_raw]),
            Type::Any,
            None,
        );
        return Some(Operand::Value(v));
    }
    if !is_reflect
        && matches!(obj_ty, Type::Str)
        && let Expr::String(k) = key_expr
        && is_canonical_index_key(k)
        && let Ok(idx) = k.parse::<i64>()
    {
        let v = ctx.f.append_inst(
            cur_block,
            InstKind::Call(
                ctx.intrinsics.str_index_descriptor,
                vec![obj_raw, Operand::ConstI64(idx)],
            ),
            Type::Any,
            None,
        );
        return Some(Operand::Value(v));
    }
    // RFC C5a's typed-Arr "length" fast path retired (RFC
    // 20260712-arr-exotic-define chunk D): the general helper's
    // TAG_ARR arm answers the full descriptor including the
    // FLAG_ARR_LENGTH_RO writable bit, which the len-only helper
    // cannot see.
    let obj_op = if matches!(obj_ty, Type::Any) {
        obj_raw
    } else {
        ctx.box_to_any_from_expr(args[0], obj_raw)
    };
    // RFC 20260716 刀 17 — ToPropertyKey coerce the key arg (checker
    // 刀 17 relaxed the sig from Type::String to Type::Any). This used
    // to carry its own copy of the define family's resolution table,
    // and so carried the same hole: a symbol riding an `any` was
    // stringified, and the descriptor came back undefined for a
    // property that was right there. It asks the shared resolver now.
    let (key_op, key_owned) = crate::ssa_lower_object_define::lower_key(
        ctx,
        &crate::ssa_lower_object_define::DefineKey::Expr(args[1]),
    );
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.get_property_descriptor,
            vec![obj_op, key_op.clone()],
        ),
        Type::Any,
        None,
    );
    crate::ssa_lower_object_define::emit_key_release(ctx, key_op, key_owned);
    ctx.emit_throw_check(None);
    Some(Operand::Value(v))
}

/// `Object.getOwnPropertyDescriptors(O)` — one kernel call over the
/// boxed receiver. Only the `Object` namespace has this spelling;
/// `Reflect` has no plural, so the caller's namespace check is left
/// to the singular path above and re-made here.
fn lower_plural(ctx: &mut LowerCtx<'_>, callee: ExprId, args: &[ExprId]) -> Option<Operand> {
    let Expr::Member { obj: ns_id, .. } = ctx.ast.get_expr(callee) else {
        return None;
    };
    let Expr::Ident(ns) = ctx.ast.get_expr(*ns_id) else {
        return None;
    };
    if ns != "Object" {
        return None;
    }
    let obj_raw = ctx.lower_expr(args[0]);
    let obj_ty = ctx.operand_ty(&obj_raw);
    // S272 — lower and drop the trailing args (useful arity 1).
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    let obj_op = if matches!(obj_ty, Type::Any) {
        obj_raw
    } else {
        ctx.box_to_any_from_expr(args[0], obj_raw)
    };
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.get_property_descriptors, vec![obj_op]),
        Type::Any,
        None,
    );
    ctx.emit_throw_check(None);
    Some(Operand::Value(v))
}

fn is_canonical_index_key(k: &str) -> bool {
    let bytes = k.as_bytes();
    if bytes.is_empty() || bytes.len() > 20 {
        return false;
    }
    if bytes == b"0" {
        return true;
    }
    if bytes[0] == b'0' {
        return false;
    }
    bytes.iter().all(|&b| b.is_ascii_digit())
}
