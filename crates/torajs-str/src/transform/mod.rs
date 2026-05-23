//! Str transformation ops — case / trim / pad / build / replace.
//!
//! Per the P3.1-e sub-step matrix (see [`crate`] docs):
//!
//! | Sub-step  | Module       | Surface                                                 |
//! |-----------|--------------|---------------------------------------------------------|
//! | P3.1-e.1  | [`case`]     | `s.toUpperCase()` / `s.toLowerCase()` (ASCII-only fold) |
//! | P3.1-e.2  | (trim)       | `s.trim()` / `trimStart()` / `trimEnd()`                |
//! | P3.1-e.3  | (pad)        | `s.padStart(n, fill)` / `padEnd(n, fill)`               |
//! | P3.1-e.4  | [`construct`]| `s.repeat(n)` / `charAt` / `at` / `fromCharCode` / ...  |
//! | P3.1-e.5  | (replace)    | `s.replace(needle, repl)` / `replaceAll(...)`           |
//!
//! Each sub-module is independently shippable. Heap allocations all
//! flow through [`crate::alloc::StrBlock`]; non-ASCII bytes pass
//! through unchanged on case-fold paths (matches the C-side subset
//! contract documented at the original `runtime_str.c` impl).
//!
//! Module-level layout helpers (`str_len` / `str_bytes`) are
//! intentionally **replicated per sub-module** rather than promoted
//! to a shared `pub(crate)` helper. Each sub-module is small (≤ 200
//! LOC) so the duplication cost is one inline fn pair; the win is
//! that each sub-module is self-contained and can be deleted in
//! isolation when its IR-side counterpart consolidates (P3.1-g).

pub mod case;
pub mod construct;
pub mod pad;
pub mod replace;
pub mod trim;
