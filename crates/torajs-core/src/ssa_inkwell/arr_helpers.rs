//! Array<T> heap-layout helpers for inkwell IR builders.
//!
//! These six helpers compute byte-level pointers / values out of the
//! Array<T> heap header (`refcount @ +0`, `type_tag @ +4`, `flags @ +6`,
//! `len @ +8`, `cap @ +16`, `head_offset @ +20`, slots `@ +24`). All
//! the per-IR `define_arr_*` builders consume them.
//!
//! T-13.5: cap (u32) + head_offset (u32) packed into the 8-B slot at
//! offset 16; `arr.shift()` is O(1) (head++ / len--), `arr.push()`
//! compacts when phys_used == cap and head > 0.
//!
//! Extracted from `ssa_inkwell.rs` god-file decomposition (2026-05-25,
//! batch 3).

use inkwell::context::Context;

use super::{ARR_HDR_CAP_OFF, ARR_HDR_DATA_OFF, ARR_HDR_HEAD_OFF, ARR_HDR_LEN_OFF};

/// Get the Arr's logical-data byte pointer: `arr + 24 + head*8`. Used
/// by callers that index by logical-slot (push fast-path, push grow-
/// path store).
pub(super) fn arr_data_ptr<'ctx>(
    ctx: &'ctx Context,
    builder: &inkwell::builder::Builder<'ctx>,
    arr_ptr: inkwell::values::PointerValue<'ctx>,
    name: &str,
) -> inkwell::values::PointerValue<'ctx> {
    let i64_t = ctx.i64_type();
    let i8_t = ctx.i8_type();
    let raw = unsafe {
        builder
            .build_in_bounds_gep(
                i8_t,
                arr_ptr,
                &[i64_t.const_int(ARR_HDR_DATA_OFF, false)],
                &format!("{name}_raw"),
            )
            .unwrap()
    };
    let head_x8 = arr_head_x8_load(ctx, builder, arr_ptr, name);
    unsafe {
        builder
            .build_in_bounds_gep(i8_t, raw, &[head_x8], name)
            .unwrap()
    }
}

/// Get the Arr's raw-data byte pointer (`p + 24`), bypassing the
/// head_offset adjustment. Used by paths that need physical-slot
/// access — currently the in-IR compact memmove. Avoid otherwise.
pub(super) fn arr_raw_data_ptr<'ctx>(
    ctx: &'ctx Context,
    builder: &inkwell::builder::Builder<'ctx>,
    arr_ptr: inkwell::values::PointerValue<'ctx>,
    name: &str,
) -> inkwell::values::PointerValue<'ctx> {
    let i64_t = ctx.i64_type();
    let i8_t = ctx.i8_type();
    unsafe {
        builder
            .build_in_bounds_gep(
                i8_t,
                arr_ptr,
                &[i64_t.const_int(ARR_HDR_DATA_OFF, false)],
                name,
            )
            .unwrap()
    }
}

/// Load the Arr's `head_offset * 8` (i64) — the byte offset of
/// logical[0] within the slot data section. Loads u32 at offset 20,
/// zext to i64, shl 3.
pub(super) fn arr_head_x8_load<'ctx>(
    ctx: &'ctx Context,
    builder: &inkwell::builder::Builder<'ctx>,
    arr_ptr: inkwell::values::PointerValue<'ctx>,
    name: &str,
) -> inkwell::values::IntValue<'ctx> {
    let i64_t = ctx.i64_type();
    let i32_t = ctx.i32_type();
    let i8_t = ctx.i8_type();
    let head_ptr = unsafe {
        builder
            .build_in_bounds_gep(
                i8_t,
                arr_ptr,
                &[i64_t.const_int(ARR_HDR_HEAD_OFF, false)],
                &format!("{name}_hp"),
            )
            .unwrap()
    };
    let head_i32 = builder
        .build_load(i32_t, head_ptr, &format!("{name}_h32"))
        .unwrap()
        .into_int_value();
    let head_i64 = builder
        .build_int_z_extend(head_i32, i64_t, &format!("{name}_h64"))
        .unwrap();
    builder
        .build_left_shift(head_i64, i64_t.const_int(3, false), &format!("{name}_x8"))
        .unwrap()
}

/// Load the Arr's `head_offset` field (i64-extended). Cheaper-named
/// helper for callers that need head as a count, not a byte offset.
pub(super) fn arr_head_load<'ctx>(
    ctx: &'ctx Context,
    builder: &inkwell::builder::Builder<'ctx>,
    arr_ptr: inkwell::values::PointerValue<'ctx>,
    name: &str,
) -> inkwell::values::IntValue<'ctx> {
    let i64_t = ctx.i64_type();
    let i32_t = ctx.i32_type();
    let i8_t = ctx.i8_type();
    let head_ptr = unsafe {
        builder
            .build_in_bounds_gep(
                i8_t,
                arr_ptr,
                &[i64_t.const_int(ARR_HDR_HEAD_OFF, false)],
                &format!("{name}_hp"),
            )
            .unwrap()
    };
    let head_i32 = builder
        .build_load(i32_t, head_ptr, &format!("{name}_h32"))
        .unwrap()
        .into_int_value();
    builder.build_int_z_extend(head_i32, i64_t, name).unwrap()
}

/// Load the Arr's `len` field (`*(u64*)(p + ARR_HDR_LEN_OFF)`).
pub(super) fn arr_len_load<'ctx>(
    ctx: &'ctx Context,
    builder: &inkwell::builder::Builder<'ctx>,
    arr_ptr: inkwell::values::PointerValue<'ctx>,
    name: &str,
) -> inkwell::values::IntValue<'ctx> {
    let i64_t = ctx.i64_type();
    let i8_t = ctx.i8_type();
    let len_ptr = unsafe {
        builder
            .build_in_bounds_gep(
                i8_t,
                arr_ptr,
                &[i64_t.const_int(ARR_HDR_LEN_OFF, false)],
                &format!("{name}_lp"),
            )
            .unwrap()
    };
    builder
        .build_load(i64_t, len_ptr, name)
        .unwrap()
        .into_int_value()
}

/// Load the Arr's `cap` field (`*(u32*)(p + ARR_HDR_CAP_OFF)`)
/// zext to i64. T-13.5: cap shrunk from u64 to u32 to share a
/// 64-bit slot with `head_offset` at offset 20.
pub(super) fn arr_cap_load<'ctx>(
    ctx: &'ctx Context,
    builder: &inkwell::builder::Builder<'ctx>,
    arr_ptr: inkwell::values::PointerValue<'ctx>,
    name: &str,
) -> inkwell::values::IntValue<'ctx> {
    let i64_t = ctx.i64_type();
    let i32_t = ctx.i32_type();
    let i8_t = ctx.i8_type();
    let cap_ptr = unsafe {
        builder
            .build_in_bounds_gep(
                i8_t,
                arr_ptr,
                &[i64_t.const_int(ARR_HDR_CAP_OFF, false)],
                &format!("{name}_cp"),
            )
            .unwrap()
    };
    let cap_i32 = builder
        .build_load(i32_t, cap_ptr, &format!("{name}_c32"))
        .unwrap()
        .into_int_value();
    builder.build_int_z_extend(cap_i32, i64_t, name).unwrap()
}
