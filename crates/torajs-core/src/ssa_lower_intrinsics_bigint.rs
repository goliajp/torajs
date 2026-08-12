//! Pass 0 `declare_intrinsic` group: BigInt runtime (T-25 + V3-03).
//!
//! chunk 124 continuation of the Pass 0 multi-sub-sibling split
//! (chunks 110-123). 23 declarations covering the entire bigint
//! runtime surface — literal-from-string allocators (T-25; only
//! allocator wired from ssa_lower today, arithmetic intrinsics
//! dispatch from BinOp lowering for `Type::BigInt` operands), the
//! callable ctor's three runtime paths (V3-03), arithmetic / bitwise
//! / shift / pow / mod / cmp / to_string / asIntN / asUintN /
//! lifecycle.
//!
//! - Literal parsers: `bigint_from_decimal(s, sign)`,
//!   `bigint_from_hex(s, sign)`.
//! - `BigInt(value)` ctor (V3-03): `bigint_from_str`,
//!   `bigint_from_number`, `bigint_from_decimal` reused.
//! - Arithmetic: add / sub / mul / div / mod / pow / neg.
//! - Bitwise: and / or / xor / not / shl / shr.
//! - Compare: `bigint_cmp` returns i64 (-1/0/1) for ICmp lowering.
//! - Stringify: `bigint_to_string`, `bigint_to_string_radix(b, r)`.
//! - Convert: `bigint_as_int_n(bits, b)`, `bigint_as_uint_n(bits, b)`.
//! - Lifecycle: `bigint_clone`, `bigint_drop_rc`.

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct BigIntIds {
    pub bigint_from_decimal: FuncId,
    pub bigint_from_hex: FuncId,
    pub bigint_add: FuncId,
    pub bigint_sub: FuncId,
    pub bigint_mul: FuncId,
    pub bigint_div: FuncId,
    pub bigint_mod: FuncId,
    pub bigint_pow: FuncId,
    pub bigint_and: FuncId,
    pub bigint_or: FuncId,
    pub bigint_xor: FuncId,
    pub bigint_not: FuncId,
    pub bigint_shl: FuncId,
    pub bigint_shr: FuncId,
    pub bigint_from_str: FuncId,
    pub bigint_from_number: FuncId,
    /// §21.1.1.1 step 3 — `Number(bigint)` = 𝔽(ℝ(value)).
    pub bigint_to_number: FuncId,
    /// The `Number(any)` pre-gate kernel (BigInt window + generic
    /// ToNumber delegate; torajs-anyvalue `number_ctor.rs`).
    pub number_ctor_any: FuncId,
    /// §13.5.5 any-tier unary minus (BigInt leg + Number-lane 0-x;
    /// torajs-anyvalue `nanbox_encode/pair.rs`).
    pub any_unary_neg: FuncId,
    pub bigint_clone: FuncId,
    pub bigint_neg: FuncId,
    pub bigint_cmp: FuncId,
    pub bigint_is_nonzero: FuncId,
    pub bigint_to_string: FuncId,
    pub bigint_to_string_radix: FuncId,
    pub bigint_to_locale_string: FuncId,
    pub bigint_as_int_n: FuncId,
    pub bigint_as_uint_n: FuncId,
    pub bigint_drop_rc: FuncId,
    /// RFC 20260716 刀 2 — Number wrapper alloc (torajs-wrapper).
    /// Same batch as BigInt because it's the primitive-adjacent
    /// substrate; own sub-group unnecessary for a single fn.
    pub number_wrapper_new: FuncId,
    /// RFC 20260716 刀 2b — String wrapper alloc (transfer-ownership
    /// of the inner Str cell).
    pub string_wrapper_new: FuncId,
    /// RFC 20260716 刀 2c — Boolean wrapper alloc.
    pub boolean_wrapper_new: FuncId,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> BigIntIds {
    let bb = &[Type::BigInt, Type::BigInt][..];
    BigIntIds {
        bigint_from_decimal: declare_intrinsic(
            module,
            fn_table,
            "__torajs_bigint_from_decimal",
            &[Type::Str, Type::I64],
            Type::BigInt,
        ),
        bigint_from_hex: declare_intrinsic(
            module,
            fn_table,
            "__torajs_bigint_from_hex",
            &[Type::Str, Type::I64],
            Type::BigInt,
        ),
        bigint_add: declare_intrinsic(module, fn_table, "__torajs_bigint_add", bb, Type::BigInt),
        bigint_sub: declare_intrinsic(module, fn_table, "__torajs_bigint_sub", bb, Type::BigInt),
        bigint_mul: declare_intrinsic(module, fn_table, "__torajs_bigint_mul", bb, Type::BigInt),
        bigint_div: declare_intrinsic(module, fn_table, "__torajs_bigint_div", bb, Type::BigInt),
        bigint_mod: declare_intrinsic(module, fn_table, "__torajs_bigint_mod", bb, Type::BigInt),
        bigint_pow: declare_intrinsic(module, fn_table, "__torajs_bigint_pow", bb, Type::BigInt),
        bigint_and: declare_intrinsic(module, fn_table, "__torajs_bigint_and", bb, Type::BigInt),
        bigint_or: declare_intrinsic(module, fn_table, "__torajs_bigint_or", bb, Type::BigInt),
        bigint_xor: declare_intrinsic(module, fn_table, "__torajs_bigint_xor", bb, Type::BigInt),
        bigint_not: declare_intrinsic(
            module,
            fn_table,
            "__torajs_bigint_not",
            &[Type::BigInt],
            Type::BigInt,
        ),
        bigint_shl: declare_intrinsic(module, fn_table, "__torajs_bigint_shl", bb, Type::BigInt),
        bigint_shr: declare_intrinsic(module, fn_table, "__torajs_bigint_shr", bb, Type::BigInt),
        bigint_from_str: declare_intrinsic(
            module,
            fn_table,
            "__torajs_bigint_from_str",
            &[Type::Str],
            Type::BigInt,
        ),
        bigint_from_number: declare_intrinsic(
            module,
            fn_table,
            "__torajs_bigint_from_number",
            &[Type::F64],
            Type::BigInt,
        ),
        bigint_to_number: declare_intrinsic(
            module,
            fn_table,
            "__torajs_bigint_to_number",
            &[Type::BigInt],
            Type::F64,
        ),
        number_ctor_any: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_number_ctor",
            &[Type::Any],
            Type::F64,
        ),
        any_unary_neg: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_unary_neg_pair",
            &[Type::I64, Type::I64],
            Type::Any,
        ),
        bigint_clone: declare_intrinsic(
            module,
            fn_table,
            "__torajs_bigint_clone",
            &[Type::BigInt],
            Type::BigInt,
        ),
        bigint_neg: declare_intrinsic(
            module,
            fn_table,
            "__torajs_bigint_neg",
            &[Type::BigInt],
            Type::BigInt,
        ),
        bigint_cmp: declare_intrinsic(module, fn_table, "__torajs_bigint_cmp", bb, Type::I64),
        bigint_is_nonzero: declare_intrinsic(
            module,
            fn_table,
            "__torajs_bigint_is_nonzero",
            &[Type::BigInt],
            Type::I64,
        ),
        bigint_to_string: declare_intrinsic(
            module,
            fn_table,
            "__torajs_bigint_to_string",
            &[Type::BigInt],
            Type::Str,
        ),
        bigint_to_string_radix: declare_intrinsic(
            module,
            fn_table,
            "__torajs_bigint_to_string_radix",
            &[Type::BigInt, Type::I64],
            Type::Str,
        ),
        bigint_to_locale_string: declare_intrinsic(
            module,
            fn_table,
            "__torajs_bigint_to_locale_string",
            &[Type::BigInt],
            Type::Str,
        ),
        bigint_as_int_n: declare_intrinsic(
            module,
            fn_table,
            "__torajs_bigint_as_int_n",
            &[Type::I64, Type::BigInt],
            Type::BigInt,
        ),
        bigint_as_uint_n: declare_intrinsic(
            module,
            fn_table,
            "__torajs_bigint_as_uint_n",
            &[Type::I64, Type::BigInt],
            Type::BigInt,
        ),
        bigint_drop_rc: declare_intrinsic(
            module,
            fn_table,
            "__torajs_bigint_drop_rc",
            &[Type::BigInt],
            Type::Void,
        ),
        // RFC 20260716 刀 2 — `__torajs_number_wrapper_new(f64) -> ptr`.
        // ssa_lower emits the coerce-to-f64 op then calls this; the
        // return type is Ptr (Any-boxable) rather than a dedicated SSA
        // Wrapper variant (the checker keeps it as Type::Any for now;
        // later blades promote to Type::NumberWrapper for member-ladder
        // + auto-unbox).
        number_wrapper_new: declare_intrinsic(
            module,
            fn_table,
            "__torajs_number_wrapper_new",
            &[Type::F64],
            Type::Ptr,
        ),
        // RFC 20260716 刀 2b — `__torajs_string_wrapper_new(*mut u8) -> ptr`.
        // Transfer-ownership: the wrapper adopts the caller's owned
        // Str cell reference and releases it on drop; no post-call
        // drop needed at the emit site.
        string_wrapper_new: declare_intrinsic(
            module,
            fn_table,
            "__torajs_string_wrapper_new",
            &[Type::Str],
            Type::Ptr,
        ),
        // RFC 20260716 刀 2c — `__torajs_boolean_wrapper_new(u8) -> ptr`.
        // Leaf substrate; caller `coerce_to_bool`s the arg and hands
        // over a Bool value (SSA Type::Bool lowers to i8 at the ABI).
        boolean_wrapper_new: declare_intrinsic(
            module,
            fn_table,
            "__torajs_boolean_wrapper_new",
            &[Type::Bool],
            Type::Ptr,
        ),
    }
}
