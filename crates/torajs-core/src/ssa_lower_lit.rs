//! Literal/intrinsic-construct arms of
//! [`crate::ssa_lower::lower_expr_inner`] pulled out as chunk-82
//! of the decomp. Five arms grouped because they all yield a
//! constant or single-intrinsic-call value with minimal control
//! flow:
//!
//! - `Expr::Number(n)` → `ConstI64` (integer in [-2^53, 2^53]) /
//!   `ConstF64` (fractional, NaN, ±Inf, magnitude > 2^53 — without
//!   the magnitude check `1e21 as i64` saturates to `i64::MAX`,
//!   printing 9223372036854775807 instead of `1e+21`).
//! - `Expr::String(s)` — intern as `Type::Str` ptr via
//!   `intern_string_literal`.
//! - `Expr::BigInt { digits, radix }` (T-25 v0.7) — interned digit
//!   body fed to `__torajs_bigint_from_hex` (radix 16) or
//!   `__torajs_bigint_from_decimal`. The runtime walks past the
//!   heap header (offset 16) at the call site to read the digit
//!   bytes; passing the Str ptr directly keeps SSA arithmetic
//!   clean (no pointer-to-int casts).
//! - `Expr::NewTarget` (P4.5) — inside a ctor body (where
//!   `desugar_classes` injected the hidden `__new_target: any`
//!   param), Load + `rc_inc` from the local slot so each read
//!   yields an owned ref balanced by the consumer's end-of-scope
//!   drop. Multi-level `super()` chains would UAF the new.target
//!   any-box without the bump (deepest ctor drops both
//!   `__new_target` AND `const t = new.target` slot for one
//!   transferred ref). Outside any ctor, emit ANY_UNDEF box per
//!   spec §13.3.10.
//! - `Expr::Regex { pattern, flags }` (v0.2 #1) — regex literal
//!   `/pat/flags`. Three things at once:
//!   1. **Per-fn dedup cache** (`regex_lit_cache`) — V0.2 perf:
//!      multiple `/pat/flag` at different positions within the
//!      same fn lower to the same RegExp value (fn-scope const
//!      RegExp LICM).
//!   2. **Entry-block hoist** — switch `cur_block` to `BlockId(0)`
//!      before emitting the `regex_compile*` call so the value
//!      dominates every later use, then restore.
//!   3. **AOT bake gate** (V0.2 P14 chunk 7.7 v2 step 12 C2 Phase
//!      C-6) — capture-free + DFA-eligible literal regex gets a
//!      baked-DFA payload pushed into `module.baked_regex_entries`
//!      via [`crate::ssa_lower_regex_bake::try_bake_regex_dfa`].
//!      The lowered call switches to
//!      `__torajs_regex_compile_from_static_dfa` (3-arg:
//!      meta_ptr/pat/flag) so the runtime surface match path reads
//!      `RegExp::baked_dfa_view` directly and skips `build_dfa`.
//!      Ineligible patterns + capture-using literals + `new
//!      RegExp(...)` keep using the 2-arg `regex_compile`.

use crate::ssa::{self, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn lower_number(n: f64) -> Operand {
    // Integer-valued literals within the double-exact window
    // (|n| ≤ 2^53) stay i64 — both tiers print those identically.
    // Beyond 2^53 the JS value IS a rounded double and Number
    // printing is shortest-roundtrip (`1000000000000000128` prints
    // "1000000000000000100"), so the literal must stay F64-tier;
    // the old 2^63 bound only guarded the as-cast saturation and
    // let the I64 tier print the exact decimal (rotation 184).
    if n.fract() != 0.0 || n.abs() > 9.007199254740992e15 || !n.is_finite() {
        Operand::ConstF64(n)
    } else {
        Operand::ConstI64(n as i64)
    }
}

pub(crate) fn lower_string(ctx: &mut LowerCtx<'_>, s: &torajs_wtf8::Wtf8) -> Operand {
    Operand::Value(ctx.intern_string_literal(s))
}

pub(crate) fn lower_bigint(ctx: &mut LowerCtx<'_>, digits: &str, radix: u32) -> Operand {
    let len = digits.len() as i64;
    let s_ptr = ctx.intern_string_literal(digits);
    let intrinsic = if radix == 16 {
        ctx.intrinsics.bigint_from_hex
    } else {
        ctx.intrinsics.bigint_from_decimal
    };
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            intrinsic,
            vec![Operand::Value(s_ptr), Operand::ConstI64(len)],
        ),
        Type::BigInt,
        None,
    );
    Operand::Value(v)
}

pub(crate) fn lower_new_target(ctx: &mut LowerCtx<'_>) -> Operand {
    if let Some(info) = ctx.locals.get("__new_target").cloned() {
        let cur_block = ctx.cur_block;
        let v = ctx.f.append_inst(
            cur_block,
            InstKind::Load(info.ty, Operand::Value(info.slot), 0),
            info.ty,
            None,
        );
        ctx.emit_rc_inc(Operand::Value(v));
        return Operand::Value(v);
    }
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.any_box,
            vec![Operand::ConstI64(5), Operand::ConstI64(0)],
        ),
        Type::Any,
        None,
    );
    Operand::Value(v)
}

pub(crate) fn lower_regex(ctx: &mut LowerCtx<'_>, pattern: &str, flags: &str) -> Operand {
    let key = (pattern.to_string(), flags.to_string());
    if let Some(&cached) = ctx.regex_lit_cache.get(&key) {
        return Operand::Value(cached);
    }
    let saved_block = ctx.cur_block;
    ctx.cur_block = ssa::BlockId(0);
    let pat_v = ctx.intern_string_literal(pattern);
    let flag_v = ctx.intern_string_literal(flags);
    let baked_idx =
        crate::ssa_lower_regex_bake::try_bake_regex_dfa(ctx.baked_regex_buf, pattern, flags);
    let v = if let Some(idx) = baked_idx {
        emit_baked_dfa_compile(ctx, idx, pat_v, flag_v)
    } else {
        emit_regex_compile(ctx, pat_v, flag_v)
    };
    ctx.cur_block = saved_block;
    ctx.regex_lit_cache.insert(key, v);
    Operand::Value(v)
}

fn emit_baked_dfa_compile(
    ctx: &mut LowerCtx<'_>,
    idx: u32,
    pat_v: ssa::ValueId,
    flag_v: ssa::ValueId,
) -> ssa::ValueId {
    let meta_sym = format!("___torajs_baked_regex_{idx}");
    let cur_block = ctx.cur_block;
    let meta_ptr = ctx
        .f
        .append_inst(cur_block, InstKind::GlobalRef(meta_sym), Type::Ptr, None);
    let cur_block = ctx.cur_block;
    ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.regex_compile_from_static_dfa,
            vec![
                Operand::Value(meta_ptr),
                Operand::Value(pat_v),
                Operand::Value(flag_v),
            ],
        ),
        Type::RegExp,
        None,
    )
}

fn emit_regex_compile(
    ctx: &mut LowerCtx<'_>,
    pat_v: ssa::ValueId,
    flag_v: ssa::ValueId,
) -> ssa::ValueId {
    let cur_block = ctx.cur_block;
    ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.regex_compile,
            vec![Operand::Value(pat_v), Operand::Value(flag_v)],
        ),
        Type::RegExp,
        None,
    )
}
