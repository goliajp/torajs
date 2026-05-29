//! Obj alloc/drop IR builders — split out from `builders.rs` (Phase
//! 2e item 14 / Step 2 of perf/mem/size extremes plan) once
//! `define_obj_drop_sized`'s inline TLAB.push body pushed the
//! parent file past the 500-LOC hard limit.
//!
//! Two LLVM-IR-built intrinsics:
//!
//! - `define_obj_alloc` — inline TLAB.pop fast path (Phase 2e item
//!   13b v2). Returns `slot+16` to match the libc-compat SHIM
//!   layout; falls back to `__torajs_libc_malloc(size)` on TLAB
//!   miss or too-big.
//! - `define_obj_drop_sized` — inline TLAB.push fast path mirror
//!   (Phase 2e item 14). Takes `(user_ptr, size)`; undoes the
//!   `+16` SHIM offset before push; falls back to
//!   `__torajs_libc_free(user_ptr)` on TLAB-full or too-big.

use inkwell::AddressSpace;
use inkwell::IntPredicate;
use inkwell::context::Context;
use inkwell::module::{Linkage, Module as LlvmModule};
use inkwell::values::FunctionValue;

/// `__torajs_obj_alloc(u64 size) -> *void` — plain `malloc(size)`.
///
/// Stays a dumb allocator (no header init): the same intrinsic is
/// reused by ObjectLit lowering AND by escape-captured Copy boxes
/// (8-byte cells) AND by closure env blocks (header layout is
/// fn_addr + drop_fn, not the universal heap header). The lowerer
/// writes the universal refcount header at the call site for actual
/// Obj allocations only.
pub(super) fn define_obj_alloc<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    malloc: FunctionValue<'ctx>,
) -> FunctionValue<'ctx> {
    use torajs_mmalloc::size_class::SIZE_CLASSES;
    use torajs_mmalloc::tlab::{
        TLAB_CLASS_SLOTS_STRIDE, TLAB_DEPTH_OFFSET, TLAB_SLOT_STRIDE, TLAB_TOTAL_SIZE,
    };

    let builder = ctx.create_builder();
    let i8_t = ctx.i8_type();
    let i32_t = ctx.i32_type();
    let i64_t = ctx.i64_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let fn_t = ptr_t.fn_type(&[i64_t.into()], false);
    let f = m.add_function("__torajs_obj_alloc", fn_t, None);
    // Phase 2e item 13b: emit inline TLAB.pop fast path; alwaysinline
    // pushes the body to every user-binary alloc site. Hot loop alloc
    // becomes ~5-10 cyc inline TLS-aware sequence (parity with libc
    // nano allocator's thread-cache pop).
    super::attrs::mark_alwaysinline(ctx, f);

    // Declare or reuse extern plain-static @__torajs_core_tlab.
    // Storage lives in libtorajs_mmalloc.a (`static mut` in .bss);
    // single shared TLAB, no per-thread isolation (multi-thread
    // re-derivation deferred to v0.8 backlog — Step 16-c-2 dropped
    // `#[thread_local]` to eliminate the `__tlv_bootstrap` libc dep).
    let tlab_arr_t = i8_t.array_type(TLAB_TOTAL_SIZE as u32);
    let tlab_global = match m.get_global("__torajs_core_tlab") {
        Some(g) => g,
        None => {
            let g = m.add_global(tlab_arr_t, None, "__torajs_core_tlab");
            g.set_linkage(Linkage::External);
            g
        }
    };

    let entry = ctx.append_basic_block(f, "entry");
    let small_blk = ctx.append_basic_block(f, "small");
    let pop_blk = ctx.append_basic_block(f, "pop");
    let fallback_blk = ctx.append_basic_block(f, "fallback");

    builder.position_at_end(entry);
    let size = f.get_nth_param(0).unwrap().into_int_value();
    // libc-compat shim layout: every chunk has a 16-byte size header
    // **before** the user-visible pointer. `__torajs_libc_malloc(size)`
    // allocates `size + 16` and returns `header_ptr + 16`;
    // `__torajs_libc_free(user_ptr)` reads `*(user_ptr - 16)` to recover
    // the size for free-side dispatch. The inline TLAB.pop fast path
    // must preserve both invariants — bucket against **total** size
    // (size + 16, which determines the class TLAB pushes into on free),
    // and return **slot_ptr + 16** (user pointer above the header).
    let header_off = i64_t.const_int(16, false);
    let total_size = builder
        .build_int_add(size, header_off, "total_size")
        .unwrap();

    // Branch: total > biggest class → fallback (large alloc).
    let max_class = i64_t.const_int(SIZE_CLASSES[SIZE_CLASSES.len() - 1] as u64, false);
    let too_big = builder
        .build_int_compare(IntPredicate::UGT, total_size, max_class, "too_big")
        .unwrap();
    builder
        .build_conditional_branch(too_big, fallback_blk, small_blk)
        .unwrap();

    builder.position_at_end(small_blk);
    // class_idx via reverse-iterated select chain — final value is
    // smallest matching class. Sentinel u32::MAX if nothing matches
    // (shouldn't happen since too_big guarded above, but safe).
    let mut class_idx_val: inkwell::values::IntValue = i32_t.const_int(u32::MAX as u64, false);
    for (i, &sz) in SIZE_CLASSES.iter().enumerate().rev() {
        let cmp = builder
            .build_int_compare(
                IntPredicate::ULE,
                total_size,
                i64_t.const_int(sz as u64, false),
                "le",
            )
            .unwrap();
        let sel = builder
            .build_select(cmp, i32_t.const_int(i as u64, false), class_idx_val, "ci")
            .unwrap();
        class_idx_val = sel.into_int_value();
    }
    let class_idx = class_idx_val;
    let class_idx_i64 = builder
        .build_int_z_extend(class_idx, i64_t, "class_idx_64")
        .unwrap();

    // tlab_ptr = address of plain-static @__torajs_core_tlab. With a
    // plain (non-thread_local) global the static section base IS the
    // single shared TLAB instance, so the constexpr `GEP (i8, @tlab, k)`
    // fold LLVM applies at instcombine is the **correct** lowering —
    // there is no per-thread thunk to defeat. (The 9170c47 SIGBUS was a
    // `#[thread_local]`-only hazard: constexpr fold there bypassed the
    // TLV thunk and resolved to the wrong storage. Dropping
    // `#[thread_local]` in Step 16-c-2 removes both the hazard and the
    // `__tlv_bootstrap` libc dependency.)
    let tlab_ptr = tlab_global.as_pointer_value();
    // depth_ptr = tlab_ptr + TLAB_DEPTH_OFFSET + class_idx
    let depth_off = builder
        .build_int_add(
            i64_t.const_int(TLAB_DEPTH_OFFSET as u64, false),
            class_idx_i64,
            "depth_off",
        )
        .unwrap();
    let depth_ptr = unsafe {
        builder
            .build_in_bounds_gep(i8_t, tlab_ptr, &[depth_off], "depth_ptr")
            .unwrap()
    };
    let depth = builder
        .build_load(i8_t, depth_ptr, "depth")
        .unwrap()
        .into_int_value();
    let is_empty = builder
        .build_int_compare(IntPredicate::EQ, depth, i8_t.const_zero(), "empty")
        .unwrap();
    builder
        .build_conditional_branch(is_empty, fallback_blk, pop_blk)
        .unwrap();

    // pop hot path: new_depth = depth - 1; store; load slot[new_depth]; return
    builder.position_at_end(pop_blk);
    let new_depth = builder
        .build_int_sub(depth, i8_t.const_int(1, false), "new_depth")
        .unwrap();
    builder.build_store(depth_ptr, new_depth).unwrap();
    // slot offset: TLAB_SLOTS_OFFSET (0) + class_idx*TLAB_CLASS_SLOTS_STRIDE
    //              + new_depth*TLAB_SLOT_STRIDE
    let class_off = builder
        .build_int_mul(
            class_idx_i64,
            i64_t.const_int(TLAB_CLASS_SLOTS_STRIDE as u64, false),
            "class_off",
        )
        .unwrap();
    let new_depth_i64 = builder
        .build_int_z_extend(new_depth, i64_t, "new_depth_64")
        .unwrap();
    let slot_off_in_class = builder
        .build_int_mul(
            new_depth_i64,
            i64_t.const_int(TLAB_SLOT_STRIDE as u64, false),
            "slot_off",
        )
        .unwrap();
    let total_off = builder
        .build_int_add(class_off, slot_off_in_class, "total_off")
        .unwrap();
    let slot_ptr = unsafe {
        builder
            .build_in_bounds_gep(i8_t, tlab_ptr, &[total_off], "slot_ptr")
            .unwrap()
    };
    let slot_val = builder
        .build_load(ptr_t, slot_ptr, "slot_val")
        .unwrap()
        .into_pointer_value();
    // Shift past the 16-byte chunk header so the user sees a pointer
    // matching `__torajs_libc_malloc`'s return shape. Without this, free
    // sites read `*(slot_val - 16)` and walk before the chunk's mapped
    // region (most visible on 16K-page-aligned slot addresses, where the
    // pre-header byte falls into an unmapped page → SIGBUS).
    let user_ptr = unsafe {
        builder
            .build_in_bounds_gep(i8_t, slot_val, &[header_off], "user_ptr")
            .unwrap()
    };
    builder.build_return(Some(&user_ptr)).unwrap();

    // Fallback (TLAB miss or too-big) → call __torajs_libc_malloc(size).
    builder.position_at_end(fallback_blk);
    let p = builder
        .build_call(malloc, &[size.into()], "p")
        .unwrap()
        .try_as_basic_value()
        .unwrap_basic()
        .into_pointer_value();
    builder.build_return(Some(&p)).unwrap();

    f
}

/// `__torajs_obj_drop_sized(*void user_ptr, i64 size) -> void` —
/// inline TLAB.push fast path mirroring `define_obj_alloc`'s inline
/// TLAB.pop. Phase 2e item 14 (Step 2 of perf/mem/size extremes plan):
/// alloc/free symmetric, eliminates `bl __torajs_obj_drop` from the
/// hot loop just as 13b v2 eliminated `bl __torajs_obj_alloc`.
///
/// `size` is the same value passed to the matching `__torajs_obj_alloc`
/// callsite (callsite-known: env block = `CLOSURE_CAP_BASE_OFF +
/// N_caps*8`, typed Obj = `OBJ_HEADER_SIZE + N_fields*8`). The
/// inline body buckets on `total_size = size + 16` (matches
/// alloc-side libc-compat SHIM offset), undoes the user-visible
/// `+16` shift before pushing back into the TLAB slot, and falls
/// back to `__torajs_libc_free(user_ptr)` for too-big or TLAB-full
/// cases.
///
/// Refcount-aware drop walk lives at the lowerer site
/// (`emit_drop_value Type::Obj`); this intrinsic is only the final
/// block-release step.
pub(super) fn define_obj_drop_sized<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    free: FunctionValue<'ctx>,
) -> FunctionValue<'ctx> {
    use torajs_mmalloc::size_class::SIZE_CLASSES;
    use torajs_mmalloc::tlab::{
        TLAB_CACHE_DEPTH, TLAB_CLASS_SLOTS_STRIDE, TLAB_DEPTH_OFFSET, TLAB_SLOT_STRIDE,
        TLAB_TOTAL_SIZE,
    };

    let builder = ctx.create_builder();
    let i8_t = ctx.i8_type();
    let i32_t = ctx.i32_type();
    let i64_t = ctx.i64_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let void_t = ctx.void_type();
    let fn_t = void_t.fn_type(&[ptr_t.into(), i64_t.into()], false);
    let f = m.add_function("__torajs_obj_drop_sized", fn_t, None);
    super::attrs::mark_alwaysinline(ctx, f);

    // Reuse extern plain-static @__torajs_core_tlab. If define_obj_alloc
    // ran first the global is already declared; otherwise add it here.
    let tlab_arr_t = i8_t.array_type(TLAB_TOTAL_SIZE as u32);
    let tlab_global = match m.get_global("__torajs_core_tlab") {
        Some(g) => g,
        None => {
            let g = m.add_global(tlab_arr_t, None, "__torajs_core_tlab");
            g.set_linkage(Linkage::External);
            g
        }
    };

    let entry = ctx.append_basic_block(f, "entry");
    let small_blk = ctx.append_basic_block(f, "small");
    let push_blk = ctx.append_basic_block(f, "push");
    let fallback_blk = ctx.append_basic_block(f, "fallback");

    builder.position_at_end(entry);
    let user_ptr = f.get_nth_param(0).unwrap().into_pointer_value();
    let size = f.get_nth_param(1).unwrap().into_int_value();
    // libc-compat shim layout: chunk header = 16B before user_ptr;
    // total chunk = size + 16. Bucket against total_size so the slot
    // returned to the TLAB matches the class alloc would pop it from
    // (see define_obj_alloc for the symmetric calculation).
    let header_off = i64_t.const_int(16, false);
    let total_size = builder
        .build_int_add(size, header_off, "total_size")
        .unwrap();

    // Branch: total > biggest class → fallback (large alloc free).
    let max_class = i64_t.const_int(SIZE_CLASSES[SIZE_CLASSES.len() - 1] as u64, false);
    let too_big = builder
        .build_int_compare(IntPredicate::UGT, total_size, max_class, "too_big")
        .unwrap();
    builder
        .build_conditional_branch(too_big, fallback_blk, small_blk)
        .unwrap();

    builder.position_at_end(small_blk);
    // class_idx via reverse-iterated select chain — identical shape to
    // define_obj_alloc. With const `size` at the callsite this folds
    // away under instcombine to a single i32 constant.
    let mut class_idx_val: inkwell::values::IntValue = i32_t.const_int(u32::MAX as u64, false);
    for (i, &sz) in SIZE_CLASSES.iter().enumerate().rev() {
        let cmp = builder
            .build_int_compare(
                IntPredicate::ULE,
                total_size,
                i64_t.const_int(sz as u64, false),
                "le",
            )
            .unwrap();
        let sel = builder
            .build_select(cmp, i32_t.const_int(i as u64, false), class_idx_val, "ci")
            .unwrap();
        class_idx_val = sel.into_int_value();
    }
    let class_idx = class_idx_val;
    let class_idx_i64 = builder
        .build_int_z_extend(class_idx, i64_t, "class_idx_64")
        .unwrap();

    // tlab_ptr = address of plain-static @__torajs_core_tlab (see the
    // long comment in define_obj_alloc: with a plain global the
    // constexpr GEP fold is the correct lowering, not the 9170c47 TLS
    // hazard).
    let tlab_ptr = tlab_global.as_pointer_value();
    let depth_off = builder
        .build_int_add(
            i64_t.const_int(TLAB_DEPTH_OFFSET as u64, false),
            class_idx_i64,
            "depth_off",
        )
        .unwrap();
    let depth_ptr = unsafe {
        builder
            .build_in_bounds_gep(i8_t, tlab_ptr, &[depth_off], "depth_ptr")
            .unwrap()
    };
    let depth = builder
        .build_load(i8_t, depth_ptr, "depth")
        .unwrap()
        .into_int_value();
    let is_full = builder
        .build_int_compare(
            IntPredicate::UGE,
            depth,
            i8_t.const_int(TLAB_CACHE_DEPTH as u64, false),
            "full",
        )
        .unwrap();
    builder
        .build_conditional_branch(is_full, fallback_blk, push_blk)
        .unwrap();

    // push hot path: header_ptr = user_ptr - 16; store at slot[depth];
    //                depth++; ret void.
    builder.position_at_end(push_blk);
    let neg_header_off = i64_t.const_int((-16i64) as u64, true);
    let header_ptr = unsafe {
        builder
            .build_in_bounds_gep(i8_t, user_ptr, &[neg_header_off], "header_ptr")
            .unwrap()
    };
    // slot offset: TLAB_SLOTS_OFFSET (=0) + class_idx*TLAB_CLASS_SLOTS_STRIDE
    //              + depth*TLAB_SLOT_STRIDE.
    let class_off = builder
        .build_int_mul(
            class_idx_i64,
            i64_t.const_int(TLAB_CLASS_SLOTS_STRIDE as u64, false),
            "class_off",
        )
        .unwrap();
    let depth_i64 = builder
        .build_int_z_extend(depth, i64_t, "depth_64")
        .unwrap();
    let slot_off_in_class = builder
        .build_int_mul(
            depth_i64,
            i64_t.const_int(TLAB_SLOT_STRIDE as u64, false),
            "slot_off",
        )
        .unwrap();
    let total_off = builder
        .build_int_add(class_off, slot_off_in_class, "total_off")
        .unwrap();
    let slot_ptr = unsafe {
        builder
            .build_in_bounds_gep(i8_t, tlab_ptr, &[total_off], "slot_ptr")
            .unwrap()
    };
    builder.build_store(slot_ptr, header_ptr).unwrap();
    let new_depth = builder
        .build_int_add(depth, i8_t.const_int(1, false), "new_depth")
        .unwrap();
    builder.build_store(depth_ptr, new_depth).unwrap();
    builder.build_return(None).unwrap();

    // Fallback (TLAB full or too-big) → call __torajs_libc_free(user_ptr).
    // user_ptr is the libc-compat SHIM-offset pointer (= header_ptr+16);
    // __torajs_libc_free reads `*(user_ptr-16)` to recover the size and
    // dispatch to the correct free path.
    builder.position_at_end(fallback_blk);
    builder.build_call(free, &[user_ptr.into()], "_f").unwrap();
    builder.build_return(None).unwrap();

    f
}
