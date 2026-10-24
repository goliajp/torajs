//! How a function's open fall-through path returns.

use crate::ssa::{self, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

/// Terminate a block that the body walk left open — the path that
/// runs off the end of the function.
///
/// ES §10.2.1.4 [[Call]] step 11 says that path answers `undefined`,
/// so the question is only how each return width spells it:
///
/// - `void` — the slot's one value already is it.
/// - `number` — an I64 slot has no bit pattern to spare, F64 does.
///   num_width seeds exactly the `number` returns whose body can get
///   here (see [`crate::ast::body_always_terminates`]), so an F64
///   return slot with a still-open block is one of those.
/// - pointer-shaped (`Str` / `Substr` / fn / Obj / Arr / Closure) —
///   the per-type immortal sentinel cell, the same one an optional
///   field or a `find` miss hands out.
///
/// A width with no sentinel yet keeps asserting unreachable, which is
/// what every one of these used to do.
pub(super) fn close_fallthrough_path(ctx: &mut LowerCtx<'_>, cb: ssa::BlockId) {
    let term = match ctx.f.ret {
        Type::Void => Terminator::Ret(None),
        Type::F64 => Terminator::Ret(Some(Operand::ConstF64(f64::from_bits(
            crate::ssa_lower_undef_f64_source::F64_UNDEF_SENTINEL_BITS,
        )))),
        // An `any` return spells its undefined as the immediate
        // ANY_UNDEF box (tag 5, payload 0), not a heap cell — the
        // implicit-generics pass routes every value-returning fn
        // whose body can fall through to an `any` ret precisely so
        // this path has a spelling (a Bool/I64 ret has none, and
        // the open block used to terminate `unreachable`: running
        // off the end of `if (c) return true;` trapped).
        Type::Any => {
            let b = ctx.f.append_inst(
                cb,
                ssa::InstKind::BoxAny(5, Operand::ConstI64(0)),
                Type::Any,
                None,
            );
            Terminator::Ret(Some(Operand::Value(b)))
        }
        ret_ty => match ctx.str_undef_sentinel_for(ret_ty) {
            Some(cell) => Terminator::Ret(Some(cell)),
            None => Terminator::Unreachable,
        },
    };
    ctx.f.set_term(cb, term);
}
