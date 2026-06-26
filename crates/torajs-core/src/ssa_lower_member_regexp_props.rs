//! `Type::RegExp` Member-access accessor cluster pulled out of
//! [`crate::ssa_lower::lower_expr_inner`]'s `Expr::Member` god-arm
//! as chunk-63 of the decomp (chunks 1-62 = ... + await/p.value
//! Promise + P10.4 fastpath).
//!
//! Three sub-arms covering the spec-defined RegExp accessor surface:
//!
//! - **T-37 followup** — `re.source`. Wires through
//!   `__torajs_regex_get_source` which materializes the runtime
//!   `re->src_bytes` as a fresh `Str`.
//! - **ES §22.2.6.4** — `re.flags`. Returns the spec-ordered flag
//!   string built by `__torajs_regex_get_flags`.
//! - **ES §22.2.6.5-10 boolean flag accessors** — `global` /
//!   `ignoreCase` / `multiline` / `dotAll` / `unicode` / `sticky`.
//!   Routes through the shared `__torajs_regex_has_flag` helper
//!   with the matching `RE_FLAG_*` byte constant. Bit values
//!   mirror `torajs-regex::parser`:
//!   `RE_FLAG_I=0x01, _G=0x02, _M=0x04, _S=0x08, _U=0x10, _Y=0x20`.
//! - **P9.4** — `re.lastIndex`. Reads the i64 field on the
//!   RegExp heap object via `__torajs_regex_get_last_index`. Value
//!   tracks across exec / match calls when the regex carries
//!   `g` or `y`.
//!
//! Returns `Some(op)` on hit; `None` on miss (receiver not
//! `Type::RegExp`, or Member name not in the accessor allowlist).

use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
    obj_ty: Type,
    name: &str,
) -> Option<Operand> {
    if obj_ty != Type::RegExp {
        return None;
    }
    match name {
        "source" => Some(call_string_helper(
            ctx,
            ctx.intrinsics.regex_get_source,
            obj_val,
        )),
        "flags" => Some(call_string_helper(
            ctx,
            ctx.intrinsics.regex_get_flags,
            obj_val,
        )),
        "lastIndex" => {
            let cur_block = ctx.cur_block;
            let v = ctx.f.append_inst(
                cur_block,
                InstKind::Call(ctx.intrinsics.regex_get_last_index, vec![obj_val]),
                Type::I64,
                None,
            );
            Some(Operand::Value(v))
        }
        _ => bool_flag_bit(name).map(|bit| {
            let cur_block = ctx.cur_block;
            let v = ctx.f.append_inst(
                cur_block,
                InstKind::Call(
                    ctx.intrinsics.regex_has_flag,
                    vec![obj_val, Operand::ConstI64(bit)],
                ),
                Type::Bool,
                None,
            );
            Operand::Value(v)
        }),
    }
}

fn call_string_helper(
    ctx: &mut LowerCtx<'_>,
    intrinsic: crate::ssa::FuncId,
    obj_val: Operand,
) -> Operand {
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(intrinsic, vec![obj_val]),
        Type::Str,
        None,
    );
    Operand::Value(v)
}

fn bool_flag_bit(name: &str) -> Option<i64> {
    match name {
        "global" => Some(0x02),
        "ignoreCase" => Some(0x01),
        "multiline" => Some(0x04),
        "dotAll" => Some(0x08),
        "unicode" => Some(0x10),
        "sticky" => Some(0x20),
        _ => None,
    }
}
