//! Which positional slot of a `<Str>.<method>(...)` call is a §7.1.17
//! `ToString` slot — the string mirror of
//! [`crate::ssa_lower_str_str_argv_numslot`]'s ToNumber family.
//!
//! A slot whose spec step is `ToString` reaches EVERY value:
//! `'x'.padStart(4, 12)` is `"121x"`, `'abc'.indexOf({toString(){return
//! 'b'}})` is 1, `'a'.normalize(null)` is a RangeError raised by the
//! kernel and not a compile-time refusal. The lane's argv walk used to
//! name `replace` / `replaceAll` here and leave the rest on the raw
//! `lower_expr` fall-through, which hands a non-Str operand to a helper
//! whose ABI declares `Str`: rotation 677 widened the `padStart` fill
//! CHECKER without this, and the program exited after its first line
//! with no message at all.
//!
//! Naming the family once is what keeps the checker gate and the
//! lowering road from being widened one without the other — the shape
//! this rotation kept meeting (a rule written in one place and applied
//! to half of what it covers).
//!
//! Deliberately NOT here:
//! - `startsWith` / `endsWith` / `includes` arg 0 — §22.1.3.{7,23,8}
//!   run `IsRegExp` BEFORE `ToString` and raise a TypeError on a
//!   regex, so their slot is not a plain ToString slot.
//! - `split` arg 0 — a user `@@split` claims the call first.

/// `true` when argument `i` of `<Str>.<method>(...)` is a §7.1.17
/// ToString slot, i.e. the lowering must coerce whatever it gets
/// rather than trust a checker gate to have proved it a `Str`.
pub(crate) fn is_tostring_slot(method: &str, i: usize) -> bool {
    match method {
        // §22.1.3.19 steps 3/6 — searchValue and replaceValue.
        "replace" | "replaceAll" => i < 2,
        // §22.1.3.16.1 step 5 — the fill string.
        "padStart" | "padEnd" => i == 1,
        // §22.1.3.14 step 3 — the normalization form. (Its lowering
        // lives in `ssa_lower_str_short_circuits`, not the argv walk;
        // the predicate is shared so the two cannot disagree.)
        "normalize" => i == 0,
        // §22.1.3.9 step 3 / §22.1.3.11 step 3 — the search string.
        "indexOf" | "lastIndexOf" => i == 0,
        _ => false,
    }
}
