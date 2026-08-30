//! Runtime-branch `typeof` emitters split out of
//! [`crate::ssa_lower_typeof`] (file-size limit): the sentinel-aware
//! two/three-state chains for Str / Substr / F64 / pointer-shaped
//! heap slots. Each mirrors the alloca/merge shape of
//! `coerce_to_bool`'s Str arm; the static gates (`is_*_source`
//! predicates) live with the caller.

use crate::ssa::{IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_intrinsics_str_b::STR_UNDEF_CELL_SYM;
use crate::ssa_lower_intrinsics_substr::SUBSTR_UNDEF_CELL_SYM;

/// Three-state `typeof` for a Str slot that may hold a nullish repr
/// (RFC 20260707 chunk 3): NULL → "object" (JS null), the undefined
/// sentinel cell → "undefined", a real Str → "string". Branch chain
/// over interned literals, same alloca/merge shape as
/// `coerce_to_bool`'s Str arm.
pub(crate) fn emit_str_typeof_runtime(ctx: &mut LowerCtx<'_>, v: Operand) -> Operand {
    let result_slot = ctx.alloca_in_entry(Type::Str, Some("__typeof_r"));
    let null_blk = ctx.f.add_block();
    let chk_undef_blk = ctx.f.add_block();
    let undef_blk = ctx.f.add_block();
    let str_blk = ctx.f.add_block();
    let merge = ctx.f.add_block();
    let is_null = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Eq, v.clone(), Operand::ConstPtrNull),
        Type::Bool,
        None,
    );
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: Operand::Value(is_null),
            then_blk: null_blk,
            else_blk: chk_undef_blk,
        },
    );
    ctx.cur_block = null_blk;
    let obj_lit = ctx.intern_string_literal("object");
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(obj_lit), Operand::Value(result_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(merge));
    ctx.cur_block = chk_undef_blk;
    let sentinel = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::GlobalRef(STR_UNDEF_CELL_SYM.to_string()),
        Type::Str,
        None,
    );
    let is_undef = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Eq, v, Operand::Value(sentinel)),
        Type::Bool,
        None,
    );
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: Operand::Value(is_undef),
            then_blk: undef_blk,
            else_blk: str_blk,
        },
    );
    ctx.cur_block = undef_blk;
    let undef_lit = ctx.intern_string_literal("undefined");
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(undef_lit), Operand::Value(result_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(merge));
    ctx.cur_block = str_blk;
    let str_lit = ctx.intern_string_literal("string");
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(str_lit), Operand::Value(result_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(merge));
    ctx.cur_block = merge;
    let r = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Str, Operand::Value(result_slot), 0),
        Type::Str,
        None,
    );
    Operand::Value(r)
}

/// Two-state `typeof` for a Substr slot (RFC 20260707 residual):
/// the Substr-shaped undefined sentinel → "undefined", any real
/// view → "string". Same alloca/merge shape as the Str three-state
/// chain minus the NULL arm (the index-get producers never answer
/// NULL — nullish inputs propagate the sentinel).
pub(crate) fn emit_substr_typeof_runtime(ctx: &mut LowerCtx<'_>, v: Operand) -> Operand {
    let result_slot = ctx.alloca_in_entry(Type::Str, Some("__typeof_r"));
    let undef_blk = ctx.f.add_block();
    let str_blk = ctx.f.add_block();
    let merge = ctx.f.add_block();
    let sentinel = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::GlobalRef(SUBSTR_UNDEF_CELL_SYM.to_string()),
        Type::Substr,
        None,
    );
    let is_undef = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Eq, v, Operand::Value(sentinel)),
        Type::Bool,
        None,
    );
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: Operand::Value(is_undef),
            then_blk: undef_blk,
            else_blk: str_blk,
        },
    );
    ctx.cur_block = undef_blk;
    let undef_lit = ctx.intern_string_literal("undefined");
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(undef_lit), Operand::Value(result_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(merge));
    ctx.cur_block = str_blk;
    let str_lit = ctx.intern_string_literal("string");
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(str_lit), Operand::Value(result_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(merge));
    ctx.cur_block = merge;
    let r = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Str, Operand::Value(result_slot), 0),
        Type::Str,
        None,
    );
    Operand::Value(r)
}

/// RFC 20260722-find-miss chunk B — three-state `typeof` for a
/// pointer-shaped heap slot that may hold a nullish repr: NULL →
/// "object" (JS null), the generic undefined cell → "undefined",
/// a live cell → `base_lit` ("object" for Obj/Arr, "function" for
/// Closure). Same alloca/merge shape as the Str three-state chain.
pub(crate) fn emit_heap_typeof_runtime(
    ctx: &mut LowerCtx<'_>,
    v: Operand,
    base_lit: &str,
) -> Operand {
    let result_slot = ctx.alloca_in_entry(Type::Str, Some("__typeof_r"));
    let null_blk = ctx.f.add_block();
    let chk_undef_blk = ctx.f.add_block();
    let undef_blk = ctx.f.add_block();
    let live_blk = ctx.f.add_block();
    let merge = ctx.f.add_block();
    let is_null = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Eq, v.clone(), Operand::ConstPtrNull),
        Type::Bool,
        None,
    );
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: Operand::Value(is_null),
            then_blk: null_blk,
            else_blk: chk_undef_blk,
        },
    );
    ctx.cur_block = null_blk;
    let obj_lit = ctx.intern_string_literal("object");
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(obj_lit), Operand::Value(result_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(merge));
    ctx.cur_block = chk_undef_blk;
    let ty = ctx.operand_ty(&v);
    let sentinel = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::GlobalRef(crate::ssa_lower_binop_null_undef::UNDEF_CELL_SYM.to_string()),
        ty,
        None,
    );
    let is_undef = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Eq, v, Operand::Value(sentinel)),
        Type::Bool,
        None,
    );
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: Operand::Value(is_undef),
            then_blk: undef_blk,
            else_blk: live_blk,
        },
    );
    ctx.cur_block = undef_blk;
    let undef_lit = ctx.intern_string_literal("undefined");
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(undef_lit), Operand::Value(result_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(merge));
    ctx.cur_block = live_blk;
    let base = ctx.intern_string_literal(base_lit);
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(base), Operand::Value(result_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(merge));
    ctx.cur_block = merge;
    let r = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Str, Operand::Value(result_slot), 0),
        Type::Str,
        None,
    );
    Operand::Value(r)
}

/// RFC 20260708-typed-arr-oob-read chunk 2 — two-state typeof for
/// an F64 value that may hold the undefined-NaN sentinel: bits
/// compare against the sentinel pattern picks "undefined" over
/// "number". Static gate (is_undef_f64_source) keeps arithmetic
/// NaNs (payload-propagated on AArch64) out of this branch.
pub(crate) fn emit_f64_typeof_runtime(ctx: &mut LowerCtx<'_>, v: Operand) -> Operand {
    let bits = ctx
        .f
        .append_inst(ctx.cur_block, InstKind::BitCastF64ToI64(v), Type::I64, None);
    let is_undef = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(
            IPred::Eq,
            Operand::Value(bits),
            Operand::ConstI64(crate::ssa_lower_undef_f64_source::F64_UNDEF_SENTINEL_BITS as i64),
        ),
        Type::Bool,
        None,
    );
    let undef_blk = ctx.f.add_block();
    let num_blk = ctx.f.add_block();
    let merge = ctx.f.add_block();
    let result_slot = ctx.alloca_in_entry(Type::Str, Some("__f64typeof"));
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: Operand::Value(is_undef),
            then_blk: undef_blk,
            else_blk: num_blk,
        },
    );
    ctx.cur_block = undef_blk;
    let undef_lit = ctx.intern_string_literal("undefined");
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(undef_lit), Operand::Value(result_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(merge));
    ctx.cur_block = num_blk;
    let num_lit = ctx.intern_string_literal("number");
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(num_lit), Operand::Value(result_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(merge));
    ctx.cur_block = merge;
    let out = ctx.f.append_inst(
        merge,
        InstKind::Load(Type::Str, Operand::Value(result_slot), 0),
        Type::Str,
        None,
    );
    Operand::Value(out)
}
