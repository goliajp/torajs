//! Any-tag inspection — `typeof v` + `console.log(v)` +
//! `ToBoolean(v)` on NaN-box [`AnyValue`] — port of
//! `runtime_str.c` L795-833 + L1040-1157.
//!
//! Two extern fns that read an [`AnyValue`]'s NaN-box discriminant
//! and route:
//!
//! - [`any::__torajs_anyv_typeof`] — returns a fresh Str holding
//!   the ES `typeof` result for the value (`"object"` /
//!   `"undefined"` / `"boolean"` / `"number"` / `"string"` /
//!   `"function"` / `"symbol"` / `"bigint"`).
//!
//! - [`any::__torajs_print_anyv`] — `console.log(v)` dispatch.
//!   Routes to the IR-emitted `print_i64` / `print_f64` /
//!   `print_bool` and `__torajs_str_print` based on the NaN-box
//!   predicate. Cell case reads the heap header's universal
//!   `type_tag`; Tag::Str / Tag::Arr / Tag::DynObj / Tag::Date /
//!   Tag::RegExp / Tag::Promise / Tag::Map / Tag::Set /
//!   Tag::Closure dispatch to typed walkers; everything else falls
//!   back to `"[object]\n"`.
//!
//! - [`tag_dispatch::__torajs_print_anyv_inline`] — nested-print
//!   substrate. Same dispatch tree as `__torajs_print_anyv` but
//!   every arm emits its bytes WITHOUT a trailing newline; used by
//!   typed composite walkers (`__torajs_arr_print_any` /
//!   `__torajs_obj_print_any` / etc) to format AnyValue cells
//!   inside a larger composite.
//!
//! Module split (file-size debt — 707 prod was over the 500 hard
//! limit):
//! - [`formatters`] — consts, extern decls, 0-libc byte writers,
//!   Str / Substr payload helpers shared across the two dispatch
//!   entry families.
//! - [`any`] — top-level entries (`__torajs_anyv_typeof`,
//!   `__torajs_print_anyv`, `__torajs_print_str_cell_unquoted`,
//!   `__torajs_fn_print_outer`); each owns the trailing newline.
//! - [`tag_dispatch`] — nested entry
//!   (`__torajs_print_anyv_inline`); no trailing newline, called
//!   from composite walkers.

pub mod any;
pub mod formatters;
pub mod line_est;
pub mod tag_dispatch;
pub mod typeof_any;
pub mod wrapper_block;
