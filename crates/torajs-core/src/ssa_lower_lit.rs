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
//!   3. **Build-time compile** — the literal is compiled and baked
//!      into the binary by
//!      [`crate::ssa_lower_regex_bake::bake_literal`] (image, plus the
//!      DFA table of a capture-free eligible literal); the call is
//!      `__torajs_regex_compile_baked`, which decodes the image
//!      instead of parsing the pattern. `new RegExp(...)` with a
//!      run-time pattern keeps the parsing `regex_compile`.

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
        InstKind::BoxAny(5, Operand::ConstI64(0)),
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
    let idx = crate::ssa_lower_regex_bake::bake_literal(ctx.baked_regex_buf, pattern, flags);
    let has_dfa = ctx.baked_regex_buf[idx as usize].dfa.is_some();
    let matcher = |wide: bool| {
        ctx.fn_table
            .get(&crate::ssa_lower_regex_bake::matcher::matcher_key(
                pattern, flags, wide,
            ))
            .copied()
    };
    let (narrow, wide) = (matcher(false), matcher(true));
    let v = emit_baked_compile(ctx, idx, has_dfa, narrow, wide, pat_v);
    ctx.cur_block = saved_block;
    ctx.regex_lit_cache.insert(key, v);
    Operand::Value(v)
}

/// The literal's construction from its image `idx`: the address of
/// its Latin-1 and its UTF-16 one-pass matcher (null where the lane
/// declined an encoding) and of its baked DFA table (null without
/// one), then the source Str `source` reads back.
fn emit_baked_compile(
    ctx: &mut LowerCtx<'_>,
    idx: u32,
    has_dfa: bool,
    narrow: Option<ssa::FuncId>,
    wide: Option<ssa::FuncId>,
    pat_v: ssa::ValueId,
) -> ssa::ValueId {
    let cur_block = ctx.cur_block;
    let mut global = |sym: String| {
        Operand::Value(
            ctx.f
                .append_inst(cur_block, InstKind::GlobalRef(sym), Type::Ptr, None),
        )
    };
    let image = global(ssa::BakedRegexEntry::image_sym(idx));
    let meta = if has_dfa {
        global(format!("___torajs_baked_regex_{idx}"))
    } else {
        Operand::ConstPtrNull
    };
    let mut addr = |fid: Option<ssa::FuncId>| match fid {
        Some(fid) => {
            let sig = ctx.fn_sig_ids[&fid];
            let v = ctx
                .f
                .append_inst(cur_block, InstKind::FnAddr(fid), Type::FnSig(sig), None);
            Operand::Value(v)
        }
        None => Operand::ConstPtrNull,
    };
    let latin1 = addr(narrow);
    let utf16 = addr(wide);
    ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.regex_compile_baked,
            vec![image, latin1, utf16, meta, Operand::Value(pat_v)],
        ),
        Type::RegExp,
        None,
    )
}
