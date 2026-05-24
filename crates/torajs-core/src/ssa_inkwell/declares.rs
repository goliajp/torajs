//! libc + Layer-1+ staticlib extern declarations.
//!
//! Every `tr build` user binary calls into libc (malloc / free /
//! memcpy / memcmp / putchar / ...) plus the pool-aware
//! `__torajs_str_alloc_pooled` / `_arr_alloc_pooled` / `_str_free`
//! / `_arr_free` from runtime_str.c. These fns register the LLVM
//! extern signature on the module.
//!
//! Extracted from `ssa_inkwell.rs` god-file decomposition
//! (2026-05-25, batch 2). Pure function declarations + name
//! routing — no IR emission semantics.

use inkwell::AddressSpace;
use inkwell::context::Context;
use inkwell::module::Module as LlvmModule;
use inkwell::values::FunctionValue;

use super::CompileTarget;

/// T-20.b (v0.6.0) — pick the libc fn name based on target. On
/// native, IR calls libc directly with `i64` size args (matches
/// the platform's 64-bit size_t). On wasm32-wasi, libc has 32-bit
/// size_t and wasm makes function-type identity part of the type
/// system; routing through the `__torajs_libc_*` bridge in
/// `runtime_libc_bridge.c` keeps the IR-side i64 ABI uniform
/// while the C bridge does the (size_t)i64 truncation.
pub(super) fn libc_name(native: &'static str, target: CompileTarget) -> &'static str {
    match target {
        CompileTarget::Native => native,
        CompileTarget::Wasm32Wasi => match native {
            "malloc" => "__torajs_libc_malloc",
            "realloc" => "__torajs_libc_realloc",
            "memcpy" => "__torajs_libc_memcpy",
            "memmove" => "__torajs_libc_memmove",
            "memcmp" => "__torajs_libc_memcmp",
            "free" => "__torajs_libc_free",
            _ => panic!("libc_name: no wasm bridge for `{native}`"),
        },
    }
}

pub(super) fn declare_putchar<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
) -> FunctionValue<'ctx> {
    let i32_t = ctx.i32_type();
    let fn_t = i32_t.fn_type(&[i32_t.into()], false);
    m.add_function("putchar", fn_t, None)
}

pub(super) fn declare_malloc<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    target: CompileTarget,
) -> FunctionValue<'ctx> {
    let i64_t = ctx.i64_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let fn_t = ptr_t.fn_type(&[i64_t.into()], false);
    m.add_function(libc_name("malloc", target), fn_t, None)
}

pub(super) fn declare_realloc<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    target: CompileTarget,
) -> FunctionValue<'ctx> {
    let i64_t = ctx.i64_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    // void* realloc(void *p, size_t new_size)
    let fn_t = ptr_t.fn_type(&[ptr_t.into(), i64_t.into()], false);
    m.add_function(libc_name("realloc", target), fn_t, None)
}

pub(super) fn declare_memcpy<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    target: CompileTarget,
) -> FunctionValue<'ctx> {
    let i64_t = ctx.i64_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    // void* memcpy(void *dst, const void *src, size_t n)  — return ignored
    let fn_t = ptr_t.fn_type(&[ptr_t.into(), ptr_t.into(), i64_t.into()], false);
    m.add_function(libc_name("memcpy", target), fn_t, None)
}

pub(super) fn declare_memmove<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    target: CompileTarget,
) -> FunctionValue<'ctx> {
    let i64_t = ctx.i64_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    // void* memmove(void *dst, const void *src, size_t n) — overlap-safe
    let fn_t = ptr_t.fn_type(&[ptr_t.into(), ptr_t.into(), i64_t.into()], false);
    m.add_function(libc_name("memmove", target), fn_t, None)
}

pub(super) fn declare_memcmp<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    target: CompileTarget,
) -> FunctionValue<'ctx> {
    let i32_t = ctx.i32_type();
    let i64_t = ctx.i64_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    // int memcmp(const void *a, const void *b, size_t n)
    let fn_t = i32_t.fn_type(&[ptr_t.into(), ptr_t.into(), i64_t.into()], false);
    m.add_function(libc_name("memcmp", target), fn_t, None)
}

pub(super) fn declare_free<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    target: CompileTarget,
) -> FunctionValue<'ctx> {
    let void_t = ctx.void_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let fn_t = void_t.fn_type(&[ptr_t.into()], false);
    m.add_function(libc_name("free", target), fn_t, None)
}

/// `__torajs_str_free(uint8_t *p)` — pool-aware Str free. Defined in
/// torajs-str. Pushes short-string blocks onto a thread-local LIFO
/// for reuse by the next short-Str alloc; falls back to libc free
/// for blocks too large to pool.
pub(super) fn declare_str_free<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
) -> FunctionValue<'ctx> {
    let void_t = ctx.void_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let fn_t = void_t.fn_type(&[ptr_t.into()], false);
    m.add_function("__torajs_str_free", fn_t, None)
}

/// `__torajs_arr_free(void *p)` — pool-aware arr free. Defined in
/// torajs-arr. Routes split-block allocations (flagged in the
/// universal header) to a thread-local cache indexed by `cap` so
/// tight `s.split(sep)` loops recycle the exact same block every
/// iter instead of mallocing per call.
pub(super) fn declare_arr_free<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
) -> FunctionValue<'ctx> {
    let void_t = ctx.void_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let fn_t = void_t.fn_type(&[ptr_t.into()], false);
    m.add_function("__torajs_arr_free", fn_t, None)
}

/// `__torajs_str_alloc_pooled(uint64_t len) -> uint8_t*` — pool-aware
/// Str alloc. Pops a recently-freed short-Str block when one fits;
/// otherwise calls malloc + initializes the header. Defined in
/// torajs-str. Inkwell's str_alloc IR fn delegates here so the
/// LLVM-emitted hot path picks up the pool too.
pub(super) fn declare_str_alloc_pooled<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
) -> FunctionValue<'ctx> {
    let i64_t = ctx.i64_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let fn_t = ptr_t.fn_type(&[i64_t.into()], false);
    m.add_function("__torajs_str_alloc_pooled", fn_t, None)
}

/// `__torajs_arr_alloc_pooled(uint64_t cap) -> void*` — pool-aware
/// Array<T> alloc. Same shape as `_str_alloc_pooled` but for the
/// short-Array LIFO pool defined in torajs-arr.
pub(super) fn declare_arr_alloc_pooled<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
) -> FunctionValue<'ctx> {
    let i64_t = ctx.i64_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let fn_t = ptr_t.fn_type(&[i64_t.into()], false);
    m.add_function("__torajs_arr_alloc_pooled", fn_t, None)
}
