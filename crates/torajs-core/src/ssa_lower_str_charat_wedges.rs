//! `charAt` / `charCodeAt` / `codePointAt` spec-default wedges for
//! Str | Substr receivers — second sub-split carved out of
//! [`ssa_lower_str::try_lower_method_call`] along the
//! `ssa_lower_str/{view,search,transform,structural}.rs` axis.
//!
//! Encapsulates two pre-dispatch wedges that share the
//! `charAt`/`charCodeAt`/`codePointAt` spec defaults:
//! - 0-arg form (ES §22.1.3.2 / §22.1.3.3 / §22.1.3.4 step 2-3:
//!   missing `pos` defaults to 0) — synthesizes a `ConstI64(0)`
//!   index and routes through the matching `__torajs_str_*` /
//!   `__torajs_substr_*` intrinsic.
//! - 1+ arg `charAt(i, ...trailing)` fast path (S272 trailing-arg
//!   ignore + S222 undefined / S332 Any operand decode) that emits a
//!   length-1 view slice rather than a separate runtime helper.
//!
//! Returns `None` when the receiver is not `Type::Str | Type::Substr`
//! or the method/arg shape does not match either wedge, so the
//! caller can fall through to the generic Str-path dispatch.

use crate::ast::ExprId;
use crate::ssa::{BinOp as SsaBinOp, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

/// Try to lower `<Str | Substr>.charAt(...)` /
/// `.charCodeAt(...)` / `.codePointAt(...)` through the
/// spec-default wedges. Returns `Some(value)` when one of the
/// wedges fired; `None` otherwise.
pub(crate) fn try_dispatch(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    args: &[ExprId],
    recv_op: Operand,
    recv_ty: Type,
) -> Option<Operand> {
    if !matches!(recv_ty, Type::Str | Type::Substr) {
        return None;
    }
    // V3-18 wedge — charAt / charCodeAt /
    // codePointAt 0-arg form per JS spec
    // §22.1.3.4 / §22.1.3.5 / §22.1.3.6: missing
    // pos defaults to 0. Synthesize a ConstI64(0)
    // index and route through the existing 1-arg
    // paths below.
    if matches!(method, "charAt" | "charCodeAt" | "codePointAt") && args.is_empty() {
        let idx_val = Operand::ConstI64(0);
        if method == "charAt" {
            let v = if recv_ty == Type::Str {
                ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.str_char_at, vec![recv_op, idx_val]),
                    Type::Substr,
                    None,
                )
            } else {
                ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(
                        ctx.intrinsics.substr_slice,
                        vec![recv_op, idx_val, Operand::ConstI64(1)],
                    ),
                    Type::Substr,
                    None,
                )
            };
            return Some(Operand::Value(v));
        }
        // P11.3-A1 — split charCodeAt vs codePointAt (the latter
        // combines surrogate pairs per ES §22.1.3.3). 0-arg form
        // still applies: `'😀'.codePointAt()` should default pos
        // to 0 and return 0x1F600, not 0xD83D.
        //
        // charCodeAt answers a Number that is NaN out of range
        // (§22.1.3.2 step 5), so its kernel hands back an `f64`;
        // codePointAt still rides the integer ABI here.
        let (target, ret) = if method == "codePointAt" {
            let fid = if recv_ty == Type::Str {
                ctx.intrinsics.str_code_point_at
            } else {
                ctx.intrinsics.substr_code_point_at
            };
            (fid, Type::I64)
        } else {
            let fid = if recv_ty == Type::Str {
                ctx.intrinsics.str_char_code_at
            } else {
                ctx.intrinsics.substr_char_code_at
            };
            (fid, Type::F64)
        };
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(target, vec![recv_op, idx_val]),
            ret,
            None,
        );
        return Some(Operand::Value(v));
    }
    // `s.charAt(i)` — same-shape alias for `s[i]`.
    // Lowers to a length-1 substr view instead of going
    // through a separate runtime helper.
    // S240 widens the 1-arg detection to 2-arg so the
    // charAt(idx, trailing) shape still routes through this length-1
    // substr-view fast path; args[1] is never lowered (trailing-arg
    // ignore).
    //
    // S272 — widen `== 1 || == 2` to `>= 1` so charAt(idx, ...trailing)
    // with any trailing count routes through this fast path; the
    // trailing exprs are eval-and-dropped below so side effects fire.
    if method == "charAt" && !args.is_empty() {
        // S222 — `s.charAt(undefined)` per ES §22.1.3.2 step 2-3:
        // ToIntegerOrInfinity(undefined)=0. Short-circuit to ConstI64(0)
        // before coerce_to_i64, which can't lower a ConstPtrNull undef
        // sentinel.
        let arg0_undef = matches!(
            ctx.expr_types.get(&args[0]),
            Some(crate::check::Type::Undefined)
        );
        // S332 — `s.charAt(x)` per ES §22.1.3.2: ToIntegerOrInfinity
        // accepts arbitrary-typed input, so the operand is COERCED
        // rather than shape-checked. `lower_to_number_operand` keeps a
        // Number on the typed-tier fast path (no box, no call) and
        // routes every other shape — Str, Bool, Any, a cell with a
        // `valueOf` — through the runtime's own ToNumber; the
        // `coerce_to_i64` below then folds NaN/±∞ per ToInteger so the
        // helper's `(Str|Substr, i64)` ABI sees a clean index.
        let idx_val = if arg0_undef {
            Operand::ConstI64(0)
        } else {
            let n = ctx.lower_to_number_operand(args[0]);
            ctx.coerce_to_i64(n)
        };
        let v = if recv_ty == Type::Str {
            // V3-18 m1.h.37 — bounds-checked str charAt.
            // Pre-fix called substr_create directly; OOB
            // indices stored garbage offsets and printed
            // bytes from past the parent's data.
            ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.str_char_at, vec![recv_op, idx_val]),
                Type::Substr,
                None,
            )
        } else {
            let end = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::BinOp(SsaBinOp::Add, idx_val, Operand::ConstI64(1)),
                Type::I64,
                None,
            );
            ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.substr_slice,
                    vec![recv_op, idx_val, Operand::Value(end)],
                ),
                Type::Substr,
                None,
            )
        };
        // S272 — eval-and-drop trailing exprs so side effects fire
        // per ES §22.1.3.2 trailing-arg ignore semantics.
        for &a in &args[1..] {
            let _ = ctx.lower_expr(a);
        }
        return Some(Operand::Value(v));
    }
    None
}

/// S7-r1 — inline `Substr.charCodeAt(i)` for a Latin-1 parent.
///
/// The kernel call (`__torajs_substr_char_code_at`, f64 ABI) priced
/// at 19% of the whole rpn probe after the S7-r2 SplitIter inline
/// scan landed — for a split token the per-call work is four field
/// loads plus one byte load, so the cross-archive BL (uninlinable,
/// S7 立项事实 4-①) dominates. This wedge emits the read inline,
/// versioned on the parent's encoding flag:
///
/// - out-of-range `i` → NaN (ES §22.1.3.2 step 5), bounds checked
///   BEFORE any parent-field read (strictly safer than the kernel,
///   which resolves the parent header even for OOB indices);
/// - Latin-1 parent → `LDRB` at `parent_data + offset + i`,
///   `SiToFp` to the same f64 the kernel answers;
/// - UTF-16 parent → the pre-existing kernel call (no u16 load
///   instruction exists, and the mixed-encoding shape is off every
///   measured hot path).
///
/// The old pre-P11.1-S2 inline was dropped as "encoding branch is
/// no cheaper than the call" — that held for a per-call header
/// probe against an INLINE kernel; against today's cross-archive
/// BL + f64 return the branch is one perfectly-predicted test.
///
/// Mirrors `substr_code_unit_at` (torajs-str/substr_methods.rs):
/// len u64 @8, parent ptr @16, code-unit offset u64 @24; Latin-1
/// stride 1 with payload at parent+16.
pub(crate) fn try_inline_substr_char_code_at(
    ctx: &mut LowerCtx<'_>,
    args: &[ExprId],
    recv_op: &Operand,
) -> Option<Operand> {
    if args.len() > 1 {
        return None;
    }
    // ES §22.1.3.2 step 2-3: missing / undefined pos → 0; anything
    // else runs ToIntegerOrInfinity (the charAt S332 coercion pair).
    let idx = if args.is_empty()
        || matches!(
            ctx.expr_types.get(&args[0]),
            Some(crate::check::Type::Undefined)
        ) {
        Operand::ConstI64(0)
    } else {
        let n = ctx.lower_to_number_operand(args[0]);
        ctx.coerce_to_i64(n)
    };
    let cur = ctx.cur_block;
    let result_slot = ctx
        .f
        .append_inst(cur, InstKind::Alloca(Type::F64), Type::Ptr, None);
    let len = ctx.f.append_inst(
        cur,
        InstKind::Load(Type::I64, recv_op.clone(), 8),
        Type::I64,
        None,
    );
    let ge_zero = ctx.f.append_inst(
        cur,
        InstKind::ICmp(IPred::Sge, idx.clone(), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    let lt_len = ctx.f.append_inst(
        cur,
        InstKind::ICmp(IPred::Slt, idx.clone(), Operand::Value(len)),
        Type::Bool,
        None,
    );
    let in_bounds = ctx.f.append_inst(
        cur,
        InstKind::BinOp(
            SsaBinOp::And,
            Operand::Value(ge_zero),
            Operand::Value(lt_len),
        ),
        Type::Bool,
        None,
    );
    let inb_blk = ctx.f.add_block();
    let oob_blk = ctx.f.add_block();
    let lat_blk = ctx.f.add_block();
    let slow_blk = ctx.f.add_block();
    let join_blk = ctx.f.add_block();
    ctx.f.set_term(
        cur,
        Terminator::CondBr {
            cond: Operand::Value(in_bounds),
            then_blk: inb_blk,
            else_blk: oob_blk,
        },
    );
    ctx.f.append_void(
        oob_blk,
        InstKind::Store(Operand::ConstF64(f64::NAN), Operand::Value(result_slot), 0),
    );
    ctx.f.set_term(oob_blk, Terminator::Br(join_blk));

    let parent = ctx.f.append_inst(
        inb_blk,
        InstKind::Load(Type::Ptr, recv_op.clone(), 16),
        Type::Ptr,
        None,
    );
    let hdr = ctx.f.append_inst(
        inb_blk,
        InstKind::Load(Type::I64, Operand::Value(parent), 0),
        Type::I64,
        None,
    );
    // STR_FLAG_IS_LATIN1 (0x0002) lives in the header's flags u16
    // at bits 48..64 of the packed u64.
    let flag = ctx.f.append_inst(
        inb_blk,
        InstKind::BinOp(
            SsaBinOp::And,
            Operand::Value(hdr),
            Operand::ConstI64(0x0002i64 << 48),
        ),
        Type::I64,
        None,
    );
    let is_lat = ctx.f.append_inst(
        inb_blk,
        InstKind::ICmp(IPred::Ne, Operand::Value(flag), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    ctx.f.set_term(
        inb_blk,
        Terminator::CondBr {
            cond: Operand::Value(is_lat),
            then_blk: lat_blk,
            else_blk: slow_blk,
        },
    );

    let off = ctx.f.append_inst(
        lat_blk,
        InstKind::Load(Type::I64, recv_op.clone(), 24),
        Type::I64,
        None,
    );
    let p_int = ctx.f.append_inst(
        lat_blk,
        InstKind::PtrToInt(Operand::Value(parent)),
        Type::I64,
        None,
    );
    let base_i = ctx.f.append_inst(
        lat_blk,
        InstKind::BinOp(SsaBinOp::Add, Operand::Value(p_int), Operand::ConstI64(16)),
        Type::I64,
        None,
    );
    let base = ctx.f.append_inst(
        lat_blk,
        InstKind::IntToPtr(Operand::Value(base_i)),
        Type::Ptr,
        None,
    );
    let j = ctx.f.append_inst(
        lat_blk,
        InstKind::BinOp(SsaBinOp::Add, Operand::Value(off), idx.clone()),
        Type::I64,
        None,
    );
    let b = ctx.f.append_inst(
        lat_blk,
        InstKind::LoadU8Dyn(Operand::Value(base), Operand::Value(j)),
        Type::I64,
        None,
    );
    let f = ctx.f.append_inst(
        lat_blk,
        InstKind::SiToFp(Operand::Value(b)),
        Type::F64,
        None,
    );
    ctx.f.append_void(
        lat_blk,
        InstKind::Store(Operand::Value(f), Operand::Value(result_slot), 0),
    );
    ctx.f.set_term(lat_blk, Terminator::Br(join_blk));

    let v = ctx.f.append_inst(
        slow_blk,
        InstKind::Call(
            ctx.intrinsics.substr_char_code_at,
            vec![recv_op.clone(), idx],
        ),
        Type::F64,
        None,
    );
    ctx.f.append_void(
        slow_blk,
        InstKind::Store(Operand::Value(v), Operand::Value(result_slot), 0),
    );
    ctx.f.set_term(slow_blk, Terminator::Br(join_blk));

    ctx.cur_block = join_blk;
    let out = ctx.f.append_inst(
        join_blk,
        InstKind::Load(Type::F64, Operand::Value(result_slot), 0),
        Type::F64,
        None,
    );
    Some(Operand::Value(out))
}
