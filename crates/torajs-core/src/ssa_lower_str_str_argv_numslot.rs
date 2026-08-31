//! The numeric slots of `<Str>.<method>(args)` — every position whose
//! spec step is `ToIntegerOrInfinity` / `ToLength` / `ToUint32`, split
//! out of [`super::ssa_lower_str_str_argv::populate_argv`] where they
//! had grown to be most of the cascade.
//!
//! What they share is that the operand is COERCED, not shape-checked:
//! `'abcd'.charAt('x')` is `'a'` and `'abc'.indexOf('c', '1')` is 2,
//! neither a type error. What separates them is the reading of the
//! two values ToNumber can produce that are not positions:
//!
//! - `NaN → 0` and `undefined → 0` — the plain case, and the only one
//!   [`crate::ssa_lower::LowerCtx::lower_to_index_operand`] answers on
//!   its own: `at` / `charCodeAt` / `codePointAt` / `repeat`, the
//!   `slice` / `substring` / `substr` positions, `padStart` /
//!   `padEnd`, and the `indexOf` / `includes` / `startsWith` position.
//! - `undefined` means "to the end" — the `slice` / `substring` /
//!   `substr` END slot (§22.1.3.{23,24} step 4, §B.2.2.1 step 5),
//!   `endsWith` (§22.1.3.7 step 5) and `split`'s limit
//!   (§22.1.3.23 step 3). NaN must NOT take that path, so the test is
//!   on the any TAG.
//! - `NaN` itself means +∞ — `lastIndexOf` (§22.1.3.10 steps 5-6),
//!   where the test is on the NUMBER and covers `Type::Number` too, a
//!   literal `NaN` being one.
//!
//! The statically-`undefined` spellings never reach here: the parent's
//! substitution arms claim them and answer the same defaults as
//! constants. Neither does a `split` whose separator may carry a user
//! `@@split`, whose step 2 hands the limit over before anything
//! coerces it.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Lower `args[i]` if it is one of the numeric slots; `None` if this
/// method and position is not one, leaving the parent's fallback to
/// push the operand as it lowered. `recv_op` and `substring_len_op`
/// are the parent's lazily-loaded receiver length, shared so a
/// two-slot call loads it once.
pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    i: usize,
    a: ExprId,
    args: &[ExprId],
    recv_op: &Operand,
    substring_len_op: &mut Option<Operand>,
) -> Option<Operand> {
    if matches!(method, "at" | "charCodeAt" | "codePointAt" | "repeat")
        && i == 0
        && !matches!(ctx.expr_types.get(&a), Some(crate::check::Type::Number))
    {
        // S332 — `s.{at,charCodeAt,codePointAt,repeat}(x)`:
        // ToIntegerOrInfinity COERCES its operand, so every shape
        // but Number (which stays on the typed-tier fast path)
        // routes through the runtime's own ToNumber. Rotation 463
        // widened this from an `Any`-only admission — the checker
        // mirror in `check_type_of_call_string_char_any` dropped
        // the matching shape gate, so `'abc'.charCodeAt('1')` is
        // 98 rather than a compile-time refusal.
        return Some(ctx.lower_to_index_operand(a));
    } else if matches!(method, "slice" | "substring" | "substr")
        && i == 1
        && matches!(ctx.expr_types.get(&a), Some(crate::check::Type::Any))
    {
        // Rotation 543 — §22.1.3.{23,24} step 4 and §B.2.2.1
        // step 5 both read an `undefined` second slot as "to the
        // end of the string", NOT as ToIntegerOrInfinity's 0. The
        // STATIC spelling already answers that (`slice_subs_2arg`
        // above loads the receiver's length); a value that only
        // turns out to be undefined at RUN time fell through to
        // the plain coercion and answered "".
        //
        // Found because rotation 543's mixed-element-type fix made
        // `[...numbers, undefined]` actually hold `undefined`
        // instead of `0`. test262's substr coverage builds its
        // argument matrix out of exactly that array and had been
        // passing only because its own reference implementation
        // read the same wrong `0` — both sides wrong, agreeing.
        //
        // NaN must NOT take this path (`slice(0, NaN)` is ""), so
        // the test is on the any TAG, not on the number.
        let (tag, _, idx) = ctx.any_slot_tag_number_index(a);
        if substring_len_op.is_none() {
            let len = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::I64, recv_op.clone(), 8),
                Type::I64,
                None,
            );
            *substring_len_op = Some(Operand::Value(len));
        }
        let sel = ctx.select_on_undef_tag(
            tag,
            substring_len_op.clone().unwrap(),
            idx,
            "__str_end_slot",
        );
        return Some(sel);
    } else if matches!(method, "slice" | "substring" | "substr")
        && i < 2
        && matches!(ctx.expr_types.get(&a), Some(crate::check::Type::Any))
    {
        // S333 — `s.{slice,substring,substr}(Any, Any)`
        // ToIntegerOrInfinity accepts arbitrary-typed input on
        // both positional slots.
        return Some(ctx.lower_to_index_operand(a));
    } else if matches!(method, "padStart" | "padEnd")
        && i == 0
        && matches!(ctx.expr_types.get(&a), Some(crate::check::Type::Any))
    {
        // S338 — `s.{padStart,padEnd}(Any [, fillStr])`
        // ToLength accepts arbitrary-typed input.
        return Some(ctx.lower_to_index_operand(a));
    } else if matches!(method, "endsWith" | "split")
        && i == 1
        && !matches!(ctx.expr_types.get(&a), Some(crate::check::Type::Number))
        && !split_defers_to_user_splitter(ctx, method, args)
    {
        // Rotation 544 — §22.1.3.7 step 5 and §22.1.3.23 step 3
        // both test the slot for `undefined` ITSELF, not for the
        // NaN its ToNumber would give: `'abc'.endsWith('c',
        // undefined)` is true and `'abc'.endsWith('c', NaN)` is
        // false; `'a,b,c'.split(',', undefined)` is the whole
        // split and `split(',', NaN)` is `[]`. So the runtime
        // test is on the any TAG, the same shape the slice /
        // substring / substr end slot takes, and the `i64::MAX`
        // sentinel is what the static-undefined arm above
        // already hands the helper for "no limit".
        // A statically-shaped operand that is not Number is also not
        // `undefined` — that spelling is claimed by the
        // `undef_max_at_arg1` arm above — so only an any box owes a
        // run-time test, which is what the shared lane decides.
        return Some(ctx.lower_to_index_or_undef_default(
            a,
            Operand::ConstI64(i64::MAX),
            "__str_pos_slot",
        ));
    } else if method == "lastIndexOf" && i == 1 {
        // Rotation 544 — §22.1.3.10 steps 5-6 read a NaN position
        // as +∞, so `'abcabc'.lastIndexOf('a', NaN)` is 3, not
        // `coerce_to_i64`'s 0 — and so is the `undefined`
        // spelling, by way of the same NaN. The test is on the
        // NUMBER, which is why this slot cannot share the
        // endsWith / split arm above, and it covers EVERY shape:
        // a literal `NaN` is a `Type::Number` and reaches it just
        // as `'zz'` and an any box do.
        return Some(lower_lastindexof_pos(ctx, a));
    } else if matches!(method, "indexOf" | "includes" | "startsWith")
        && i == 1
        && !matches!(ctx.expr_types.get(&a), Some(crate::check::Type::Number))
    {
        // Rotation 544 — §22.1.3.{8,14,22} read this slot with a
        // plain ToIntegerOrInfinity, whose NaN and whose
        // `undefined` are both 0, so no run-time test is owed and
        // the shared coercion is the whole answer.
        return Some(ctx.lower_to_index_operand(a));
    }
    None
}

/// Whether this `split` call will hand its arguments to a user
/// `@@split` instead of the kernel — the same question
/// [`crate::ssa_lower_str_str_split::lower_split`] asks, asked one
/// step earlier.
///
/// §22.1.3.23 step 2 dispatches on the separator BEFORE step 3
/// coerces the limit, and passes «O, limit» raw. Coercing here would
/// run a user `valueOf` on the limit that the spec never runs, and
/// hand the splitter a number where it must see the object it was
/// given. `endsWith` has no such step, so it never defers.
fn split_defers_to_user_splitter(ctx: &LowerCtx<'_>, method: &str, args: &[ExprId]) -> bool {
    method == "split"
        && args
            .first()
            .is_some_and(|a| matches!(ctx.expr_types.get(a), Some(crate::check::Type::Any)))
        && crate::check_type_of_call_string_match::any_pattern_may_carry_matcher(ctx.ast, args[0])
}

/// The `lastIndexOf` position slot: `ToNumber(pos)` is NaN → +∞,
/// otherwise `ToIntegerOrInfinity(pos)` (§22.1.3.10 steps 5-6). The
/// `i64::MAX` sentinel is what the helper already takes for +∞.
///
/// An integer-shaped operand can never be NaN, and a constant one
/// answers at compile time, so only an `F64` value pays for the test.
fn lower_lastindexof_pos(ctx: &mut LowerCtx<'_>, a: ExprId) -> Operand {
    let n = ctx.lower_to_number_operand(a);
    if let Operand::ConstF64(v) = n {
        return if v.is_nan() {
            Operand::ConstI64(i64::MAX)
        } else {
            ctx.coerce_to_i64(n)
        };
    }
    if !matches!(ctx.operand_ty(&n), Type::F64) {
        return ctx.coerce_to_i64(n);
    }
    let idx = ctx.coerce_to_i64(n.clone());
    select_on_nan(ctx, n, Operand::ConstI64(i64::MAX), idx, "__str_pos_slot")
}

/// `<n is NaN> ? default : idx`, the number-side twin of
/// [`crate::ssa_lower::LowerCtx::select_on_undef_tag`] for a slot
/// whose spec reads NaN itself as
/// the default (`Une` is the unordered `n != n`).
fn select_on_nan(
    ctx: &mut LowerCtx<'_>,
    n: Operand,
    default: Operand,
    idx: Operand,
    slot_name: &str,
) -> Operand {
    let is_nan = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::FCmp(crate::ssa::FPred::Une, n.clone(), n),
        Type::Bool,
        None,
    );
    ctx.select_i64(Operand::Value(is_nan), default, idx, slot_name)
}
