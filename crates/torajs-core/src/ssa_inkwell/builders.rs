//! Primitive console-output + Obj-alloc/drop IR builders.
//!
//! Five small fns that don't fit the Array<T> / split-iter families:
//!
//! - `define_print_bool` — putchar `"true\n"` / `"false\n"` per JS
//!   `console.log(true|false)`.
//! - `define_print_f64` — tail call to `__torajs_print_f64_js` in
//!   the runtime, which formats per ES (lowercase `nan` becomes
//!   `NaN`, `Infinity`, ...).
//! - `define_print_i64` — divide-by-10 digit extraction + putchar
//!   in reverse. mem2reg lifts the allocas at -O1+.
//! - `define_obj_alloc` — plain `malloc(size)` wrapper. Header is
//!   written by the lowerer at the call site for actual Obj
//!   allocations (box / env cells go through here too with no
//!   header).
//! - `define_obj_drop` — plain `free(p)` wrapper. The
//!   refcount-aware drop walk lives at the lowerer site.
//!
//! Extracted from `ssa_inkwell.rs` god-file decomposition (2026-05-25,
//! batch 6).

use inkwell::AddressSpace;
use inkwell::IntPredicate;
use inkwell::ThreadLocalMode;
use inkwell::context::Context;
use inkwell::module::{Linkage, Module as LlvmModule};
use inkwell::values::FunctionValue;

/// `print_bool(bool) -> void` — putchar's `"true\n"` or `"false\n"`
/// per the bool input. M6.1 console.log dispatch routes Type::Bool
/// args here. (Same shared stdio buffer as print_i64 / str_print —
/// no ordering surprises.)
pub(super) fn define_print_bool<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    putchar: FunctionValue<'ctx>,
) -> FunctionValue<'ctx> {
    let builder = ctx.create_builder();
    let i32_t = ctx.i32_type();
    let bool_t = ctx.bool_type();
    let void_t = ctx.void_type();
    let fn_t = void_t.fn_type(&[bool_t.into()], false);
    let f = m.add_function("print_bool", fn_t, None);
    let entry = ctx.append_basic_block(f, "entry");
    let true_blk = ctx.append_basic_block(f, "tbl");
    let false_blk = ctx.append_basic_block(f, "fbl");
    let nl_blk = ctx.append_basic_block(f, "nl");
    builder.position_at_end(entry);
    let b = f.get_nth_param(0).unwrap().into_int_value();
    builder
        .build_conditional_branch(b, true_blk, false_blk)
        .unwrap();
    let putc = |ch: u8| {
        builder
            .build_call(putchar, &[i32_t.const_int(ch as u64, false).into()], "")
            .unwrap();
    };
    builder.position_at_end(true_blk);
    putc(b't');
    putc(b'r');
    putc(b'u');
    putc(b'e');
    builder.build_unconditional_branch(nl_blk).unwrap();
    builder.position_at_end(false_blk);
    putc(b'f');
    putc(b'a');
    putc(b'l');
    putc(b's');
    putc(b'e');
    builder.build_unconditional_branch(nl_blk).unwrap();
    builder.position_at_end(nl_blk);
    putc(b'\n');
    builder.build_return(None).unwrap();
    f
}

/// `print_f64(f64) -> void` — tail call to `__torajs_print_f64_js`
/// in C runtime, which handles JS-spec NaN / Infinity formatting
/// (was: printf("%g\n", x), which printed lowercase "nan" — a
/// bun-divergence on every test262 NaN case).
pub(super) fn define_print_f64<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
) -> FunctionValue<'ctx> {
    let f64_t = ctx.f64_type();
    let void_t = ctx.void_type();
    let helper_t = void_t.fn_type(&[f64_t.into()], false);
    let helper = m
        .get_function("__torajs_print_f64_js")
        .unwrap_or_else(|| m.add_function("__torajs_print_f64_js", helper_t, None));
    let fn_t = void_t.fn_type(&[f64_t.into()], false);
    let f = m.add_function("print_f64", fn_t, None);
    let entry = ctx.append_basic_block(f, "entry");
    let builder = ctx.create_builder();
    builder.position_at_end(entry);
    let arg = f.get_nth_param(0).unwrap().into_float_value();
    builder.build_call(helper, &[arg.into()], "_p").unwrap();
    builder.build_return(None).unwrap();
    f
}

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

    // Declare or reuse extern thread_local @__torajs_core_tlab.
    // Body lives in libtorajs_mmalloc.a (TPIDR_EL0 register backed).
    let tlab_arr_t = i8_t.array_type(TLAB_TOTAL_SIZE as u32);
    let tlab_global = match m.get_global("__torajs_core_tlab") {
        Some(g) => g,
        None => {
            let g = m.add_global(tlab_arr_t, None, "__torajs_core_tlab");
            g.set_thread_local_mode(Some(ThreadLocalMode::GeneralDynamicTLSModel));
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

    // Branch: size > biggest class → fallback (large alloc).
    let max_class = i64_t.const_int(SIZE_CLASSES[SIZE_CLASSES.len() - 1] as u64, false);
    let too_big = builder
        .build_int_compare(IntPredicate::UGT, size, max_class, "too_big")
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
                size,
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

    // tlab_ptr = thread_local addr of @__torajs_core_tlab
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
    let slot_val = builder.build_load(ptr_t, slot_ptr, "slot_val").unwrap();
    builder.build_return(Some(&slot_val)).unwrap();

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

/// `__torajs_obj_drop(*void p) -> void` — plain `free(p)`. The
/// Obj-specific refcount-aware drop lives at the lowerer site
/// (`emit_drop_value Type::Obj`), which walks fields and emits an
/// inline rc_dec + cond-free for the Obj header. This intrinsic is
/// only called for box / env paths, both of which are single-owner.
/// The inline drop site (ssa_lower's emit_drop_value Type::Obj
/// walk_blk) gates on `is_class_sid` to call
/// `__torajs_cycle_unbuffer` BEFORE reaching here, so this stays a
/// 1-instruction tail call.
pub(super) fn define_obj_drop<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    free: FunctionValue<'ctx>,
) -> FunctionValue<'ctx> {
    let builder = ctx.create_builder();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let void_t = ctx.void_type();
    let fn_t = void_t.fn_type(&[ptr_t.into()], false);
    let f = m.add_function("__torajs_obj_drop", fn_t, None);
    // Phase 2e item 13a: alwaysinline — eliminate wrapper layer.
    super::attrs::mark_alwaysinline(ctx, f);
    let entry = ctx.append_basic_block(f, "entry");
    builder.position_at_end(entry);
    let arg = f.get_nth_param(0).unwrap().into_pointer_value();
    builder.build_call(free, &[arg.into()], "_f").unwrap();
    builder.build_return(None).unwrap();
    f
}

/// Build the body of `print_i64(i64 n)` directly in LLVM IR. Same shape as
/// labs/0002-inkwell-spike's `add_print_i64` — divide-by-10, push digits,
/// putchar them out in reverse, then putchar('\n'). LLVM mem2reg lifts the
/// allocas to SSA values at -O1+.
pub(super) fn define_print_i64<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    putchar: FunctionValue<'ctx>,
) -> FunctionValue<'ctx> {
    let builder = ctx.create_builder();
    let i64_t = ctx.i64_type();
    let i32_t = ctx.i32_type();
    let void_t = ctx.void_type();

    let fn_t = void_t.fn_type(&[i64_t.into()], false);
    let f = m.add_function("print_i64", fn_t, None);
    let entry = ctx.append_basic_block(f, "entry");
    let zero_blk = ctx.append_basic_block(f, "zero");
    let loop1 = ctx.append_basic_block(f, "loop1");
    let dump = ctx.append_basic_block(f, "dump");
    let loop2 = ctx.append_basic_block(f, "loop2");
    let pop = ctx.append_basic_block(f, "pop");
    let done = ctx.append_basic_block(f, "done");

    let neg_blk = ctx.append_basic_block(f, "neg");
    let prep_blk = ctx.append_basic_block(f, "prep");
    builder.position_at_end(entry);
    let buf = builder.build_alloca(i64_t.array_type(20), "buf").unwrap();
    let cnt_a = builder.build_alloca(i64_t, "count").unwrap();
    builder
        .build_store(cnt_a, i64_t.const_int(0, false))
        .unwrap();
    let n_a = builder.build_alloca(i64_t, "n").unwrap();
    let arg = f.get_nth_param(0).unwrap().into_int_value();
    builder.build_store(n_a, arg).unwrap();
    // Special-case `arg == 0`: the digit-extraction loop terminates
    // when `n_cur == 0`, so without this branch a 0 input prints
    // nothing.
    let is_zero = builder
        .build_int_compare(IntPredicate::EQ, arg, i64_t.const_int(0, false), "is_zero")
        .unwrap();
    builder
        .build_conditional_branch(is_zero, zero_blk, prep_blk)
        .unwrap();
    // prep: if n < 0 → emit '-' + negate, then fall through to loop1.
    // Without this branch the digit-extraction loop bailed early on
    // negative inputs (the SGT > 0 check sent them to loop2 with
    // count=0 → just a newline).
    builder.position_at_end(prep_blk);
    let is_neg = builder
        .build_int_compare(IntPredicate::SLT, arg, i64_t.const_int(0, false), "is_neg")
        .unwrap();
    builder
        .build_conditional_branch(is_neg, neg_blk, loop1)
        .unwrap();
    builder.position_at_end(neg_blk);
    let minus_ch = i32_t.const_int(b'-' as u64, false);
    builder
        .build_call(putchar, &[minus_ch.into()], "_minus")
        .unwrap();
    let neg_arg = builder.build_int_neg(arg, "neg_arg").unwrap();
    builder.build_store(n_a, neg_arg).unwrap();
    builder.build_unconditional_branch(loop1).unwrap();

    builder.position_at_end(zero_blk);
    let zero_ch = i32_t.const_int(b'0' as u64, false);
    builder
        .build_call(putchar, &[zero_ch.into()], "_z")
        .unwrap();
    let newline_ch = i32_t.const_int(b'\n' as u64, false);
    builder
        .build_call(putchar, &[newline_ch.into()], "_nl_z")
        .unwrap();
    builder.build_return(None).unwrap();

    builder.position_at_end(loop1);
    let n_cur = builder
        .build_load(i64_t, n_a, "n_cur")
        .unwrap()
        .into_int_value();
    let zero = i64_t.const_int(0, false);
    let pos = builder
        .build_int_compare(IntPredicate::SGT, n_cur, zero, "pos")
        .unwrap();
    builder.build_conditional_branch(pos, dump, loop2).unwrap();

    builder.position_at_end(dump);
    let ten = i64_t.const_int(10, false);
    let digit = builder.build_int_signed_rem(n_cur, ten, "digit").unwrap();
    let ascii = builder
        .build_int_add(digit, i64_t.const_int(b'0' as u64, false), "ascii")
        .unwrap();
    let cnt = builder
        .build_load(i64_t, cnt_a, "cnt")
        .unwrap()
        .into_int_value();
    let slot = unsafe {
        builder
            .build_in_bounds_gep(
                i64_t.array_type(20),
                buf,
                &[i64_t.const_int(0, false), cnt],
                "slot",
            )
            .unwrap()
    };
    builder.build_store(slot, ascii).unwrap();
    let cnt_next = builder
        .build_int_add(cnt, i64_t.const_int(1, false), "cnt_next")
        .unwrap();
    builder.build_store(cnt_a, cnt_next).unwrap();
    let n_next = builder.build_int_signed_div(n_cur, ten, "n_next").unwrap();
    builder.build_store(n_a, n_next).unwrap();
    builder.build_unconditional_branch(loop1).unwrap();

    builder.position_at_end(loop2);
    let cnt2 = builder
        .build_load(i64_t, cnt_a, "cnt2")
        .unwrap()
        .into_int_value();
    let still = builder
        .build_int_compare(IntPredicate::SGT, cnt2, zero, "still")
        .unwrap();
    builder.build_conditional_branch(still, pop, done).unwrap();

    builder.position_at_end(pop);
    let cnt_dec = builder
        .build_int_sub(cnt2, i64_t.const_int(1, false), "cnt_dec")
        .unwrap();
    builder.build_store(cnt_a, cnt_dec).unwrap();
    let pop_slot = unsafe {
        builder
            .build_in_bounds_gep(
                i64_t.array_type(20),
                buf,
                &[i64_t.const_int(0, false), cnt_dec],
                "pop_slot",
            )
            .unwrap()
    };
    let ch = builder
        .build_load(i64_t, pop_slot, "ch")
        .unwrap()
        .into_int_value();
    let ch32 = builder.build_int_truncate(ch, i32_t, "ch32").unwrap();
    builder.build_call(putchar, &[ch32.into()], "_pc").unwrap();
    builder.build_unconditional_branch(loop2).unwrap();

    builder.position_at_end(done);
    let nl = i32_t.const_int(b'\n' as u64, false);
    builder.build_call(putchar, &[nl.into()], "_nl").unwrap();
    builder.build_return(None).unwrap();

    f
}
