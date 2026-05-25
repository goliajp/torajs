//! Array<T> hot-path IR builders — `__torajs_arr_push`,
//! `__torajs_arr_shift`, `__torajs_arr_push_unchecked`.
//!
//! Each fn is emitted with Internal linkage + alwaysinline so
//! user-code's SSA-emitted call sites get the body folded into the
//! caller. The corresponding extern "C" Rust impls in torajs-arr
//! stay as cross-staticlib link-time fallbacks for fs/process/
//! promise/regex callers that can't see this module's Internal
//! defines.
//!
//! B1b (2026-05-24, takagi: "我们不要总是考虑成本去选简化、便宜的路径，
//! 我们要扎实！"): the 6 arr_*_ptr/load helpers + define_arr_push body
//! restored from P4.1-l predecessor (commit 6b90dae5) so user-code's
//! SSA-emitted `call __torajs_arr_push` can be inlined into hot push
//! loops (array-sum-1m baseline 16 ms in rust; P4.1-l staticlib port
//! regressed to 21.5 ms because cross-TU `bl __torajs_arr_push + ret`
//! defeats LLVM's loop-fold). B4-shift / B4-push-unchecked follow the
//! same recipe.
//!
//! Extracted from `ssa_inkwell.rs` god-file decomposition (2026-05-25,
//! batch 5).

use inkwell::AddressSpace;
use inkwell::IntPredicate;
use inkwell::context::Context;
use inkwell::module::Module as LlvmModule;
use inkwell::values::FunctionValue;

use super::arr_helpers::{
    ARR_HDR_CAP_OFF, ARR_HDR_DATA_OFF, ARR_HDR_HEAD_OFF, ARR_HDR_LEN_OFF, arr_cap_load,
    arr_data_ptr, arr_head_load, arr_len_load, arr_raw_data_ptr,
};

/// Build the body of `__torajs_arr_push(arr*, val) -> arr*`. 187-LOC
/// IR restored from commit 6b90dae5 (B1b 2026-05-24). Emitted with
/// Internal linkage + alwaysinline so user-code's push-hot-loops
/// fold the algorithm into the caller — recovers the array-sum-1m
/// 16 ms target lost when P4.1-l moved this to torajs-arr/grow.rs's
/// extern "C" Rust impl (cross-TU `bl ... + ret` regressed by +35%).
///
/// Algorithm:
/// ```text
/// entry:       phys_used = head + len; need_room = phys_used >= cap
///              branch need_room ? need_room_blk : store_blk
/// need_room:   branch head>0 ? compact_blk : after_compact_blk
/// compact:     memmove(raw_data, raw_data + head*8, len*8); head = 0
///              fall-through to after_compact
/// after_compact: full = (len == cap); branch full ? grow_blk : post_compact
/// post_compact: jump store
/// grow:        new_cap = cap == 0 ? 4 : cap*2; arr = realloc(arr, ...)
///              store new_cap_u32 at +16; jump store
/// store:       arr = phi(entry/post_compact: arr_in, grow: arr_grown)
///              data = arr + 24 + head*8 (re-load head since compact reset it)
///              *(data + len*8) = val; *(arr + 8) = len + 1; ret arr
/// ```
pub(super) fn define_arr_push<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    realloc: FunctionValue<'ctx>,
    memmove: FunctionValue<'ctx>,
) -> FunctionValue<'ctx> {
    let builder = ctx.create_builder();
    let i64_t = ctx.i64_type();
    let i32_t = ctx.i32_type();
    let i8_t = ctx.i8_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let fn_t = ptr_t.fn_type(&[ptr_t.into(), i64_t.into()], false);
    let f = m.add_function("__torajs_arr_push", fn_t, None);
    let entry = ctx.append_basic_block(f, "entry");
    let need_room_blk = ctx.append_basic_block(f, "need_room");
    let compact_blk = ctx.append_basic_block(f, "compact");
    let after_compact_blk = ctx.append_basic_block(f, "after_compact");
    let grow_blk = ctx.append_basic_block(f, "grow");
    let store_blk = ctx.append_basic_block(f, "store");
    builder.position_at_end(entry);

    let arr_in = f.get_nth_param(0).unwrap().into_pointer_value();
    let val = f.get_nth_param(1).unwrap().into_int_value();

    let len = arr_len_load(ctx, &builder, arr_in, "len");
    let cap = arr_cap_load(ctx, &builder, arr_in, "cap");
    let head = arr_head_load(ctx, &builder, arr_in, "head");
    let phys_used = builder.build_int_add(head, len, "phys_used").unwrap();
    let need_room = builder
        .build_int_compare(IntPredicate::UGE, phys_used, cap, "need_room")
        .unwrap();
    builder
        .build_conditional_branch(need_room, need_room_blk, store_blk)
        .unwrap();

    // need_room_blk: head>0 → compact, else → grow
    builder.position_at_end(need_room_blk);
    let head_pos = builder
        .build_int_compare(
            IntPredicate::UGT,
            head,
            i64_t.const_int(0, false),
            "head_pos",
        )
        .unwrap();
    builder
        .build_conditional_branch(head_pos, compact_blk, after_compact_blk)
        .unwrap();

    // compact_blk: memmove(data, data + head*8, len*8); head=0
    builder.position_at_end(compact_blk);
    let raw_data = arr_raw_data_ptr(ctx, &builder, arr_in, "raw_data");
    let head_x8 = builder
        .build_int_mul(head, i64_t.const_int(8, false), "head_x8")
        .unwrap();
    let src = unsafe {
        builder
            .build_in_bounds_gep(i8_t, raw_data, &[head_x8], "src")
            .unwrap()
    };
    let len_bytes = builder
        .build_int_mul(len, i64_t.const_int(8, false), "len_bytes")
        .unwrap();
    builder
        .build_call(
            memmove,
            &[raw_data.into(), src.into(), len_bytes.into()],
            "_mm",
        )
        .unwrap();
    let head_p = unsafe {
        builder
            .build_in_bounds_gep(
                i8_t,
                arr_in,
                &[i64_t.const_int(ARR_HDR_HEAD_OFF, false)],
                "head_p",
            )
            .unwrap()
    };
    builder
        .build_store(head_p, i32_t.const_int(0, false))
        .unwrap();
    builder
        .build_unconditional_branch(after_compact_blk)
        .unwrap();

    // after_compact_blk: if len == cap, realloc; else go to store
    builder.position_at_end(after_compact_blk);
    let full = builder
        .build_int_compare(IntPredicate::EQ, len, cap, "full")
        .unwrap();
    let post_compact_blk = ctx.append_basic_block(f, "post_compact");
    builder
        .build_conditional_branch(full, grow_blk, post_compact_blk)
        .unwrap();

    // post_compact_blk: jump to store with arr_in (no realloc happened)
    builder.position_at_end(post_compact_blk);
    builder.build_unconditional_branch(store_blk).unwrap();

    // grow_blk: realloc with new_cap = (cap == 0 ? 4 : cap*2). cap stored as u32.
    builder.position_at_end(grow_blk);
    let cap_zero = builder
        .build_int_compare(IntPredicate::EQ, cap, i64_t.const_int(0, false), "cap_zero")
        .unwrap();
    let cap_x2 = builder
        .build_int_mul(cap, i64_t.const_int(2, false), "cap_x2")
        .unwrap();
    let new_cap = builder
        .build_select(cap_zero, i64_t.const_int(4, false), cap_x2, "new_cap")
        .unwrap()
        .into_int_value();
    let new_cap_bytes = builder
        .build_int_mul(new_cap, i64_t.const_int(8, false), "new_cap_bytes")
        .unwrap();
    let new_total = builder
        .build_int_add(
            new_cap_bytes,
            i64_t.const_int(ARR_HDR_DATA_OFF, false),
            "new_total",
        )
        .unwrap();
    let arr_grown = builder
        .build_call(realloc, &[arr_in.into(), new_total.into()], "arr_grown")
        .unwrap()
        .try_as_basic_value()
        .unwrap_basic()
        .into_pointer_value();
    // 4-byte store at offset 16 (cap u32) — must NOT overwrite head at offset 20
    let new_cap_p = unsafe {
        builder
            .build_in_bounds_gep(
                i8_t,
                arr_grown,
                &[i64_t.const_int(ARR_HDR_CAP_OFF, false)],
                "new_cap_p",
            )
            .unwrap()
    };
    let new_cap_i32 = builder
        .build_int_truncate(new_cap, i32_t, "new_cap_i32")
        .unwrap();
    builder.build_store(new_cap_p, new_cap_i32).unwrap();
    builder.build_unconditional_branch(store_blk).unwrap();

    // store_blk: phi arr (entry/post_compact → arr_in, grow → arr_grown).
    // Then write val at logical[len] via head-aware data ptr.
    builder.position_at_end(store_blk);
    let phi = builder.build_phi(ptr_t, "arr").unwrap();
    phi.add_incoming(&[
        (&arr_in, entry),
        (&arr_in, post_compact_blk),
        (&arr_grown, grow_blk),
    ]);
    let arr = phi.as_basic_value().into_pointer_value();
    let data = arr_data_ptr(ctx, &builder, arr, "data");
    let len_x8 = builder
        .build_int_mul(len, i64_t.const_int(8, false), "len_x8")
        .unwrap();
    let slot = unsafe {
        builder
            .build_in_bounds_gep(i8_t, data, &[len_x8], "slot")
            .unwrap()
    };
    builder.build_store(slot, val).unwrap();
    let len_p1 = builder
        .build_int_add(len, i64_t.const_int(1, false), "len_p1")
        .unwrap();
    let len_p = unsafe {
        builder
            .build_in_bounds_gep(
                i8_t,
                arr,
                &[i64_t.const_int(ARR_HDR_LEN_OFF, false)],
                "len_p",
            )
            .unwrap()
    };
    builder.build_store(len_p, len_p1).unwrap();
    builder.build_return(Some(&arr)).unwrap();
    f
}

/// B4-shift (2026-05-25, follow-on to B1b): restored from commit
/// 6b90dae5's pre-P4.1-m IR builder. Same rationale as B1b: P4.1-m
/// moved this to torajs-arr/grow.rs's extern "C" Rust impl, which
/// can't be inlined into the caller's fifo-queue hot loop (`q.shift()`
/// inside a `for` over 100k iters). The Rust comment at grow.rs:155
/// admits "fat-LTO at `tr build` time can still inline" — but in
/// practice fat-LTO doesn't recover the 4-memory-op alwaysinline
/// behavior because the extern "C" function still has prologue/
/// epilogue cost vs a pure 4-instruction inline.
///
/// Linkage = Internal + alwaysinline so user-code's `call
/// __torajs_arr_shift` folds the 4 ops in directly. The
/// torajs-arr/grow.rs extern stays for any cross-staticlib caller
/// (none in current tree, but kept as link-time fallback).
///
/// Algorithm (4 memory ops, branchless):
/// ```text
///   head = *(u32*)(arr + 20)
///   v    = *(i64*)(arr + 24 + head*8)   // logical[0]
///   *(u32*)(arr + 20) = head + 1        // bump head_offset
///   *(u64*)(arr + 8)  -= 1              // dec len
///   return v
/// ```
pub(super) fn define_arr_shift<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
) -> FunctionValue<'ctx> {
    let builder = ctx.create_builder();
    let i64_t = ctx.i64_type();
    let i32_t = ctx.i32_type();
    let i8_t = ctx.i8_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let fn_t = i64_t.fn_type(&[ptr_t.into()], false);
    let f = m.add_function("__torajs_arr_shift", fn_t, None);
    let entry = ctx.append_basic_block(f, "entry");
    builder.position_at_end(entry);
    let arr = f.get_nth_param(0).unwrap().into_pointer_value();

    // Load value at logical[0] = data + head*8 (data starts at +24).
    let head_p = unsafe {
        builder
            .build_in_bounds_gep(
                i8_t,
                arr,
                &[i64_t.const_int(ARR_HDR_HEAD_OFF, false)],
                "head_p",
            )
            .unwrap()
    };
    let head32 = builder
        .build_load(i32_t, head_p, "head32")
        .unwrap()
        .into_int_value();
    let head64 = builder.build_int_z_extend(head32, i64_t, "head64").unwrap();
    let head_x8 = builder
        .build_int_mul(head64, i64_t.const_int(8, false), "head_x8")
        .unwrap();
    let data = unsafe {
        builder
            .build_in_bounds_gep(
                i8_t,
                arr,
                &[i64_t.const_int(ARR_HDR_DATA_OFF, false)],
                "data",
            )
            .unwrap()
    };
    let slot = unsafe {
        builder
            .build_in_bounds_gep(i8_t, data, &[head_x8], "slot")
            .unwrap()
    };
    let v = builder
        .build_load(i64_t, slot, "v")
        .unwrap()
        .into_int_value();

    // head_offset += 1 (u32)
    let head_inc = builder
        .build_int_add(head32, i32_t.const_int(1, false), "head_inc")
        .unwrap();
    builder.build_store(head_p, head_inc).unwrap();

    // len -= 1 (u64)
    let len_p = unsafe {
        builder
            .build_in_bounds_gep(
                i8_t,
                arr,
                &[i64_t.const_int(ARR_HDR_LEN_OFF, false)],
                "len_p",
            )
            .unwrap()
    };
    let len = builder
        .build_load(i64_t, len_p, "len")
        .unwrap()
        .into_int_value();
    let len_dec = builder
        .build_int_sub(len, i64_t.const_int(1, false), "len_dec")
        .unwrap();
    builder.build_store(len_p, len_dec).unwrap();

    builder.build_return(Some(&v)).unwrap();
    f
}

/// B4-push-unchecked (2026-05-25, follow-on to B1b / B4-shift):
/// restored 5-instr M6.2 fast-path from commit a390337^ (pre-P4.1-c).
/// Used by array-literal materializers — `[1, 2, 3, ...]` compiles to
/// `arr = arr_alloc(N); arr_push_unchecked(arr, 1); arr_push_unchecked(arr, 2); ...`
/// where the caller has already guaranteed `cap >= N` so the per-push
/// cap check is gone. The 5 ops are: load len, gep data+head*8,
/// gep slot=data+len*8, store val, store len+1.
///
/// At extern-C in torajs-arr/ops.rs, every array-literal element
/// becomes a `bl __torajs_arr_push_unchecked + ret`. With Internal +
/// alwaysinline back, LLVM folds the body in-place; for a `[1..1000]`
/// literal that's 1000× call overhead removed.
pub(super) fn define_arr_push_unchecked<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
) -> FunctionValue<'ctx> {
    let builder = ctx.create_builder();
    let i64_t = ctx.i64_type();
    let i8_t = ctx.i8_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let void_t = ctx.void_type();
    let fn_t = void_t.fn_type(&[ptr_t.into(), i64_t.into()], false);
    let f = m.add_function("__torajs_arr_push_unchecked", fn_t, None);
    let entry = ctx.append_basic_block(f, "entry");
    builder.position_at_end(entry);
    let arr = f.get_nth_param(0).unwrap().into_pointer_value();
    let val = f.get_nth_param(1).unwrap().into_int_value();
    let len = arr_len_load(ctx, &builder, arr, "len");
    let data = arr_data_ptr(ctx, &builder, arr, "data");
    let len_x8 = builder
        .build_int_mul(len, i64_t.const_int(8, false), "len_x8")
        .unwrap();
    let slot = unsafe {
        builder
            .build_in_bounds_gep(i8_t, data, &[len_x8], "slot")
            .unwrap()
    };
    builder.build_store(slot, val).unwrap();
    let len_p1 = builder
        .build_int_add(len, i64_t.const_int(1, false), "len_p1")
        .unwrap();
    let len_p = unsafe {
        builder
            .build_in_bounds_gep(
                i8_t,
                arr,
                &[i64_t.const_int(ARR_HDR_LEN_OFF, false)],
                "len_p",
            )
            .unwrap()
    };
    builder.build_store(len_p, len_p1).unwrap();
    builder.build_return(None).unwrap();
    f
}
