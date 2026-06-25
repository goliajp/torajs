//! Per-method trailing-arg-ignore predicate — second carve-out chunk
//! pulled out of [`ssa_lower_str_str_dispatch::try_dispatch`].
//!
//! Encapsulates the family of "drop args beyond i=N" tests that ES
//! method specs prescribe for each `Str.<method>` whose helper ABI
//! has a fixed arity smaller than what the source call wrote. The
//! caller's per-arg loop consults [`should_drop`]; on `true` it
//! lower-and-drops the arg (per the S272 idiom, so step()-style
//! side-effect exprs still fire per ES eval-then-discard semantics)
//! and `continue`s to the next index without pushing into `argv`.
//!
//! Spec carve-outs (the S-number traces back the wedge to a specific
//! conformance fixture in repo history):
//!
//! - **S238** `localeCompare(other, locales?, options?)` — tora's
//!   bytewise helper has no Intl-locale awareness; drop `i > 0`.
//! - **S239** `indexOf` / `lastIndexOf` / `includes` / `startsWith`
//!   / `endsWith` (needle, fromIndex, …trailing) — `_from` helper
//!   ABI is `(Str, Str, I64)`; drop `i > 1`.
//! - **S240** `at` / `charAt` / `charCodeAt` / `codePointAt` /
//!   `repeat` / `normalize` / `search` (useful, …trailing) —
//!   helper ABI is 1-arg; drop `i > 0`.
//! - **S281** `trim` / `trimStart` / `trimEnd` / `trimLeft` /
//!   `trimRight` / `toUpperCase` / `toLowerCase` / `toWellFormed` /
//!   `isWellFormed` (…trailing) — spec-defined 0-arg methods; drop
//!   every operand. `toLocale{Upper,Lower}Case` excluded because
//!   the caller's `drop_args = true` path skips the entire arg
//!   loop and never reaches this predicate.
//! - **S241** `slice` / `substring` / `substr` / `padStart` /
//!   `padEnd` (a, b, …trailing) — helper ABI is 2-arg;
//!   drop `i > 1`.
//! - **S282** `replace` / `replaceAll` / `split` (useful, useful,
//!   …trailing) — helpers are 2-arg only (`str_replace` /
//!   `str_replace_all` / `str_split`); drop `i > 1`.
//!
//! S272/S278/S284/S285 history: each of these wedges originally
//! used `break` (silent drop). The lower-and-drop idiom replaced
//! `break` so the discarded exprs still execute.

/// Returns `true` when the `i`-th positional argument of a
/// `<Str>.<method>(...)` call should be lower-and-dropped per the
/// trailing-arg-ignore spec carve-outs documented above.
pub(crate) fn should_drop(method: &str, i: usize) -> bool {
    // S238
    if method == "localeCompare" && i > 0 {
        return true;
    }
    // S239
    if matches!(
        method,
        "indexOf" | "lastIndexOf" | "includes" | "startsWith" | "endsWith"
    ) && i > 1
    {
        return true;
    }
    // S240
    if matches!(
        method,
        "at" | "charAt" | "charCodeAt" | "codePointAt" | "repeat" | "normalize" | "search"
    ) && i > 0
    {
        return true;
    }
    // S281 — 0-arg methods drop every operand
    if matches!(
        method,
        "trim"
            | "trimStart"
            | "trimEnd"
            | "trimLeft"
            | "trimRight"
            | "toUpperCase"
            | "toLowerCase"
            | "toWellFormed"
            | "isWellFormed"
    ) {
        return true;
    }
    // S241
    if matches!(
        method,
        "slice" | "substring" | "substr" | "padStart" | "padEnd"
    ) && i > 1
    {
        return true;
    }
    // S282
    if matches!(method, "replace" | "replaceAll" | "split") && i > 1 {
        return true;
    }
    false
}
