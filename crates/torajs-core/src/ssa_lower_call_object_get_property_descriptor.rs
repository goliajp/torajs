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
//! - **RFC C5a** — typed `Type::Arr(_)` `.length`. ES §10.4.2.4:
//!   Array's `length` own prop is `{value, writable: true,
//!   enumerable: false, configurable: false}`. Bypasses the general
//!   dynobj-walking path (which would report undefined for
//!   Array.length).
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
    if args.len() < 2 {
        return None;
    }
    let Expr::Member {
        obj: ns_id,
        name: m_name,
    } = ctx.ast.get_expr(callee)
    else {
        return None;
    };
    if m_name != "getOwnPropertyDescriptor" {
        return None;
    }
    let ns_id = *ns_id;
    let Expr::Ident(ns) = ctx.ast.get_expr(ns_id) else {
        return None;
    };
    if ns != "Object" {
        return None;
    }
    let obj_raw = ctx.lower_expr(args[0]);
    let obj_ty = ctx.operand_ty(&obj_raw);
    for &a in args.iter().skip(2) {
        let _ = ctx.lower_expr(a);
    }
    let key_expr = ctx.ast.get_expr(args[1]);
    let cur_block = ctx.cur_block;
    if matches!(obj_ty, Type::Str)
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
    if matches!(obj_ty, Type::Str)
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
    if matches!(obj_ty, Type::Arr(_))
        && let Expr::String(k) = key_expr
        && k == "length"
    {
        let len = ctx.f.append_inst(
            cur_block,
            InstKind::Load(Type::I64, obj_raw, 8),
            Type::I64,
            None,
        );
        let cur_block = ctx.cur_block;
        let v = ctx.f.append_inst(
            cur_block,
            InstKind::Call(
                ctx.intrinsics.arr_length_descriptor,
                vec![Operand::Value(len)],
            ),
            Type::Any,
            None,
        );
        return Some(Operand::Value(v));
    }
    let obj_op = if matches!(obj_ty, Type::Any) {
        obj_raw
    } else {
        ctx.box_to_any_from_expr(args[0], obj_raw)
    };
    let key_op = ctx.lower_expr(args[1]);
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.get_property_descriptor, vec![obj_op, key_op]),
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
