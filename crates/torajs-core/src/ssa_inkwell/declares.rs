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
use super::attrs::{mark_extern_canonical, mark_nofree};

// Step 12-a-1 (Path B) — LLVM `memory(...)` encoding cheatsheet
// (verified against LLVM 22 IR printer by attrs.rs cfg(test)
// tests; see § "Extern attribute upgrade map" in
// docs/v0.7-A2-finding.md for the per-fn rationale):
//
//   3  = memory(argmem: readwrite)
//   1  = memory(argmem: read)
//   12 = memory(inaccessiblemem: readwrite)
//   15 = memory(argmem: readwrite, inaccessiblemem: readwrite)
//
// Rationale: every libc + pool-aware fn is `nounwind` +
// `willreturn` + `mustprogress` (FFI boundary, finite work,
// always returns) — bundled via `mark_extern_canonical`. The
// memory-effect mask additionally tells LLVM LICM / GVN that
// these calls never touch globals or non-arg memory, unblocking
// hoist of `@i / @sum / @j`-shape loop-invariant loads across
// push-heavy hot paths (Step 12-a target: array-sum-1m vs rust
// 1.53× → ≤ 1.40×).
const MEM_ARGMEM_RW: u32 = 3;
const MEM_ARGMEM_R: u32 = 1;
const MEM_INACCESSIBLE_RW: u32 = 12;
const MEM_ARGMEM_INACCESSIBLE_RW: u32 = 15;

/// Pick the mmalloc / libc-bridge fn name for IR-emitted alloc/copy
/// calls. v0.7-A2 step 6b: Native now also routes through
/// `__torajs_libc_*` — these symbols resolve to torajs-mmalloc's
/// libc-compat shim (libtorajs_mmalloc.a), so user-binary IR alloc
/// + sub-crate Rust extern alloc share a single allocator instance
/// instead of splitting libc vs mmalloc and tripping the SHIM_HEADER
/// invariant. Wasm32-wasi already used the same bridge for the
/// i64↔size_t glue; native joins it under the same names.
pub(super) fn libc_name(native: &'static str, _target: CompileTarget) -> &'static str {
    match native {
        "malloc" => "__torajs_libc_malloc",
        "realloc" => "__torajs_libc_realloc",
        "memcpy" => "__torajs_libc_memcpy",
        "memmove" => "__torajs_libc_memmove",
        "memcmp" => "__torajs_libc_memcmp",
        "free" => "__torajs_libc_free",
        _ => panic!("libc_name: no torajs-mmalloc shim for `{native}`"),
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
    let f = m.add_function(libc_name("malloc", target), fn_t, None);
    // malloc only touches the allocator's internal book-keeping
    // (inaccessible to the caller) — no caller-visible argmem,
    // since the size arg is a scalar.
    mark_extern_canonical(ctx, f, MEM_INACCESSIBLE_RW);
    f
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
    let f = m.add_function(libc_name("realloc", target), fn_t, None);
    // realloc reads/writes the old block (argmem) and updates
    // allocator metadata (inaccessiblemem).
    mark_extern_canonical(ctx, f, MEM_ARGMEM_INACCESSIBLE_RW);
    f
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
    let f = m.add_function(libc_name("memcpy", target), fn_t, None);
    // memcpy reads src + writes dst — both argmem; never touches
    // globals or allocator state.
    mark_extern_canonical(ctx, f, MEM_ARGMEM_RW);
    f
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
    let f = m.add_function(libc_name("memmove", target), fn_t, None);
    mark_extern_canonical(ctx, f, MEM_ARGMEM_RW);
    f
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
    let f = m.add_function(libc_name("memcmp", target), fn_t, None);
    // memcmp reads both ptrs (argmem read-only) and never frees
    // anything; the `nofree` flag is safe here in a way it isn't
    // for free/realloc/pool-aware free fns below.
    mark_extern_canonical(ctx, f, MEM_ARGMEM_R);
    mark_nofree(ctx, f);
    f
}

pub(super) fn declare_free<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    target: CompileTarget,
) -> FunctionValue<'ctx> {
    let void_t = ctx.void_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let fn_t = void_t.fn_type(&[ptr_t.into()], false);
    let f = m.add_function(libc_name("free", target), fn_t, None);
    // free writes the freed block's metadata (argmem) and updates
    // allocator book-keeping (inaccessiblemem).
    mark_extern_canonical(ctx, f, MEM_ARGMEM_INACCESSIBLE_RW);
    f
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
    let f = m.add_function("__torajs_str_free", fn_t, None);
    // Same memory profile as libc free — touches the freed block
    // (argmem) plus the per-thread Str pool LIFO (inaccessiblemem).
    mark_extern_canonical(ctx, f, MEM_ARGMEM_INACCESSIBLE_RW);
    f
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
    let f = m.add_function("__torajs_arr_free", fn_t, None);
    mark_extern_canonical(ctx, f, MEM_ARGMEM_INACCESSIBLE_RW);
    f
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
    let f = m.add_function("__torajs_str_alloc_pooled", fn_t, None);
    // Pool path may touch the recycled block before returning it
    // (argmem write to initialize header). Fallback to malloc
    // touches allocator state (inaccessiblemem).
    mark_extern_canonical(ctx, f, MEM_ARGMEM_INACCESSIBLE_RW);
    f
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
    let f = m.add_function("__torajs_arr_alloc_pooled", fn_t, None);
    mark_extern_canonical(ctx, f, MEM_ARGMEM_INACCESSIBLE_RW);
    f
}

/// Apply the same canonical attribute bundle as `declare_arr_alloc_pooled`
/// to any `FunctionValue` that has already been declared elsewhere (e.g.
/// via the `declare_ssa_fn` fall-through path in `ssa_inkwell.rs`).
/// Used for `__torajs_arr_alloc`, which ssa_lower emits as a direct
/// extern reference without going through `declare_*` here. The Rust
/// impl in `torajs-arr/alloc.rs` is a trivial wrapper around
/// `__torajs_arr_alloc_pooled`, so the memory profile is identical
/// (touches the freshly-allocated block + allocator state).
pub(super) fn apply_arr_alloc_extern_attrs<'ctx>(ctx: &'ctx Context, f: FunctionValue<'ctx>) {
    mark_extern_canonical(ctx, f, MEM_ARGMEM_INACCESSIBLE_RW);
}
