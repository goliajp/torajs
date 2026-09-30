//! Pass 0 `declare_intrinsic` group: the exec-record lane's two
//! kernels (`ssa_lower_exec_record`).
//!
//! - `regex_exec_span(re, subject, out, n_slots) -> hit` — one exec
//!   whose result is written as UTF-16 spans into the caller's stack
//!   record instead of a match array; lastIndex effects and the
//!   frozen-lastIndex TypeError are `regex_exec`'s, so the call is
//!   followed by a throw check.
//! - `str_group_span(subject, start, end) -> Str` — the string value
//!   of one span: a fresh Str of the subject's code units, or the Str
//!   `undefined` sentinel for a group that did not participate.

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct ExecRecordIds {
    pub regex_exec_span: FuncId,
    pub str_group_span: FuncId,
}

pub(crate) fn declare(
    module: &mut Module,
    fn_table: &mut HashMap<String, FuncId>,
) -> ExecRecordIds {
    ExecRecordIds {
        regex_exec_span: declare_intrinsic(
            module,
            fn_table,
            "__torajs_regex_exec_span",
            &[Type::RegExp, Type::Str, Type::Ptr, Type::I64],
            Type::I64,
        ),
        str_group_span: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_group_span",
            &[Type::Str, Type::I64, Type::I64],
            Type::Str,
        ),
    }
}
