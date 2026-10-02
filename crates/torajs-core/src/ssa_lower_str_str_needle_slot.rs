//! §22.1.3.{7,8,23} steps 3-5 then step 7 — the needle slot of
//! `String.prototype.{startsWith,endsWith,includes}`, lowered.
//!
//! This is the one search slot in the String lane that is NOT a plain
//! §7.1.17 ToString slot: `IsRegExp` runs FIRST and a regex raises a
//! TypeError before any coercion happens.
//! [`crate::ssa_lower_str_str_tostring_slot`] says in its own doc why
//! these three are kept off the ToString family, and that reading is
//! right. What it did not say is where they go INSTEAD — and the
//! answer, until this module, was nowhere: the slot fell through to
//! the argv walk's raw `lower_expr` tail, which hands whatever it got
//! to a helper whose ABI declares `Str`.
//!
//! So `"ab".startsWith(anyValue)` did not merely mis-answer a regex.
//! It SEGV'd for every needle that was not already a string —
//! `const n: any = "a"; "ab".startsWith(n)` included, which is as
//! ordinary as this call gets. `indexOf`, one line away in the same
//! walk and on the ToString road since rotation 678, answered it
//! correctly the whole time.
//!
//! That is the third form of the shape the last rotations kept
//! meeting: a family was named, and the members excluded from it were
//! never given anywhere else to go. Excluding something from a road
//! is only half a decision.
//!
//! ## What runs where
//!
//! | needle | decided | by |
//! |---|---|---|
//! | statically `RegExp` / `Symbol` | compile time | band B, `check_type_of_call_string_isregexp_slot` |
//! | statically nullish | compile time | the argv walk's substitution arm |
//! | statically `Str` / `Substr` | compile time | step 1 — a string is not an Object |
//! | anything else | runtime | `__torajs_anyv_reject_regexp_needle` then `coerce_to_str` |
//!
//! Only the last row costs anything, and the row above it is the one
//! nearly every program takes.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Lower the needle argument `a` of the `which`-th slot method into
/// the `Str` operand the helper ABI wants, raising per spec on the
/// way when the value is one the spec rejects.
///
/// A statically nullish needle never arrives: the argv walk's
/// `undef_to_str_at_arg0` substitution claims it several arms
/// earlier and hands over the §7.1.17 literal. That arm has to own
/// it, because `coerce_to_str` panics on a `Ptr` operand — at that
/// point `null` and `undefined` are indistinguishable, and the
/// static type is the last place they are not.
pub(crate) fn lower(ctx: &mut LowerCtx<'_>, a: ExprId, which: i64) -> Operand {
    let raw = ctx.lower_expr(a);
    let raw_ty = ctx.operand_ty(&raw);
    if matches!(raw_ty, Type::Str | Type::Substr) {
        // Step 1 short-circuit: a string is not an Object, so
        // `IsRegExp` is false and `ToString` is identity. The
        // overwhelmingly common needle therefore pays nothing — no
        // box, no call, no check.
        if ctx.expr_is_fresh_owned(a) {
            ctx.park_argv_owned(raw, raw_ty);
        }
        return raw;
    }
    // Everything else boxes ONCE and stays boxed for both steps.
    // An operand that is ALREADY `Any` must not be re-boxed:
    // `box_to_any`'s refcounted arm tags it 4 and calls the NaN-box
    // encoder, which would read a boxed double or int as a heap
    // pointer.
    // `ssa_lower_call_has_own` guards the same way.
    // Coercing from the box rather than from `raw_ty` is deliberate:
    // `coerce_to_str` supports a fixed list of typed shapes and
    // panics outside it, and reproducing that list here would be a
    // second copy of it — the exact thing this slot already suffered
    // from. The Any arm dispatches by tag at runtime and covers
    // every shape by construction, and this is the slow path anyway:
    // a string needle returned above without boxing at all.
    let boxed = if matches!(raw_ty, Type::Any) {
        raw
    } else {
        ctx.box_to_any(raw)
    };
    // Rotation 550's contract, widened by one check: the raw operand
    // has to be on the throw ledger BEFORE the first check, not
    // between the two. This slot raises TWICE — once for the regex
    // and once for a throwing `toString` — and a fresh-owned needle
    // (`"ab".startsWith(mk())`) unwinding out of the FIRST one has
    // nobody to drop it unless it is registered by then.
    let raw_owned = raw_ty.is_refcounted() && ctx.expr_is_fresh_owned(a);
    let raw_tok = raw_owned.then(|| ctx.push_throw_temp(raw, raw_ty));
    let cur = ctx.cur_block;
    let _ = ctx.f.append_inst(
        cur,
        InstKind::Call(
            ctx.intrinsics.anyv_reject_regexp_needle,
            vec![boxed, Operand::ConstI64(which)],
        ),
        Type::Void,
        None,
    );
    // Two raises ride out of that call — the step-5 TypeError and a
    // `@@match` getter's own — and this check observes either. It has
    // to come BEFORE the coercion: running `ToString` on a regex
    // after deciding to reject it would run a user `toString` the
    // spec never reaches.
    ctx.emit_throw_check(None);
    // The coerced Str is ours whether or not the toString threw
    // (kernel placeholder contract), so it joins the ledger too.
    let s = ctx.coerce_to_str(boxed, Type::Any);
    ctx.park_argv_owned(s, Type::Str);
    ctx.emit_throw_check(None);
    ctx.unpark_owned_temp(raw_tok);
    if raw_owned {
        ctx.emit_drop_value(raw, raw_ty);
    }
    s
}
