//! Pass 0 `declare_intrinsic` group: the kernels of the two lanes that
//! hold a regex match as spans on the stack — the exec-record lane
//! (`ssa_lower_exec_record`) and the replace-inline lane
//! (`ssa_lower_call_str_regex_emit::replace_inline`).
//!
//! - `regex_exec_span(re, subject, out, n_slots) -> hit` — one exec
//!   whose result is written as UTF-16 spans into the caller's stack
//!   record instead of a match array; lastIndex effects and the
//!   frozen-lastIndex TypeError are `regex_exec`'s, so the call is
//!   followed by a throw check.
//! - `str_group_span(subject, start, end) -> Str` — the string value
//!   of one span: a fresh Str of the subject's code units, or the Str
//!   `undefined` sentinel for a group that did not participate.
//! - `num_parse_int_span(subject, start, end, radix) -> F64` —
//!   `parseInt` of that same value read in place, no Str cut.
//! - `regex_replace_iter_init(iter, re, subject)` /
//!   `_next(iter, out, n_slots) -> hit` / `_close(iter)` — the
//!   replace-callback walk driven one match at a time; `init` may
//!   record the frozen-lastIndex TypeError, so it is followed by a
//!   throw check.
//! - `str_append_substr(a, view) -> Str` — `a ++ view` consuming `a`,
//!   the walk's output builder; `str_alloc_capacity(cap) -> Str` is
//!   the empty cell it starts from.

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

#[derive(Debug, Clone, Copy)]
pub(crate) struct ExecRecordIds {
    pub regex_exec_span: FuncId,
    pub str_group_span: FuncId,
    pub num_parse_int_span: FuncId,
    pub regex_replace_iter_init: FuncId,
    pub regex_replace_iter_next: FuncId,
    pub regex_replace_iter_close: FuncId,
    pub str_append_substr: FuncId,
    pub str_alloc_capacity: FuncId,
}

pub(crate) fn declare(
    module: &mut Module,
    fn_table: &mut HashMap<String, FuncId>,
) -> ExecRecordIds {
    macro_rules! decl {
        ($name:literal, [$($param:ident),*], $ret:ident) => {
            declare_intrinsic(module, fn_table, $name, &[$(Type::$param),*], Type::$ret)
        };
    }
    ExecRecordIds {
        regex_exec_span: decl!("__torajs_regex_exec_span", [RegExp, Str, Ptr, I64], I64),
        str_group_span: decl!("__torajs_str_group_span", [Str, I64, I64], Str),
        num_parse_int_span: decl!("__torajs_num_parse_int_span", [Str, I64, I64, I64], F64),
        regex_replace_iter_init: decl!(
            "__torajs_regex_replace_iter_init",
            [Ptr, RegExp, Str],
            Void
        ),
        regex_replace_iter_next: decl!("__torajs_regex_replace_iter_next", [Ptr, Ptr, I64], I64),
        regex_replace_iter_close: decl!("__torajs_regex_replace_iter_close", [Ptr], Void),
        str_append_substr: decl!("__torajs_str_append_substr", [Str, Ptr], Str),
        str_alloc_capacity: decl!("__torajs_str_alloc_capacity", [I64], Str),
    }
}
