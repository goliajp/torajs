//! FuncId metadata lookup helpers for `LowerCtx<'a>` extracted from
//! `ssa_lower.rs` chunk 399 — Path A.3-batch20.
//!
//! Three tiny helpers reading precomputed `LowerCtx` maps (`fn_table`,
//! `signatures`, `intrinsics`) to answer "what does this FuncId
//! resolve to?" questions from various emit sites:
//!
//! - `f_name_of(fid) -> String` — reverse lookup FuncId -> name via
//!   the lowerer's fn_table. Linear in table size; called by
//!   `emit_throw_check` to consult the may_throw set (also
//!   name-keyed).
//! - `is_intrinsic(fid) -> bool` — flat OR-chain against every
//!   runtime intrinsic FuncId. None of the runtime intrinsics throw,
//!   so M4's call-site throw-check can skip the cond_br after their
//!   calls (saves one runtime fn call per intrinsic invocation on
//!   the hot path).
//! - `f_ret_type_hint(fid) -> Type` — look up callee's return type
//!   from the signatures map populated in pass 1 of `lower`. Defaults
//!   to I64 for unknown FuncIds (intrinsics or forward refs we
//!   haven't catalogued yet — print_i64 returns void and is called
//!   via `append_void`, so its callsites never reach here).

use crate::ssa::{FuncId, Type};
use crate::ssa_lower::LowerCtx;

impl<'a> LowerCtx<'a> {
    /// Reverse lookup `FuncId → name` via the lowerer's fn_table. Linear
    /// in the table size; used by `emit_throw_check` to consult the
    /// may_throw set (also keyed by name). Module fn count stays in the
    /// double-digits for our cases, so the linear scan is in the noise.
    pub(crate) fn f_name_of(&self, fid: FuncId) -> String {
        self.fn_table
            .iter()
            .find(|(_, v)| **v == fid)
            .map(|(k, _)| k.clone())
            .unwrap_or_default()
    }

    /// True if `fid` is one of the runtime intrinsics declared at the top
    /// of `lower()`. None of these throw, so M4's call-site throw-check
    /// can skip the cond_br after their calls (saves a runtime fn call
    /// per intrinsic invocation in the hot path).
    pub(crate) fn is_intrinsic(&self, fid: FuncId) -> bool {
        let i = &self.intrinsics;
        fid == i.print_i64
            || fid == i.print_f64
            || fid == i.print_bool
            || fid == i.io_sink_to_stderr
            || fid == i.io_sink_to_stdout
            || fid == i.str_alloc
            || fid == i.str_print
            || fid == i.str_drop
            || fid == i.str_concat
            || fid == i.obj_alloc
            || fid == i.obj_drop_sized
            || fid == i.arr_alloc
            || fid == i.arr_push
            || fid == i.arr_shift
            || fid == i.arr_unshift
            || fid == i.arr_splice
            || fid == i.arr_drop
            || fid == i.arr_reserve
            || fid == i.arr_push_unchecked
            || fid == i.str_slice
            || fid == i.str_char_code_at
            || fid == i.str_code_point_at
            || fid == i.str_starts_with
            || fid == i.str_ends_with
            || fid == i.str_index_of
            || fid == i.str_last_index_of
            || fid == i.str_locale_compare
            || fid == i.str_includes
            || fid == i.str_eq
            || fid == i.str_split
            || fid == i.substr_create
            || fid == i.substr_drop
            || fid == i.substr_char_code_at
            || fid == i.substr_code_point_at
            || fid == i.substr_eq_str
            || fid == i.substr_to_owned
            || fid == i.substr_starts_with
            || fid == i.substr_ends_with
            || fid == i.substr_includes
            || fid == i.substr_index_of
            || fid == i.substr_slice
            || fid == i.substr_substring
            || fid == i.substr_trim
            || fid == i.substr_trim_into
            || fid == i.substr_trim_start
            || fid == i.substr_trim_end
            || fid == i.substr_concat_substr_str
            || fid == i.substr_concat_str_substr
            || fid == i.substr_concat_substr_substr
            || fid == i.arr_from_string
            || fid == i.str_substring
            || fid == i.arr_to_reversed
            || fid == i.arr_with
            || fid == i.arr_join
            || fid == i.arr_join_substr
            || fid == i.math_sqrt
            || fid == i.math_abs
            || fid == i.math_floor
            || fid == i.math_ceil
            || fid == i.math_log
            || fid == i.math_exp
            || fid == i.math_pow
            || fid == i.math_min
            || fid == i.math_max
            || fid == i.math_sign
            || fid == i.math_round
            || fid == i.math_trunc
            || fid == i.math_sin
            || fid == i.math_cos
            || fid == i.math_tan
            || fid == i.math_asin
            || fid == i.math_acos
            || fid == i.math_atan
            || fid == i.math_atan2
            || fid == i.math_log2
            || fid == i.math_log10
            || fid == i.math_cbrt
            || fid == i.math_sinh
            || fid == i.math_cosh
            || fid == i.math_tanh
            || fid == i.math_asinh
            || fid == i.math_acosh
            || fid == i.math_atanh
            || fid == i.math_expm1
            || fid == i.math_log1p
            || fid == i.math_imul
            || fid == i.math_clz32
            || fid == i.math_fround
            || fid == i.math_f16round
            || fid == i.math_random
            || fid == i.json_quote_str
            || fid == i.str_repeat
            || fid == i.str_to_upper
            || fid == i.str_to_lower
            || fid == i.str_trim
            || fid == i.str_trim_start
            || fid == i.str_trim_end
            || fid == i.str_pad_start
            || fid == i.str_pad_end
            || fid == i.str_from_char_code
            || fid == i.str_at
            || fid == i.str_replace
            || fid == i.str_replace_all
            || fid == i.num_to_fixed_f
            || fid == i.num_to_fixed_i
            || fid == i.num_to_string_radix_i
            || fid == i.num_to_string_radix_f
            || fid == i.num_to_exp_f
            || fid == i.num_to_exp_i
            || fid == i.num_to_precision_f
            || fid == i.num_to_precision_i
            || fid == i.arr_flat
            || fid == i.arr_flat_any
            || fid == i.arr_extend_typed_into_any
            || fid == i.arr_any_slice
            || fid == i.arr_concat
            || fid == i.arr_reverse
            || fid == i.arr_fill
            || fid == i.arr_copy_within
            || fid == i.throw_set
            || fid == i.throw_check
            || fid == i.throw_take
    }

    /// Look up the callee's return type from the signatures map populated
    /// in pass 1 of `lower`. Defaults to I64 for unknown FuncIds
    /// (intrinsics or forward refs we haven't catalogued yet —
    /// print_i64 returns void and is called via `append_void`, so its
    /// callsites never reach here).
    pub(crate) fn f_ret_type_hint(&self, fid: FuncId) -> Type {
        self.signatures.get(&fid).copied().unwrap_or(Type::I64)
    }
}
