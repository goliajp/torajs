//! Per-method missing-arg default-fill — third carve-out chunk pulled
//! out of [`ssa_lower_str_str_dispatch::try_dispatch`].
//!
//! After the per-arg loop has lowered every supplied argument into
//! `argv`, several `Str.<method>` shapes need additional positional
//! slots appended to match the runtime helper's fixed ABI. These are
//! purely spec-default substitutions — never inspect any operand's
//! type, only count the supplied args.
//!
//! Spec carve-outs:
//!
//! - **V3-18 m1.h.36** — `slice` / `substring` with 0 or 1 args:
//!   missing start → 0, missing end → `recv.length` (loaded from the
//!   str header offset 8, same shape as `s.length` elsewhere).
//! - **T-49** — `substr` with 0 or 1 args: substr's 2nd slot is a
//!   *length* not an end index; missing length means "remaining", so
//!   push `i64::MAX` and the runtime helper's `length > avail ?
//!   avail` clamp picks it up.
//! - **V3-18 m1.h.45 + S201** — `padStart` / `padEnd`: 0-arg pushes
//!   `(maxLen=0, fill=" ")` so the helper takes the no-pad path
//!   (`ToLength(undefined)=0`, step 2 returns S unchanged because
//!   `0 <= S.length`); 1-arg supplies the default fill `" "`
//!   (§21.1.3.16).
//! - **ES §22.1.3.1** — `s.at()` defaults to `s.at(0)` per
//!   ToIntegerOrInfinity(undefined)=0.
//! - **ES §22.1.3.{8,13,14,21,22}** — 0-arg `indexOf` / `lastIndexOf`
//!   / `includes` / `startsWith` / `endsWith` push the literal
//!   "undefined" so the runtime helpers see a valid Str needle
//!   (matches bun: returns -1 / false unless haystack contains
//!   "undefined").
//! - **S207** — `replace` / `replaceAll` with fewer-than-2 args:
//!   missing slots default to undefined, ToString'd to "undefined".
//!   0-arg pushes "undefined" twice; 1-arg pushes it once for the
//!   missing replaceValue.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Append missing positional slots into `argv` to match each Str
/// method's runtime helper ABI. `args.len()` is the source arg count
/// (`argv.len()` includes the receiver at slot 0 plus the lowered
/// args). Mutates `argv` in place.
pub(crate) fn fill_missing(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    args: &[ExprId],
    recv_op: Operand,
    argv: &mut Vec<Operand>,
) {
    // V3-18 m1.h.36 — slice / substring 0-1 arg defaults
    if matches!(method, "slice" | "substring") && args.len() < 2 {
        if args.is_empty() {
            argv.push(Operand::ConstI64(0));
        }
        let len = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, recv_op, 8),
            Type::I64,
            None,
        );
        argv.push(Operand::Value(len));
    }
    // T-49 — substr 0/1-arg defaults (2nd slot is *length*, not end)
    if method == "substr" && args.len() < 2 {
        if args.is_empty() {
            argv.push(Operand::ConstI64(0));
        }
        argv.push(Operand::ConstI64(i64::MAX));
    }
    // V3-18 m1.h.45 + S201 — padStart / padEnd defaults
    if matches!(method, "padStart" | "padEnd") {
        if args.is_empty() {
            argv.push(Operand::ConstI64(0));
        }
        if args.len() <= 1 {
            let space = ctx.intern_string_literal(" ");
            argv.push(Operand::Value(space));
        }
    }
    // ES §22.1.3.1 — `s.at()` defaults to `s.at(0)`
    if method == "at" && args.is_empty() {
        argv.push(Operand::ConstI64(0));
    }
    // ES §22.1.3.{8,13,14,21,22} — 0-arg search-string default
    if matches!(
        method,
        "indexOf" | "lastIndexOf" | "includes" | "startsWith" | "endsWith"
    ) && args.is_empty()
    {
        let u = ctx.intern_string_literal("undefined");
        argv.push(Operand::Value(u));
    }
    // S207 — replace / replaceAll < 2-arg defaults
    if matches!(method, "replace" | "replaceAll") && args.len() < 2 {
        let u = ctx.intern_string_literal("undefined");
        if args.is_empty() {
            argv.push(Operand::Value(u));
        }
        argv.push(Operand::Value(u));
    }
}
