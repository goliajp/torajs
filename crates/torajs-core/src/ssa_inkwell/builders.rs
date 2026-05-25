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
use inkwell::context::Context;
use inkwell::module::Module as LlvmModule;
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
    let builder = ctx.create_builder();
    let i64_t = ctx.i64_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let fn_t = ptr_t.fn_type(&[i64_t.into()], false);
    let f = m.add_function("__torajs_obj_alloc", fn_t, None);
    let entry = ctx.append_basic_block(f, "entry");
    builder.position_at_end(entry);
    let size = f.get_nth_param(0).unwrap();
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
