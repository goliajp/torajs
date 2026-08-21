//! Per-element compare helper for the
//! [`ssa_lower_str_arr_index_search`] inline scan loop.
//!
//! Carved out as the first axis of the planned 3-way second-pass
//! split documented in that file's module doc
//! (needle_coerce / from_normalize / compare_dispatch). Lifts the
//! ~150-LOC `let eq = match elem_ty { ... }` block into a single
//! `emit_compare` free fn so the parent dispatch shrinks under the
//! 500-LOC hard cap.
//!
//! Per-elem-ty semantics handled here:
//!
//! - `Type::F64` with `want_bool=true` (i.e. `arr.includes(needle)`):
//!   widens IEEE 754 `Oeq` to SameValueZero per ES §23.1.3.16 (NaN
//!   equals NaN). The +0 / -0 case is already covered by `Oeq`.
//! - `Type::F64` with `want_bool=false`: plain strict
//!   `FCmp::Oeq` (NaN never matches).
//! - `Type::Str`: `__torajs_str_eq` Call.
//! - `Type::Any`: routes through the boxed strict-eq helpers
//!   (`any_any_strict_eq` when needle is also Any, otherwise
//!   `any_strict_eq` with packed `(tag, value-as-i64)` shape).
//!   Includes S127-1 `undefined` literal recovery — the
//!   `ConstPtrNull` shared by `null` and `undefined` is
//!   disambiguated via the original AST node so the tag is `5`
//!   (ANY_UNDEF) rather than `0` (ANY_NULL).
//! - default (I64 / Bool / etc.): plain `ICmp::Eq`.
//!
//! Caller must ensure `args[0]` is the original needle expression
//! (used for the S127-1 AST-side undef-vs-null distinction).

use crate::ast::{Expr, ExprId};
use crate::ssa::{BinOp as SsaBinOp, FPred, IPred, InstKind, Operand, Type, ValueId};
use crate::ssa_lower::LowerCtx;

/// Emit the per-element compare for the indexOf/lastIndexOf/includes
/// inline scan loop. Returns the resulting `Bool` SSA value.
///
/// - `elem`     SSA value carrying the loaded element (Type = `elem_ty`).
/// - `elem_ty`  receiver-array element type.
/// - `needle`   already-coerced needle operand (caller resolves needle ↔
///              elem-ty mismatch + const-fold short-circuits before
///              calling).
/// - `needle_ty` original needle type, used by the Any arm to pick
///              the packed `(tag, value-as-i64)` shape.
/// - `want_bool` true when called from `.includes()`; widens F64
///              equality to SameValueZero.
/// - `needle_eid` original needle ExprId, only consulted for the
///              S127-1 `undefined` literal recovery in the Any arm.
pub(crate) fn emit_compare(
    ctx: &mut LowerCtx<'_>,
    elem: ValueId,
    elem_ty: Type,
    needle: Operand,
    needle_ty: Type,
    want_bool: bool,
    needle_eid: ExprId,
) -> ValueId {
    if matches!(needle_ty, Type::Any) && !matches!(elem_ty, Type::Any) {
        return emit_compare_any_needle(ctx, elem, elem_ty, needle, want_bool);
    }
    match elem_ty {
        // ES §23.1.3.16: `includes` uses SameValueZero, which
        // treats NaN as equal to NaN. IEEE 754 `fcmp oeq` is
        // unordered-rejects-NaN, so plain `Oeq(NaN, NaN)` would
        // wrongly miss. `indexOf` / `lastIndexOf` keep
        // StrictEqualityComparison (NaN never matches) — so
        // only the `includes` arm widens to SameValueZero.
        // +0 / -0: IEEE 754 fcmp oeq treats them equal already,
        // matching SameValueZero's +0 === -0.
        Type::F64 if want_bool => {
            let eq_ord = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::FCmp(FPred::Oeq, Operand::Value(elem), needle.clone()),
                Type::Bool,
                None,
            );
            // x != x is true exactly when x is NaN (FPred::Une
            // = unordered or not equal; on a self-compare the
            // unordered bit is the only source of truth).
            let elem_nan = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::FCmp(FPred::Une, Operand::Value(elem), Operand::Value(elem)),
                Type::Bool,
                None,
            );
            let needle_nan = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::FCmp(FPred::Une, needle.clone(), needle),
                Type::Bool,
                None,
            );
            let both_nan = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::BinOp(
                    SsaBinOp::And,
                    Operand::Value(elem_nan),
                    Operand::Value(needle_nan),
                ),
                Type::Bool,
                None,
            );
            ctx.f.append_inst(
                ctx.cur_block,
                InstKind::BinOp(
                    SsaBinOp::Or,
                    Operand::Value(eq_ord),
                    Operand::Value(both_nan),
                ),
                Type::Bool,
                None,
            )
        }
        Type::F64 => ctx.f.append_inst(
            ctx.cur_block,
            InstKind::FCmp(FPred::Oeq, Operand::Value(elem), needle),
            Type::Bool,
            None,
        ),
        Type::Str => ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.str_eq, vec![Operand::Value(elem), needle]),
            Type::Bool,
            None,
        ),
        // A split product is an array of substring VIEWS; the needle
        // reaches here as an owned Str (the coerce layer treats the
        // two as one family). Without this arm the view fell to the
        // pointer ICmp below and `"p q r".split(" ").indexOf("q")`
        // answered -1 — on `let` and `const` alike.
        Type::Substr => ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.substr_eq_str,
                vec![Operand::Value(elem), needle],
            ),
            Type::Bool,
            None,
        ),
        // T-48 — Array<Any> per-element compare must go
        // through the boxed-any strict-eq helpers, not
        // raw ICmp. The elem is always a heap-Box ptr
        // (Type::Any); the needle may be either Any
        // (another box ptr) or a concrete primitive.
        // Pre-fix this arm fell through to ICmp(Ptr, I64)
        // when needle was a primitive, producing
        // "LLVM verify: Both operands to ICmp instruction
        // are not of the same type!" 3 cases under
        // test/built-ins/Array/prototype/{includes,
        // indexOf, lastIndexOf}/*.
        Type::Any => {
            if matches!(needle_ty, Type::Any) {
                ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(
                        ctx.intrinsics.any_any_strict_eq,
                        vec![Operand::Value(elem), needle],
                    ),
                    Type::Bool,
                    None,
                )
            } else {
                // Pack needle as (tag, value-as-i64) — same
                // shape as the BinOp Any === concrete arm.
                // S127-1: `undefined` literal lowers to
                // ConstPtrNull (Type::Ptr) — identical shape
                // to the `null` literal. Recover the original
                // AST distinction so strict_eq packs tag=5
                // (ANY_UNDEF) instead of tag=0 (ANY_NULL).
                // Without this, `xs.indexOf(undefined)` /
                // `.lastIndexOf(undefined)` / `.includes(undefined)`
                // wrongly match the first null slot — same
                // ANY_UNDEF vs ANY_NULL collapse seen in W-D
                // narrow trunk (S126-1/-3).
                let is_undef_lit = matches!(
                    ctx.ast.get_expr(needle_eid),
                    Expr::Ident(n) if n == "undefined"
                );
                let (tag, value): (i64, Operand) = if is_undef_lit {
                    (5, Operand::ConstI64(0))
                } else {
                    match needle_ty {
                        Type::I64 | Type::I32 => (2, needle.clone()),
                        Type::F64 => {
                            let bits = ctx.f.append_inst(
                                ctx.cur_block,
                                InstKind::BitCastF64ToI64(needle.clone()),
                                Type::I64,
                                None,
                            );
                            (3, Operand::Value(bits))
                        }
                        Type::Bool => {
                            let zext = ctx.f.append_inst(
                                ctx.cur_block,
                                InstKind::ZExtBoolToI64(needle.clone()),
                                Type::I64,
                                None,
                            );
                            (1, Operand::Value(zext))
                        }
                        Type::Ptr if matches!(needle, Operand::ConstPtrNull) => {
                            (0, Operand::ConstI64(0))
                        }
                        t if t.is_refcounted() => (4, needle.clone()),
                        _ => (0, Operand::ConstI64(0)),
                    }
                };
                ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(
                        ctx.intrinsics.any_strict_eq,
                        vec![Operand::Value(elem), Operand::ConstI64(tag), value],
                    ),
                    Type::Bool,
                    None,
                )
            }
        }
        _ => ctx.f.append_inst(
            ctx.cur_block,
            InstKind::ICmp(IPred::Eq, Operand::Value(elem), needle),
            Type::Bool,
            None,
        ),
    }
}

/// An `any` needle over a typed-element receiver (checker
/// index-search Any admit) — strict equality is by-tag, NOT a
/// ToNumber coercion (`[1,2,3].indexOf("2" as any)` is -1), so the
/// ELEMENT packs as the `(tag, value)` pair and the boxed needle
/// stays whole: the reverse of emit_compare's Any-elem arm.
/// `includes` rides the SameValueZero entry (§23.1.3.16 — NaN equals
/// NaN); indexOf / lastIndexOf keep strict (§7.2.15).
fn emit_compare_any_needle(
    ctx: &mut LowerCtx<'_>,
    elem: ValueId,
    elem_ty: Type,
    needle: Operand,
    want_bool: bool,
) -> ValueId {
    let (tag, value): (i64, Operand) = match elem_ty {
        Type::I64 => (2, Operand::Value(elem)),
        Type::F64 => {
            let bits = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::BitCastF64ToI64(Operand::Value(elem)),
                Type::I64,
                None,
            );
            (3, Operand::Value(bits))
        }
        Type::Bool => {
            let zext = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::ZExtBoolToI64(Operand::Value(elem)),
                Type::I64,
                None,
            );
            (1, Operand::Value(zext))
        }
        // Str and every other refcounted elem: the kernel's cell row
        // does byte-equality for Str (ShortStr-box × heap-Str
        // crossings included) and identity otherwise.
        _ => (4, Operand::Value(elem)),
    };
    let fid = if want_bool {
        ctx.intrinsics.any_svz
    } else {
        ctx.intrinsics.any_strict_eq
    };
    ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(fid, vec![needle, Operand::ConstI64(tag), value]),
        Type::Bool,
        None,
    )
}
