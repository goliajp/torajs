// SSA → LLVM IR via Inkwell, then object file → system `cc` → native binary.
// This is the production codegen path that replaces wasm-via-C.
//
// What this module does:
//   1. Walks `ssa::Module` and emits one LLVM `FunctionValue` per SSA function.
//   2. For runtime intrinsics (currently just `print_i64`), provides the body
//      directly — same shape as the labs/0002-inkwell-spike helper, ported in
//      to keep one source of truth.
//   3. Runs the LLVM new-pass-manager pipeline (`default<O1>` by default,
//      override via `--opt O0|O1|O2|O3` like the spike).
//   4. Writes an object file, then invokes system `cc` to link against libc.
//
// Step 3 of P3.5.a — only fib40 is end-to-end testable. Other bench cases
// reach `ssa_lower` first and panic on `let`/`while`/etc; step 4 fixes that.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

mod arr_builders;
mod arr_helpers;
mod attrs;
mod builders;
mod declares;
mod entry;
mod globals;
mod link;
mod lower;
mod lower_inst;
mod pipeline;
mod split_iter;
mod types;

use lower::FnLower;

pub use entry::{compile, compile_for, compile_for_kind, compile_for_kind_with_cache};

use arr_builders::{define_arr_push, define_arr_push_unchecked, define_arr_shift};
use attrs::{
    is_alloc_intrinsic, mark_alwaysinline, mark_memory_none, mark_noalias_ret, module_uses_fetch,
    ssa_fn_is_pure,
};
use builders::{
    define_obj_alloc, define_obj_drop, define_print_bool, define_print_f64, define_print_i64,
};
use declares::{
    declare_arr_alloc_pooled, declare_arr_free, declare_free, declare_malloc, declare_memcmp,
    declare_memcpy, declare_memmove, declare_putchar, declare_realloc, declare_str_alloc_pooled,
    declare_str_free,
};
use globals::{emit_data_global, emit_static_str_global, emit_string_global};
use split_iter::define_split_iter_next;
use types::declare_ssa_fn;

use inkwell::AddressSpace;
use inkwell::context::Context;
use inkwell::debug_info::{AsDIScope, DIFlagsConstants};
use inkwell::values::FunctionValue;

use crate::ssa::{InstKind, Module};

#[derive(Debug)]
pub enum CompileError {
    Verify(String),
    Pass(String),
    Emit(String),
    Link(String),
}

/// v0.3 #4 D-2 — DWARF emission state. Created when caller passes a
/// `source_path` to `compile`; threaded into pass C / pass D so each
/// fn body can attach a DISubprogram and (D-3) per-instruction
/// DILocation. Finalized once at end-of-compile so the .o ships
/// the dwarf section.
pub(super) struct DebugCtx<'ctx> {
    dibuilder: inkwell::debug_info::DebugInfoBuilder<'ctx>,
    compile_unit: inkwell::debug_info::DICompileUnit<'ctx>,
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompileError::Verify(s) => write!(f, "LLVM verify: {s}"),
            CompileError::Pass(s) => write!(f, "LLVM passes: {s}"),
            CompileError::Emit(s) => write!(f, "object emit: {s}"),
            CompileError::Link(s) => write!(f, "linker: {s}"),
        }
    }
}

/// Compile target. `Native` (default) emits a native binary for the
/// host triple via cc + dsymutil; `Wasm32Wasi` (T-20, v0.6.0) emits
/// a `.wasm` module for the wasm32-wasip1 target via the LLVM 22
/// clang + wasi-libc sysroot + wasm-ld toolchain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompileTarget {
    Native,
    Wasm32Wasi,
}

/// V3-16 — output kind for the link step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputKind {
    /// Default: a runnable executable. Linker resolves a `_main`
    /// symbol synthesized from the program's top-level statements.
    Executable,
    /// Position-independent shared library (.dylib on macOS, .so
    /// on Linux). No `_main` requirement; consumers dlopen the
    /// resulting file and look up exported function symbols by
    /// name. Enables in-process eval via the compile-then-dlopen
    /// pattern (Function ctor substrate).
    SharedLib,
}

/// Serializes every codegen invocation through `compile_for_kind`.
/// LLVM holds non-thread-safe global state (target/pass registration,
/// command-line option parsing, internal statistics), so two compiles
/// running in parallel — e.g. the embed crate's parallel `cargo test`,
/// or any future caller spinning up workers — race on those globals
/// and SIGSEGV/SIGBUS intermittently. The textbook fix is to wrap the
/// unsafe boundary at the source: a single static Mutex around the
/// one funnel point (`compile_for_kind`) — `compile` and `compile_for`
/// both route through it, so all 5 workspace callsites
/// (torajs-embed × 3, torajs-cli × 2) are covered without per-callsite
/// changes. `Mutex::new` is const since Rust 1.63 so this is a true
/// zero-init static. cli is single-threaded (no contention); embed
/// tests serialize their codegen passes (intended — that's the fix).
pub(super) static COMPILE_LOCK: Mutex<()> = Mutex::new(());

/// Hold the original 880-line compile body. Sections 1-3 emit LLVM
/// → .o, then call into `link_object_to_binary`. Cache-write happens
/// post-emit when `fixture_o_cache` is Some.
#[allow(clippy::too_many_arguments)]
pub(super) fn compile_for_kind_impl(
    ssa_module: &Module,
    out_path: &Path,
    opt: &str,
    source_path: Option<&Path>,
    ast: Option<&crate::ast::Ast>,
    target: CompileTarget,
    kind: OutputKind,
    fixture_o_cache: Option<&Path>,
) -> Result<(), CompileError> {
    // Serialize all LLVM codegen — see COMPILE_LOCK doc above. Poisoning
    // can only happen if a previous compile panicked mid-codegen, which
    // would itself be a bug worth surfacing; expect rather than recover.
    let _guard = COMPILE_LOCK
        .lock()
        .expect("ssa_inkwell COMPILE_LOCK poisoned by a prior panicking compile");
    let ctx = Context::create();
    let llvm_module = ctx.create_module("torajs");
    let builder = ctx.create_builder();

    // v0.3 #4 D-2 — set up DIBuilder when caller provided a source
    // path. `Debug Info Version` module flag (= LLVM 3 today) is
    // mandatory; without it the linker drops the DWARF section.
    let debug_ctx = source_path.and_then(|p| {
        let i32_t = ctx.i32_type();
        llvm_module.add_basic_value_flag(
            "Debug Info Version",
            inkwell::module::FlagBehavior::Warning,
            i32_t.const_int(3, false),
        );
        let filename = p.file_name().and_then(|s| s.to_str()).unwrap_or("(stdin)");
        let directory = p.parent().and_then(|d| d.to_str()).unwrap_or(".");
        let (dibuilder, compile_unit) = llvm_module.create_debug_info_builder(
            true,
            inkwell::debug_info::DWARFSourceLanguage::C,
            filename,
            directory,
            "torajs",
            /* is_optimized */ opt != "O0",
            /* compile-line flags */ "",
            /* runtime_ver */ 0,
            /* split_name */ "",
            inkwell::debug_info::DWARFEmissionKind::Full,
            /* dwo_id */ 0,
            /* split_debug_inlining */ false,
            /* debug_info_for_profiling */ false,
            /* sysroot (LLVM 22) */ "",
            /* sdk (LLVM 22) */ "",
        );
        Some(DebugCtx {
            dibuilder,
            compile_unit,
        })
    });

    // Pass A: declare libc decls + the intrinsics whose body the backend owns.
    let putchar = declare_putchar(&ctx, &llvm_module);
    let malloc = declare_malloc(&ctx, &llvm_module, target);
    // memmove binding retained — B1b restored define_arr_push (2026-05-24)
    // which calls memmove for the head-offset compact path.
    let _ = declare_memcpy(&ctx, &llvm_module, target);
    let memmove = declare_memmove(&ctx, &llvm_module, target);
    let _ = declare_memcmp(&ctx, &llvm_module, target);

    // Pass B: emit string-literal globals (LLVM `[N x i8]` private constants).
    // Indexed by StringId so callsites resolve via slice indexing.
    let string_globals: Vec<inkwell::values::GlobalValue> = ssa_module
        .strings
        .iter()
        .enumerate()
        .map(|(i, bytes)| emit_string_global(&ctx, &llvm_module, i, bytes))
        .collect();

    // Pass B.1: per-literal Str-shaped static. Same bytes wrapped in
    // `[hdr:8 STATIC flag][len:8][bytes:N]` — drop-in compatible with
    // a heap Str. Used by `static_str_ref` to short-circuit hot-loop
    // literal allocs (every callsite shares the same global, rc_inc
    // and rc_dec no-op via the STATIC flag).
    let static_str_globals: Vec<inkwell::values::GlobalValue> = ssa_module
        .strings
        .iter()
        .enumerate()
        .map(|(i, bytes)| emit_static_str_global(&ctx, &llvm_module, i, bytes))
        .collect();

    // Pass B' (K.3): emit module-level data globals (top-level `let X: T`
    // where T is a primitive Copy type). Keyed by name so `InstKind::GlobalRef`
    // resolves via lookup.
    let data_globals: HashMap<String, inkwell::values::GlobalValue> = ssa_module
        .data_globals
        .iter()
        .map(|g| (g.name.clone(), emit_data_global(&ctx, &llvm_module, g)))
        .collect();

    // Pass C: walk every SSA function and create a corresponding LLVM
    // FunctionValue. Backend-owned intrinsics get a body here; everything
    // else gets a declaration that pass D fills in.
    let free = declare_free(&ctx, &llvm_module, target);
    // realloc binding retained — B1b restored define_arr_push (2026-05-24)
    // which calls realloc for the grow path. define_arr_reserve still
    // lives in torajs-arr/grow.rs (Rust extern) since it's not hot.
    let realloc = declare_realloc(&ctx, &llvm_module, target);
    // declare_str_free registers the LLVM extern; the Rust binding
    // was the input to define_str_drop, moved to torajs-str (P3.1-g.6).
    let _ = declare_str_free(&ctx, &llvm_module);
    // declare_str_alloc_pooled only registers the LLVM extern; the
    // Rust binding was the input to str_alloc / str_concat / str_slice
    // IR builders, all moved to torajs-str (P3.1-g.{2,4,5}).
    let _ = declare_str_alloc_pooled(&ctx, &llvm_module);
    // __torajs_arr_free body lives in runtime_str.c; declared here so
    // any IR-builder code that emits `call __torajs_arr_free` finds
    // the symbol. define_arr_drop used to consume it directly, but
    // that body moved to torajs-arr::drop (P4.1-a) — `_unused` since
    // no current IR-builder references it, but the declaration is
    // cheap insurance + future-proof.
    let _ = declare_arr_free(&ctx, &llvm_module);
    // __torajs_arr_alloc_pooled declaration kept for any IR-builder
    // call site that emits `call __torajs_arr_alloc_pooled`. Since
    // define_arr_alloc moved to Rust (P4.1-c), no internal IR-builder
    // currently references it; the body still lives in
    // libtorajs_arr.a and ssa_lower emits direct extern calls.
    let _ = declare_arr_alloc_pooled(&ctx, &llvm_module);
    let mut fn_map: Vec<FunctionValue> = Vec::with_capacity(ssa_module.funcs.len());
    for f in &ssa_module.funcs {
        let llvm_fn = match f.name.as_str() {
            "print_i64" => define_print_i64(&ctx, &llvm_module, putchar),
            "print_f64" => define_print_f64(&ctx, &llvm_module),
            "print_bool" => define_print_bool(&ctx, &llvm_module, putchar),
            // __torajs_str_alloc + __torajs_str_print moved to
            // torajs-str::{alloc,print} (P3.1-g.2, 2026-05-23). The IR
            // dispatch arms below + the matching define_str_{alloc,print}
            // fn bodies + the intrinsics-array entries are deleted; the
            // linker resolves both symbols against libtorajs_str.a.
            // __torajs_str_drop moved to torajs-str::alloc (P3.1-g.6,
            // 2026-05-23). Rust impl: NULL check + STATIC_LITERAL gate
            // + rc dec + libc::free (pool-bypass preserved bit-for-bit
            // from the IR shape). fat-LTO inlines the .a body at every
            // scope-end call site so the alwaysinline goal is preserved.
            // **This closes P3.1-g and P3.1 entirely** — runtime_str.c
            // has 0 str fns and ssa_inkwell has 0 define_str_* fns.
            // __torajs_str_concat moved to torajs-str::concat
            // (P3.1-g.4, 2026-05-23). define_str_concat fn body +
            // this dispatch arm + intrinsics-array entry deleted;
            // linker resolves via libtorajs_str.a.
            "__torajs_obj_alloc" => define_obj_alloc(&ctx, &llvm_module, malloc),
            "__torajs_obj_drop" => define_obj_drop(&ctx, &llvm_module, free),
            // __torajs_arr_alloc moved to torajs-arr::alloc (P4.1-c,
            // 2026-05-23). Trivial single-call wrapper around
            // __torajs_arr_alloc_pooled; LTO inlines across the
            // staticlib boundary same as the prior alwaysinline IR.
            "__torajs_arr_push" => {
                // B1b (2026-05-24, "扎实/不简化便宜"): restored 187-LOC
                // IR builder so user-code's push hot loops fold the
                // algorithm into the caller. Linkage = Internal so the
                // same-named external symbol in torajs-arr/grow.rs is
                // free to serve fs/process/promise/regex Rust callers
                // through libtorajs_arr.a without a link clash. The
                // alwaysinline attr makes LLVM splice the body in even
                // at low opt levels.
                let f = define_arr_push(&ctx, &llvm_module, realloc, memmove);
                f.as_global_value()
                    .set_linkage(inkwell::module::Linkage::Internal);
                mark_alwaysinline(&ctx, f);
                f
            }
            "__torajs_arr_shift" => {
                // B4-shift (2026-05-25): restored 4-memory-op IR
                // for the fifo-queue hot path. Same Internal +
                // alwaysinline mechanics as arr_push.
                let f = define_arr_shift(&ctx, &llvm_module);
                f.as_global_value()
                    .set_linkage(inkwell::module::Linkage::Internal);
                mark_alwaysinline(&ctx, f);
                f
            }
            "__torajs_arr_push_unchecked" => {
                // B4-push-unchecked (2026-05-25): restored 5-instr
                // M6.2 fast-path IR for array-literal materializers.
                // Every `[1, 2, ...]` literal element call gets the
                // bl + ret removed.
                let f = define_arr_push_unchecked(&ctx, &llvm_module);
                f.as_global_value()
                    .set_linkage(inkwell::module::Linkage::Internal);
                mark_alwaysinline(&ctx, f);
                f
            }
            "__torajs_split_iter_next" => {
                // P-iter Plan C — body emitted directly in IR (mirror
                // of runtime_str.c's removed C body) so LLVM can
                // inline the byte scan + emit_substr into the caller's
                // for-of loop. Without this, cross-TU LTO fails
                // because the inkwell side emits a native object and
                // Apple's system clang produces incompatible bitcode
                // for the C side. alwaysinline makes the inliner
                // skip cost-model and always splice the body in.
                let f = define_split_iter_next(&ctx, &llvm_module, target);
                mark_alwaysinline(&ctx, f);
                f
            }
            // __torajs_arr_push moved to torajs-arr::grow (P4.1-l,
            // 2026-05-23). Rust impl mirrors 1:1: fast path → compact
            // (head>0) → grow (max(4, cap*2)) → store + len_inc.
            // Native ptr::copy collapses the memmove call; LTO inlines
            // the body across libtorajs_arr.a same as the prior IR.
            // __torajs_arr_reserve moved to torajs-arr::grow (P4.1-k,
            // 2026-05-23). Rust impl is realloc + cap-store + return —
            // LTO across libtorajs_arr.a inlines into the caller same
            // as the prior IR; cap-equal short-circuit preserved.
            // __torajs_arr_push_unchecked moved to torajs-arr::ops
            // (P4.1-c, 2026-05-23). 5-instr fast path now in Rust;
            // LTO across libtorajs_arr.a inlines into the caller
            // same as the prior alwaysinline IR. (define_arr_push
            // with cap-check + grow path stays IR-side until P4.1-d.)
            // __torajs_arr_shift moved to torajs-arr::grow (P4.1-m,
            // 2026-05-23). Pure-Rust port — loses alwaysinline at the
            // staticlib boundary; fat-LTO at `tr build` time can still
            // inline the 4 memory ops. P4.1 fully closed (all named
            // arr_* IR builders ported).
            // __torajs_arr_drop moved to torajs-arr::drop (P4.1-a,
            // 2026-05-23). Pure-Rust port mirrors define_arr_drop's
            // semantics: NULL + FLAG_STATIC_LITERAL gate + rc_dec +
            // last-owner → arrprops_drop_entry + arr_free. Resolved at
            // link time via libtorajs_arr.a; IR builder + always-inline
            // mark deleted.
            // __torajs_str_slice moved to torajs-str::slice (P3.1-g.5,
            // 2026-05-23). Negative-wrap + clamp + alloc + memcpy in
            // Rust core (slice_range fn); IR builder body deleted.
            // __torajs_str_char_code_at moved to torajs-str::lookup
            // (P3.1-g.4, 2026-05-23). The Rust impl is bounds-check
            // + byte load + i64 cast; the alwaysinline + inline-in-
            // lex/parse-hot-loops goal is now LLVM-LTO's job (fat-LTO
            // pulls the .a fn body across to the caller's TU).
            // __torajs_str_{starts_with,ends_with,index_of,includes}
            // (no-_from 2-arg form) moved to torajs-str::lookup
            // (P3.1-g.3, 2026-05-23). Each is a thin wrapper that
            // delegates to its `_from` core. The IR builders + this
            // dispatch arm + the intrinsics-array entries are deleted;
            // the linker resolves all four symbols against
            // libtorajs_str.a.
            // "__torajs_math_sqrt" moved to torajs-num::math (P3.2-a,
            // 2026-05-23). f64::sqrt → libm sqrt, identical to what
            // define_math_unary's IR emitted.
            // **All remaining Math intrinsics moved to torajs-num::math
            // (P3.2-b, 2026-05-23)**. f64 method delegates to libm at
            // the same symbols (sqrt/fabs/floor/.../atan2). The 27 IR
            // dispatch arms + define_math_unary + define_math_binary
            // helpers + 28 intrinsics-array entries deleted. Notable:
            // __torajs_math_round preserves JS spec (floor(x+0.5))
            // not libc round semantics; runtime_str.c C version also
            // deleted in this commit.
            // P2.4-b — throw-slot machinery now provided by the
            // Rust `torajs-throw` crate (statics + extern "C" fns
            // baked into libtorajs_throw.a). Fall through to
            // `declare_ssa_fn` so the module gets an external
            // declaration; the linker resolves at `tr build` time.
            _ => declare_ssa_fn(&ctx, &llvm_module, f, target),
        };
        // Tag malloc-shaped intrinsics with `noalias` on the return so
        // LLVM can hoist invariant loads through stores via other heap
        // pointers. Concrete win: in tight loops over `arr.length`
        // where the body writes to a different array, the length load
        // moves out of the loop because the two pointers are provably
        // disjoint. See `mark_noalias_ret` for the criterion (only
        // genuine fresh-pointer producers — not arr_push / arr_reserve
        // which can return the same input ptr on the no-grow path).
        if is_alloc_intrinsic(&f.name) {
            mark_noalias_ret(&ctx, llvm_fn);
        }
        fn_map.push(llvm_fn);
    }

    /* Pass C.5 (T-24): emit `__vtable_<C>` globals — one `[N x ptr]`
     * constant per class, slot[i] populated with the FunctionValue
     * for the deepest ancestor of C that owns method[i]. Slots with
     * no impl in C's MRO get null. The keying name `__vtable_<C>`
     * lets the GlobalRef("__vtable_<C>") resolution path below pick
     * them up. */
    let vtable_globals: HashMap<String, inkwell::values::GlobalValue> = {
        let mut out = HashMap::new();
        let ptr_t = ctx.ptr_type(AddressSpace::default());
        for vt in &ssa_module.vtable_globals {
            let n = vt.fn_ids.len();
            let arr_t = ptr_t.array_type(n as u32);
            let elems: Vec<inkwell::values::PointerValue> = vt
                .fn_ids
                .iter()
                .map(|opt| match opt {
                    Some(fid) => fn_map[fid.0 as usize].as_global_value().as_pointer_value(),
                    None => ptr_t.const_null(),
                })
                .collect();
            let arr = ptr_t.const_array(&elems);
            let g = llvm_module.add_global(arr_t, None, &format!("__vtable_{}", vt.class_name));
            g.set_initializer(&arr);
            g.set_constant(true);
            g.set_linkage(inkwell::module::Linkage::Private);
            g.set_unnamed_addr(true);
            out.insert(format!("__vtable_{}", vt.class_name), g);
        }
        out
    };

    /* Pass C.6 (T-26.C): emit per-class children-offset tables
     * for the cycle collector. Two globals:
     *   __torajs_class_layouts        — `[N x { u32 n; ptr offsets }]`
     *   __torajs_n_class_layouts      — u32 = N
     * The runtime indexes by `class_tag - 1`. The collector reads
     * each entry's `offsets[]` to enumerate refcounted-pointer
     * fields during mark/scan/collect.
     *
     * Each entry's offsets array is itself a private constant `[K x i32]`
     * global; the entry holds a pointer to it. K can be 0 (class
     * has no refcounted fields → entry is `{0, NULL}`). */
    {
        let i32_t = ctx.i32_type();
        let ptr_t = ctx.ptr_type(AddressSpace::default());
        let entry_t = ctx.struct_type(&[i32_t.into(), ptr_t.into()], false);
        let n = ssa_module.class_layouts.len();
        let mut entries: Vec<inkwell::values::StructValue> = Vec::with_capacity(n);
        for (i, layout) in ssa_module.class_layouts.iter().enumerate() {
            let offsets_ptr = if layout.child_offsets.is_empty() {
                ptr_t.const_null()
            } else {
                let arr_t = i32_t.array_type(layout.child_offsets.len() as u32);
                let consts: Vec<inkwell::values::IntValue> = layout
                    .child_offsets
                    .iter()
                    .map(|o| i32_t.const_int(*o as u64, false))
                    .collect();
                let arr = i32_t.const_array(&consts);
                let g = llvm_module.add_global(arr_t, None, &format!(".__class_offsets_{i}"));
                g.set_initializer(&arr);
                g.set_constant(true);
                g.set_linkage(inkwell::module::Linkage::Private);
                g.set_unnamed_addr(true);
                g.as_pointer_value()
            };
            let n_children = i32_t.const_int(layout.child_offsets.len() as u64, false);
            let entry = ctx.const_struct(&[n_children.into(), offsets_ptr.into()], false);
            entries.push(entry);
        }
        let table_t = entry_t.array_type(n as u32);
        let table_init = entry_t.const_array(&entries);
        let table_g = llvm_module.add_global(table_t, None, "__torajs_class_layouts");
        table_g.set_initializer(&table_init);
        table_g.set_constant(true);
        table_g.set_linkage(inkwell::module::Linkage::External);
        let count_g = llvm_module.add_global(i32_t, None, "__torajs_n_class_layouts");
        count_g.set_initializer(&i32_t.const_int(n as u64, false));
        count_g.set_constant(true);
        count_g.set_linkage(inkwell::module::Linkage::External);
    }

    // Pass D: lower bodies for every SSA function that has blocks AND isn't
    // a backend-owned intrinsic.
    let intrinsics = [
        "print_i64",
        "print_f64",
        "print_bool",
        // __torajs_str_alloc + __torajs_str_print moved to torajs-str
        // (P3.1-g.2). Removed from this dispatch list so Pass D no
        // longer hunts for an IR body; symbols resolve at link time
        // against libtorajs_str.a.
        // "__torajs_str_drop" moved to torajs-str::alloc (P3.1-g.6)
        // "__torajs_str_concat" moved to torajs-str::concat (P3.1-g.4)
        "__torajs_obj_alloc",
        "__torajs_obj_drop",
        "__torajs_arr_alloc",
        // "__torajs_arr_push" moved to torajs-arr::grow (P4.1-l)
        // "__torajs_arr_shift" moved to torajs-arr::grow (P4.1-m)
        // "__torajs_arr_reserve" moved to torajs-arr::grow (P4.1-k)
        "__torajs_arr_push_unchecked",
        "__torajs_arr_drop",
        // "__torajs_str_slice" moved to torajs-str::slice (P3.1-g.5)
        // "__torajs_str_char_code_at" moved to torajs-str::lookup (P3.1-g.4)
        // __torajs_str_{starts_with,ends_with,index_of,includes}
        // (no-_from variants) moved to torajs-str::lookup (P3.1-g.3).
        // The Pass D dispatch loop no longer needs to find IR bodies
        // for them; resolved via libtorajs_str.a at link.
        // "__torajs_math_sqrt" moved to torajs-num::math (P3.2-a)
        // ** All Math intrinsics moved to torajs-num::math (P3.2-{a,b}) **
        "__torajs_throw_set",
        "__torajs_throw_check",
        "__torajs_throw_take",
        "__torajs_throw_take_tag",
    ];

    // v0.3 #4 D-2 — attach DISubprogram to every fn that's about to
    // lower its body via FnLower (i.e. user fns + runtime fns
    // synthesized at SSA layer). Done BEFORE the lowering loop so
    // each FnLower::run can pick up the subprogram and emit
    // !dbg-equipped instructions. Backend-owned intrinsics
    // (str_alloc, str_drop, ...) skip this — they're emitted by
    // their `define_*` fns which don't take a debug ctx; LLVM is
    // happy to leave them DI-less since they have no DISubprogram.
    let sub_ty_opt = debug_ctx.as_ref().map(|dctx| {
        dctx.dibuilder.create_subroutine_type(
            dctx.compile_unit.get_file(),
            None,
            &[],
            inkwell::debug_info::DIFlags::PUBLIC,
        )
    });
    if let (Some(dctx), Some(sub_ty)) = (debug_ctx.as_ref(), sub_ty_opt.as_ref()) {
        for (i, f) in ssa_module.funcs.iter().enumerate() {
            if f.is_declaration() || intrinsics.contains(&f.name.as_str()) {
                continue;
            }
            let llvm_fn = fn_map[i];
            // line_no = 0 placeholder until D-3 carries fn-decl line.
            // 0 is DWARF's "unknown"; tools fall back to scope-only.
            let sp = dctx.dibuilder.create_function(
                dctx.compile_unit.as_debug_info_scope(),
                &f.name,
                None,
                dctx.compile_unit.get_file(),
                0,
                *sub_ty,
                /* is_local_to_unit */ false,
                /* is_definition */ true,
                /* scope_line */ 0,
                inkwell::debug_info::DIFlags::PUBLIC,
                /* is_optimized */ opt != "O0",
            );
            llvm_fn.set_subprogram(sp);
        }
    }

    for (i, f) in ssa_module.funcs.iter().enumerate() {
        if f.is_declaration() || intrinsics.contains(&f.name.as_str()) {
            continue;
        }
        // v0.3 #4 D-2 — set a default DILocation for this fn so
        // LLVM verify's "inlinable call needs !dbg" rule is
        // satisfied. line=0 / col=0 is a placeholder; D-3 will
        // override per-instruction with actual span lookup.
        if let Some(dctx) = debug_ctx.as_ref()
            && let Some(sp) = fn_map[i].get_subprogram()
        {
            let loc =
                dctx.dibuilder
                    .create_debug_location(&ctx, 0, 0, sp.as_debug_info_scope(), None);
            builder.set_current_debug_location(loc);
        }
        let lower = FnLower {
            ctx: &ctx,
            builder: &builder,
            ssa_fn: f,
            llvm_fn: fn_map[i],
            fn_map: &fn_map,
            string_globals: &string_globals,
            static_str_globals: &static_str_globals,
            data_globals: &data_globals,
            vtable_globals: &vtable_globals,
            ssa_module,
            ast,
            debug_ctx: debug_ctx.as_ref(),
            block_map: HashMap::new(),
            value_map: HashMap::new(),
        };
        lower.run();
    }

    // v0.3 #4 D-2 — finalize DI metadata before LLVM verify (which
    // rejects incomplete DICompileUnits).
    if let Some(dctx) = &debug_ctx {
        dctx.dibuilder.finalize();
    }

    /* v0.6+1 perf checkpoint — fn-purity attribute pass.
     *
     * Walks every user FnDecl's lowered SSA body; if it has zero
     * memory access (no Load / Store / Call / Alloca etc — see
     * ssa_fn_is_pure for the exact predicate), tag the LLVM fn
     * with `memory(none)`. LLVM's LICM / GVN then know calls to
     * the fn have zero side effects → invariant loads in the
     * caller's loops can be hoisted past the call.
     *
     * Concrete win: `function id<T>(x: T): T { return x }` in a
     * tight `for (let i = 0; i < xs.length; i++) sum += id(xs[i])`
     * loop. Pre-tag, LLVM reloads `xs.length` (and the array data
     * pointer) on every iteration because the call to `id` could,
     * in principle, modify them. With memory(none) the loads
     * hoist out and the loop becomes the same shape as rust's
     * inlined version. */
    for (i, f) in ssa_module.funcs.iter().enumerate() {
        if f.is_declaration() || intrinsics.contains(&f.name.as_str()) {
            continue;
        }
        /* `main` always touches memory (top-level user code) and
         * doesn't benefit from the attr — skip it explicitly. */
        if f.name == "main" {
            continue;
        }
        if ssa_fn_is_pure(f) {
            mark_memory_none(&ctx, fn_map[i]);
        }
    }

    /* P-PERF.A1 (2026-05-22) — Internal-linkage pass for user
     * FnDecls (plus all tora-synthesized internal helpers like
     * `__closure_N` / `__env_drop_N` / `__cm_<C>__<m>` /
     * `__forward_<name>`). Pre-P-PERF.A1 those fns were emitted
     * with default LLVM linkage (External) — which forces LLVM
     * to assume the symbol may be called from outside the module.
     * Consequence: no IPSCCP, no per-callsite specialization, no
     * inlining of single-call helpers. Concrete leakage on the
     * `reduce(xs, add1)` shape — closure-pipeline-1m's hot loop
     * is `tail call i64 %f(i64 %arg)` through an opaque fn-ptr
     * because LLVM can't see `add1` flows in from main.
     *
     * With Internal linkage:
     *  - IPSCCP folds the fn-ptr param to a constant per call site
     *  - Inliner inlines the reducer body into main
     *  - The (now-direct) inner call to add1 inlines too
     *  - LLVM unrolls + vectorizes the loop
     *
     * Scope: Executable output only. SharedLib output (torajs-embed
     * crate's `compile_to_dylib` path) MUST keep user fns External
     * — the embedding host (C / Rust runtime) dlopens the .dylib
     * and looks up user-fn symbols by name; Internal linkage hides
     * them from the dynamic symbol table.
     *
     * Skip set within Executable:
     *  - declarations (extern C runtime helpers — MUST stay External)
     *  - intrinsics (tora-defined fns called cross-TU by the C runtime
     *    side; their symbols must be visible at link time)
     *  - `main` (renamed to platform-specific entry by declare_ssa_fn;
     *    the OS / wasi-libc resolves it as External by ABI)
     *  - `__main_argc_argv` (wasi-libc lookup target; same reason).
     */
    if matches!(kind, OutputKind::Executable) {
        for (i, f) in ssa_module.funcs.iter().enumerate() {
            if f.is_declaration() || intrinsics.contains(&f.name.as_str()) {
                continue;
            }
            if f.name == "main" || f.name == "__main_argc_argv" {
                continue;
            }
            fn_map[i].set_linkage(inkwell::module::Linkage::Internal);
        }

        /* P-PERF.A3 (2026-05-22) — alwaysinline for small,
         * non-recursive user fns. LLVM's cost-model inliner is
         * conservative on fns with loops; small reducer-shaped
         * helpers (add1, gcd inner, is_prime, etc.) that get
         * called from a tight outer loop benefit from
         * unconditional inline → enables downstream vectorize /
         * unroll on the (now flattened) inner loop.
         *
         * Heuristic:
         *   - body SSA inst count below threshold (60), AND
         *   - no Call back to self anywhere (recursive fns are
         *     skipped — alwaysinline would infinite-expand).
         *
         * Threshold tuning history:
         *   - 60 (P-PERF.A3, 2026-05-22 ship): real wins on
         *     numeric/array hot paths; only promise-all-1k +4%
         *     borderline regression.
         *   - 30 (P-PERF.A4 attempt, reverted): net worse than
         *     60 across the suite (csv-rebuild +8.6%, async-fn-call
         *     +8.5%, rpn-eval +11.2% vs A1). Missed the 30-60-inst
         *     mid-size helper range where A3 was actually winning.
         *     Geomean 4.16 → 4.15 (-0.3%). The "i-cache pressure"
         *     hypothesis for the A3 promise-all regression turned
         *     out wrong — promise-all's cost is elsewhere; the
         *     mid-size helpers are NET positive.
         *   - 60 (current, A4 reverted): sweet spot per empirical
         *     measurement. Don't drop without rebenchmarking on
         *     the full 26-case suite.
         *
         * Recursive case (fib40 / ackermann) explicitly excluded.
         * `main` already excluded above. */
        for (i, f) in ssa_module.funcs.iter().enumerate() {
            if f.is_declaration() || intrinsics.contains(&f.name.as_str()) {
                continue;
            }
            if f.name == "main" || f.name == "__main_argc_argv" {
                continue;
            }
            let inst_count: usize = f.blocks.iter().map(|b| b.insts.len()).sum();
            if inst_count >= 60 {
                continue;
            }
            let self_recursive = f.blocks.iter().any(|b| {
                b.insts.iter().any(|inst| {
                    matches!(
                        inst.kind,
                        InstKind::Call(fid, _) if ssa_module.func_name(fid) == f.name.as_str()
                    )
                })
            });
            if self_recursive {
                continue;
            }
            mark_alwaysinline(&ctx, fn_map[i]);
        }
    }

    // Compute uses_fetch from the SSA module BEFORE we hand off to
    // pipeline::emit_and_link (which doesn't see the SSA). Used both
    // for the link step decision and for the optional cache sidecar.
    let uses_fetch = module_uses_fetch(ssa_module);

    pipeline::emit_and_link(
        &llvm_module,
        opt,
        source_path,
        target,
        kind,
        out_path,
        fixture_o_cache,
        uses_fetch,
    )
}

pub(super) fn ctx_f64<'ctx>(ctx: &'ctx Context) -> inkwell::types::FloatType<'ctx> {
    ctx.f64_type()
}
