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
use std::path::{Path, PathBuf};
use std::sync::Mutex;

mod arr_builders;
mod arr_helpers;
mod attrs;
mod builders;
mod declares;
mod globals;
mod link;
mod split_iter;

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
use link::{link_object_to_binary, rand_suffix, read_uses_fetch_sidecar, write_uses_fetch_sidecar};
use split_iter::define_split_iter_next;

use inkwell::AddressSpace;
use inkwell::OptimizationLevel;
use inkwell::context::Context;
use inkwell::debug_info::{AsDIScope, DIFlagsConstants};
use inkwell::module::Module as LlvmModule;
use inkwell::passes::PassBuilderOptions;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
};
use inkwell::types::{BasicMetadataTypeEnum, BasicTypeEnum, FunctionType};
use inkwell::values::{BasicMetadataValueEnum, BasicValueEnum, FunctionValue, IntValue};
use inkwell::{FloatPredicate, IntPredicate};

use crate::ssa::{self as s, BinOp, FPred, IPred, InstKind, Module, Operand, Terminator, Type};

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
struct DebugCtx<'ctx> {
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

/// Compile an SSA module to a native binary at `out_path`. `opt` selects the
/// LLVM new-pass-manager pipeline ("O0" / "O1" / "O2" / "O3"); the default
/// is "O1" because that's the bench-tuned setting for fib40.
///
/// `source_path` (v0.3 #4 D-2) — when supplied, emits DWARF
/// debug-info: a DICompileUnit + DIFile pinned to the .ts source,
/// and per-fn DISubprogram so backtrace tools (atos, addr2line) see
/// `tr` fns as proper named scopes. D-3 will plumb per-instruction
/// DILocation; D-4 wires runtime panic backtraces into this.
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

pub fn compile(
    ssa_module: &Module,
    out_path: &Path,
    opt: &str,
    source_path: Option<&Path>,
    ast: Option<&crate::ast::Ast>,
) -> Result<(), CompileError> {
    compile_for(
        ssa_module,
        out_path,
        opt,
        source_path,
        ast,
        CompileTarget::Native,
    )
}

pub fn compile_for(
    ssa_module: &Module,
    out_path: &Path,
    opt: &str,
    source_path: Option<&Path>,
    ast: Option<&crate::ast::Ast>,
    target: CompileTarget,
) -> Result<(), CompileError> {
    compile_for_kind(
        ssa_module,
        out_path,
        opt,
        source_path,
        ast,
        target,
        OutputKind::Executable,
    )
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
static COMPILE_LOCK: Mutex<()> = Mutex::new(());

/// V3-16 — extended entry point that lets the caller pick
/// executable vs shared-lib output. `compile_for` keeps the
/// existing executable-only signature so existing callers
/// (`tr build`, `tr run`, bench harness) don't need to thread
/// the new param.
pub fn compile_for_kind(
    ssa_module: &Module,
    out_path: &Path,
    opt: &str,
    source_path: Option<&Path>,
    ast: Option<&crate::ast::Ast>,
    target: CompileTarget,
    kind: OutputKind,
) -> Result<(), CompileError> {
    compile_for_kind_with_cache(
        ssa_module,
        out_path,
        opt,
        source_path,
        ast,
        target,
        kind,
        None,
    )
}

/// B-1 phase 2 — variant of `compile_for_kind` that takes an
/// optional path to a cached per-fixture `.o` file.
///
/// Fast path (cache hit): copy cached `.o` → temp, skip the LLVM
/// pipeline entirely (parse/check/lower happened upstream in the
/// caller; the cached .o is byte-identical to what LLVM would emit
/// for the same source against the same compiler-rev). Read the
/// `.uses_fetch` sidecar to decide whether to add `-lcurl`. Jump
/// straight to runtime cc + final link.
///
/// Slow path (cache miss, or cache None): runs the full LLVM compile
/// as before. On miss with a cache slot provided, also copies the
/// freshly produced `.o` + uses_fetch sidecar into the cache slot
/// (atomically) for future hits.
///
/// `fixture_o_cache` key contract: caller MUST ensure the same source
/// + opt + compiler_rev produces the same cache path (see
/// `TORAJS_COMPILER_REV` in build.rs). Mismatch = silent stale .o.
pub fn compile_for_kind_with_cache(
    ssa_module: &Module,
    out_path: &Path,
    opt: &str,
    source_path: Option<&Path>,
    ast: Option<&crate::ast::Ast>,
    target: CompileTarget,
    kind: OutputKind,
    fixture_o_cache: Option<&Path>,
) -> Result<(), CompileError> {
    // Fast path: cached fixture .o exists → skip the entire LLVM
    // pipeline, jump straight to link.
    if let Some(cache_p) = fixture_o_cache
        && cache_p.is_file()
    {
        let _guard = COMPILE_LOCK
            .lock()
            .expect("ssa_inkwell COMPILE_LOCK poisoned by a prior panicking compile");
        let pid = std::process::id();
        let obj_path: PathBuf =
            std::env::temp_dir().join(format!("torajs-llvm-{}-{}.o", pid, rand_suffix()));
        std::fs::copy(cache_p, &obj_path)
            .map_err(|e| CompileError::Link(format!("copy cached fixture .o: {e}")))?;
        let uses_fetch = read_uses_fetch_sidecar(cache_p);
        let result = link_object_to_binary(
            &obj_path,
            out_path,
            opt,
            source_path,
            target,
            kind,
            uses_fetch,
        );
        let _ = std::fs::remove_file(&obj_path);
        return result;
    }

    // Slow path: full LLVM compile.
    let result = compile_for_kind_impl(
        ssa_module,
        out_path,
        opt,
        source_path,
        ast,
        target,
        kind,
        fixture_o_cache,
    );
    result
}

/// Hold the original 880-line compile body. Sections 1-3 emit LLVM
/// → .o, then call into `link_object_to_binary`. Cache-write happens
/// post-emit when `fixture_o_cache` is Some.
#[allow(clippy::too_many_arguments)]
fn compile_for_kind_impl(
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

    // Pass D: verify, optimize, emit, link.
    if let Err(e) = llvm_module.verify() {
        return Err(CompileError::Verify(e.to_string()));
    }

    let (triple, cpu, features) = match target {
        CompileTarget::Native => {
            Target::initialize_aarch64(&InitializationConfig::default());
            (
                TargetMachine::get_default_triple(),
                TargetMachine::get_host_cpu_name().to_string(),
                TargetMachine::get_host_cpu_features().to_string(),
            )
        }
        CompileTarget::Wasm32Wasi => {
            // T-20 (v0.6.0) — initialize the WebAssembly backend in
            // LLVM 22. wasm32-wasip1 is the canonical target triple
            // (LLVM 22 deprecated the older "wasm32-wasi" spelling).
            // No cpu / feature tuning — the default subset works on
            // every wasm engine.
            Target::initialize_webassembly(&InitializationConfig::default());
            (
                inkwell::targets::TargetTriple::create("wasm32-wasip1"),
                String::new(),
                String::new(),
            )
        }
    };
    let target_obj = Target::from_triple(&triple).map_err(|e| CompileError::Emit(e.to_string()))?;
    /* Codegen optimization level. NOT bumped to Aggressive: empirically
     * measured a net -1.5% geomean regression at OptLevel::Aggressive
     * (2026-05-22 / P-PERF.A2 attempt: gcd1m / generic-id / mandelbrot
     * +1–6% but async-fn-call +14%, promise-all +11%, startup +4.7%
     * regressed past noise. Net-negative on Promise/closure allocation
     * patterns — Aggressive's register-pressure/peephole changes hurt
     * the alloc-heavy paths more than they help the pure-numeric ones).
     * Keep Less; the IR pipeline runs at `default<O3>` (per `opt`
     * above) which is where the bulk of optimization lives. */
    /* Reloc mode stays at PIC. A P-PERF.A5 attempt switched native
     * Executable to Static (2026-05-22, reverted same day): hoped
     * to elide GOT indirection on cross-TU calls, but the bench
     * cycle ran on a thermal-loaded machine and showed correlated
     * tora-and-bun regression of 5–15 % across most cases (high
     * shared noise; couldn't isolate the Static-vs-PIC signal
     * cleanly). geomean vs bun-aot 4.155 → 4.145, vs node-v8 20.86
     * → 19.99 — both within noise and not a clear improvement
     * direction. Keeping PIC until a quiescent-machine rerun can
     * give a cleaner measurement, or until PGO arrives and re-
     * justifies the reloc question. Archived bench evidence at
     * bench/results/2026-05-22-mini-3bf6002.json. */
    let machine = target_obj
        .create_target_machine(
            &triple,
            &cpu,
            &features,
            OptimizationLevel::Less,
            RelocMode::PIC,
            CodeModel::Default,
        )
        .ok_or_else(|| CompileError::Emit("create_target_machine returned None".into()))?;
    // Pin the module's triple + datalayout for non-native targets so
    // the WebAssembly verifier sees a matching ABI. Native target
    // intentionally skips this — LLVM's implicit host detection picks
    // the right datalayout AND keeps a faster optimization path that
    // an explicit `set_data_layout` (even with the same string)
    // disables. Measured: explicitly setting on native costs ~17% on
    // the bench geomean (T-20.a regression that only surfaced at the
    // v0.6.0 perf gate). wasm always needs the explicit set or the
    // verifier rejects mismatched host-vs-target datalayout.
    if matches!(target, CompileTarget::Wasm32Wasi) {
        llvm_module.set_triple(&triple);
        llvm_module.set_data_layout(&machine.get_target_data().get_data_layout());
    }

    let pipeline = format!("default<{opt}>");
    llvm_module
        .run_passes(&pipeline, &machine, PassBuilderOptions::create())
        .map_err(|e| CompileError::Pass(format!("{pipeline}: {}", e.to_string())))?;

    let obj_path: PathBuf = std::env::temp_dir().join(format!(
        "torajs-llvm-{}-{}.o",
        std::process::id(),
        rand_suffix()
    ));
    machine
        .write_to_file(&llvm_module, FileType::Object, &obj_path)
        .map_err(|e| CompileError::Emit(e.to_string()))?;
    /* T-20.b debug — when env var set, also dump LLVM IR + .o
     * copy for postmortem of wasm signature errors. */
    if std::env::var("TR_DEBUG_KEEP").is_ok() {
        let _ = std::fs::write(
            "/tmp/torajs-debug.ll",
            llvm_module.print_to_string().to_string(),
        );
        let _ = std::fs::copy(&obj_path, "/tmp/torajs-debug.o");
    }

    // Compute uses_fetch from the SSA module BEFORE we hand off to
    // link_object_to_binary (which doesn't see the SSA). Used both
    // for the link step decision and for the optional cache sidecar.
    let uses_fetch = module_uses_fetch(ssa_module);

    // B-1 phase 2 — cache write: copy the freshly produced .o + write
    // a uses_fetch sidecar so future hits can rebuild the link command
    // without scanning the SSA. Atomic (tmp + rename) so concurrent
    // workers don't see half-written files. Same-content races are
    // benign (last writer wins; bytes are deterministic).
    if let Some(cache_p) = fixture_o_cache {
        if let Some(parent) = cache_p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let tmp = cache_p.with_extension(format!("tmp-{}-{}.o", std::process::id(), rand_suffix()));
        if std::fs::copy(&obj_path, &tmp).is_ok() {
            let _ = std::fs::rename(&tmp, cache_p);
            write_uses_fetch_sidecar(cache_p, uses_fetch);
        }
    }

    // Hand off to the link stage. obj_path will be cleaned up by the
    // link function (it removes the .o it was given). uses_fetch is
    // passed explicitly so the link path doesn't need ssa_module.
    let result = link_object_to_binary(
        &obj_path,
        out_path,
        opt,
        source_path,
        target,
        kind,
        uses_fetch,
    );
    return result;
}

/// `__torajs_arr_alloc_pooled(uint64_t cap) -> void*` — pool-aware
/// Array alloc. For cap ≤ POOL_CAP_MAX, scans the cap-indexed LIFO
/// for a matching block; falls through to malloc + header init on
/// miss. Defined in runtime_str.c. Inkwell's arr_alloc IR fn
/// delegates here so fn-local literal allocs (`let xs = [a, b, c]`
/// inside a tight loop) reuse the same block per iter instead of
/// mallocing.
/* STR_HDR_* offsets + STR_HDR_TAG_STR moved out with the IR
 * builders that consumed them (P3.1-g.{2..5} ported Str fns to
 * torajs-str; this file no longer emits per-byte Str access).
 * Layout invariants now live in `crates/torajs-str/src/layout.rs`. */

/* __torajs_str_alloc moved to torajs-str::alloc (P3.1-g.2, 2026-05-23).
 * The Rust extern wrapper does StrBlock::alloc + copy_from_slice in
 * one fn — equivalent to the IR shape (str_alloc_pooled + memcpy)
 * collapsed into a single staticlib call. */

/* __torajs_str_concat moved to torajs-str::concat (P3.1-g.4,
 * 2026-05-23). Rust impl: StrBlock::alloc + 2 copy_from_slice. */

/* __torajs_str_slice moved to torajs-str::slice (P3.1-g.5,
 * 2026-05-23). Rust impl: slice_range core (neg-wrap + clamp +
 * no-swap empty) + StrBlock::alloc + copy_from_slice. */

/* __torajs_str_char_code_at moved to torajs-str::lookup (P3.1-g.4,
 * 2026-05-23). Rust impl: bounds check + byte load + i64 cast. */

/* __torajs_str_{starts_with,ends_with,index_of,includes} (no-_from
 * 2-arg form) moved to torajs-str::lookup (P3.1-g.3, 2026-05-23). The
 * 3 IR builders (define_str_prefix_suffix_check + define_str_index_of
 * + define_str_includes) deleted, total ~210 LOC; Rust wrappers are
 * thin (1 line each calling the `_from` core). */

/* define_math_unary + define_math_binary helpers deleted (P3.2-b,
 * 2026-05-23). All 27 Math intrinsics moved to torajs-num::math
 * (P3.2-{a,b}); both helpers had no callers left. Rust f64 methods
 * delegate to the same libm symbols (sqrt/fabs/floor/.../atan2) the
 * IR builders emitted. */

// P2.4-b (2026-05-23 architecture-rewrite) — M4 exception state
// (active/tag/value globals + throw_set/check/take/take_tag
// helpers) moved out of LLVM-IR emit and into the Rust
// `torajs-throw` crate. The four extern symbols + the (no-longer-
// visible) statics are baked into libtorajs_throw.a and linked
// into every `tr build` user binary alongside libtorajs_rc.a /
// libtorajs_anyvalue.a. ssa_lower-emitted code now resolves them
// via the regular external-fn declaration path (the
// `declare_ssa_fn` fall-through in the `compile()` match further
// up). The old `ensure_throw_globals` + four `define_throw_*`
// IR-builders are deleted in this commit.

/* define_arr_alloc body moved to torajs-arr::alloc (P4.1-c, 2026-05-23).
 * Trivial single-call wrapper around __torajs_arr_alloc_pooled; LTO
 * inlines across the staticlib boundary. */

/* define_arr_drop body moved to torajs-arr::drop (P4.1-a, 2026-05-23).
 * Pure-Rust port mirrors IR shape 1:1: NULL + FLAG_STATIC_LITERAL gate
 * + rc_dec + last-owner → arrprops_drop_entry + arr_free. Resolved at
 * link time via libtorajs_arr.a. */

/* __torajs_str_drop moved to torajs-str::alloc (P3.1-g.6, 2026-05-23).
 * **P3.1-g closed — 0 IR-side str defines remaining**. Rust impl
 * preserves IR bit-for-bit: NULL check + STATIC_LITERAL gate +
 * rc-- + libc::free (pool-bypass intentional; pool fed only by
 * explicit __torajs_str_free callers, not by scope-end drop). */

/* __torajs_str_print moved to torajs-str::print (P3.1-g.2, 2026-05-23).
 * Rust impl uses std::io::stdout().lock().write_all for byte-equivalent
 * output with one syscall instead of len+1 putchar calls. The cross-
 * buffer reordering concern that motivated per-byte putchar here is
 * preserved by the Rust lock guard: the Rust Stdout handle shares fd
 * 1 with stdio's putchar (used by print_i64 etc.), and the OS-level
 * line buffering keeps order consistent regardless of the in-process
 * buffer choice. */

fn declare_ssa_fn<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    f: &s::Function,
    target: CompileTarget,
) -> FunctionValue<'ctx> {
    /* The synthesized `main` entry takes argc/argv at the LLVM ABI
     * level so the C runtime can capture them for `process.argv`.
     * SSA-side `main` has no params; the LLVM signature is widened
     * here, and the entry block emits a call to
     * `__torajs_argv_init(argc, argv)` before running user code
     * (see lower_user_fn for the init-call emission). */
    if f.name == "main" {
        let i32_t = ctx.i32_type();
        let ptr_t = ctx.ptr_type(AddressSpace::default());
        let fn_t = i32_t.fn_type(&[i32_t.into(), ptr_t.into()], false);
        // T-20.b — wasi-libc's `__main_void` looks up the user's
        // entry point under the internal name `__main_argc_argv`
        // (clang aliases `main` to this on the wasi32 ABI; we
        // emit IR directly so we have to mint the alias explicitly
        // by naming our symbol that way). Native keeps the
        // standard `main` so the OS / cc entry resolves cleanly.
        let real_name = match target {
            CompileTarget::Native => "main",
            CompileTarget::Wasm32Wasi => "__main_argc_argv",
        };
        return m.add_function(real_name, fn_t, None);
    }
    let param_tys: Vec<Type> = f.params.iter().map(|&p| f.value_type(p)).collect();
    let fn_t = build_fn_type(ctx, &param_tys, f.ret);
    m.add_function(&f.name, fn_t, None)
}

fn build_fn_type<'ctx>(ctx: &'ctx Context, params: &[Type], ret: Type) -> FunctionType<'ctx> {
    let param_metas: Vec<BasicMetadataTypeEnum> =
        params.iter().map(|&t| basic_meta_type(ctx, t)).collect();
    match ret {
        Type::Void => ctx.void_type().fn_type(&param_metas, false),
        Type::I64 => ctx.i64_type().fn_type(&param_metas, false),
        Type::I32 => ctx.i32_type().fn_type(&param_metas, false),
        Type::F64 => ctx.f64_type().fn_type(&param_metas, false),
        Type::Bool => ctx.bool_type().fn_type(&param_metas, false),
        Type::Ptr
        | Type::Str
        | Type::Substr
        | Type::Obj(_)
        | Type::Arr(_)
        | Type::FnSig(_)
        | Type::Closure(_)
        | Type::RegExp
        | Type::Date
        | Type::Any
        | Type::Symbol
        | Type::Promise
        | Type::BigInt
        | Type::WeakRef
        | Type::WeakMap
        | Type::WeakSet
        | Type::Map
        | Type::Set
        | Type::MapIter
        | Type::ArrIter => ctx
            .ptr_type(AddressSpace::default())
            .fn_type(&param_metas, false),
    }
}

fn basic_meta_type<'ctx>(ctx: &'ctx Context, t: Type) -> BasicMetadataTypeEnum<'ctx> {
    match t {
        Type::I64 => ctx.i64_type().into(),
        Type::I32 => ctx.i32_type().into(),
        Type::F64 => ctx.f64_type().into(),
        Type::Bool => ctx.bool_type().into(),
        // Str + Ptr both lower to a single opaque pointer. The SSA-level
        // distinction matters for the lowerer's dispatch decisions, not for
        // codegen.
        Type::Ptr
        | Type::Str
        | Type::Substr
        | Type::Obj(_)
        | Type::Arr(_)
        | Type::FnSig(_)
        | Type::Closure(_)
        | Type::RegExp
        | Type::Date
        | Type::Any
        | Type::Symbol
        | Type::Promise
        | Type::BigInt
        | Type::WeakRef
        | Type::WeakMap
        | Type::WeakSet
        | Type::Map
        | Type::Set
        | Type::MapIter
        | Type::ArrIter => ctx.ptr_type(AddressSpace::default()).into(),
        Type::Void => panic!("void cannot be a parameter type"),
    }
}

/// SSA Type → Inkwell BasicTypeEnum. Used by alloca / load to specify the
/// stack slot or load width. Void is intentionally not representable here.
fn basic_type<'ctx>(ctx: &'ctx Context, t: Type) -> BasicTypeEnum<'ctx> {
    match t {
        Type::I64 => ctx.i64_type().into(),
        Type::I32 => ctx.i32_type().into(),
        Type::F64 => ctx.f64_type().into(),
        Type::Bool => ctx.bool_type().into(),
        Type::Ptr
        | Type::Str
        | Type::Substr
        | Type::Obj(_)
        | Type::Arr(_)
        | Type::FnSig(_)
        | Type::Closure(_)
        | Type::RegExp
        | Type::Date
        | Type::Any
        | Type::Symbol
        | Type::Promise
        | Type::BigInt
        | Type::WeakRef
        | Type::WeakMap
        | Type::WeakSet
        | Type::Map
        | Type::Set
        | Type::MapIter
        | Type::ArrIter => ctx.ptr_type(AddressSpace::default()).into(),
        Type::Void => panic!("void cannot be a basic type (alloca/load/store)"),
    }
}

struct FnLower<'a, 'ctx> {
    ctx: &'ctx Context,
    builder: &'a inkwell::builder::Builder<'ctx>,
    ssa_fn: &'a s::Function,
    llvm_fn: FunctionValue<'ctx>,
    fn_map: &'a [FunctionValue<'ctx>],
    string_globals: &'a [inkwell::values::GlobalValue<'ctx>],
    /// Phase P-rpn — per-literal Str-shaped statics; same indexing as
    /// `string_globals`. Resolved by `InstKind::StaticStrRef`.
    static_str_globals: &'a [inkwell::values::GlobalValue<'ctx>],
    /// Phase K.3 — module-level data globals indexed by name. Looked
    /// up by `InstKind::GlobalRef` to yield the slot's pointer value.
    data_globals: &'a HashMap<String, inkwell::values::GlobalValue<'ctx>>,
    /// T-24 — per-class vtable globals (`__vtable_<C>` → const ptr
    /// array). Resolved by `InstKind::GlobalRef` after `data_globals`
    /// lookup misses, so vtable references piggyback on the existing
    /// SSA primitive without a new InstKind.
    vtable_globals: &'a HashMap<String, inkwell::values::GlobalValue<'ctx>>,
    /// Whole SSA module — needed by `InstKind::CallIndirect` to look up
    /// the signature interner. Read-only; no mutation. M2 Phase B Stage 3.
    ssa_module: &'a s::Module,
    /// v0.3 #4 D-3 — Optional source-location resolver. When present,
    /// per-Inst `lower_inst` looks up `inst.origin` → `ast.expr_spans`
    /// → `byte_to_line_col` → DILocation, attaching it to subsequent
    /// build_* calls so DWARF backtraces resolve to `.ts:line:col`.
    /// None when the caller didn't supply ast / source_path.
    ast: Option<&'a crate::ast::Ast>,
    debug_ctx: Option<&'a DebugCtx<'ctx>>,
    block_map: HashMap<u32, inkwell::basic_block::BasicBlock<'ctx>>,
    value_map: HashMap<u32, BasicValueEnum<'ctx>>,
}

impl<'a, 'ctx> FnLower<'a, 'ctx> {
    fn run(mut self) {
        // Phase 1: pre-create LLVM blocks for every SSA block so terminators
        // can reference forward blocks.
        for b in &self.ssa_fn.blocks {
            let bb = self
                .ctx
                .append_basic_block(self.llvm_fn, &format!("bb{}", b.id.0));
            self.block_map.insert(b.id.0, bb);
        }
        // Bind params: SSA params → LLVM function parameters, by position.
        for (i, &p) in self.ssa_fn.params.iter().enumerate() {
            let v = self
                .llvm_fn
                .get_nth_param(i as u32)
                .expect("param count mismatch");
            self.value_map.insert(p.0, v);
        }
        // Phase 2: lower each block.
        for (b_idx, b) in self.ssa_fn.blocks.iter().enumerate() {
            let bb = self.block_map[&b.id.0];
            self.builder.position_at_end(bb);
            /* v0.3 #3.c — at the start of `main`'s entry block, emit
             * an init call to capture argc/argv into runtime globals
             * for `process.argv` / `Bun.argv` access. The LLVM main
             * is widened to `(i32 argc, ptr argv)` by declare_ssa_fn;
             * here we forward those params to __torajs_argv_init.
             * Done before the user's main body runs. */
            if b_idx == 0 && self.ssa_fn.name == "main" {
                if let (Some(argc), Some(argv)) =
                    (self.llvm_fn.get_nth_param(0), self.llvm_fn.get_nth_param(1))
                {
                    /* fn_map indexes by the SSA module's func order;
                     * find __torajs_argv_init by name in the SSA fns. */
                    for (i, sf) in self.ssa_module.funcs.iter().enumerate() {
                        if sf.name == "__torajs_argv_init" {
                            let init_fn = self.fn_map[i];
                            self.builder
                                .build_call(init_fn, &[argc.into(), argv.into()], "")
                                .unwrap();
                            break;
                        }
                    }
                }
            }
            for inst in &b.insts {
                self.lower_inst(inst);
            }
            self.lower_term(&b.term);
        }
    }

    fn lower_inst(&mut self, inst: &s::Inst) {
        // v0.3 #4 D-3 — when DWARF debug info is enabled, look up
        // this Inst's `origin` ExprId, translate its byte span to
        // (line, col), and stamp a DILocation on the builder so all
        // build_* calls until the next override carry !dbg.
        // origin == None (synthetic Insts not tied to a user-Expr)
        // inherits the previous DILocation; this matches DWARF's
        // intent for compiler-emitted helper sequences.
        if let (Some(dctx), Some(ast), Some(eid), Some(sp)) = (
            self.debug_ctx,
            self.ast,
            inst.origin,
            self.llvm_fn.get_subprogram(),
        ) {
            let span = ast.expr_spans.get(eid.0 as usize).copied();
            if let Some(span) = span {
                let (line, col) = ast.byte_to_line_col(span.start);
                if line > 0 {
                    let loc = dctx.dibuilder.create_debug_location(
                        self.ctx,
                        line,
                        col,
                        sp.as_debug_info_scope(),
                        None,
                    );
                    self.builder.set_current_debug_location(loc);
                }
            }
        }
        let result_val = match &inst.kind {
            InstKind::BinOp(op, a, b) => {
                let r: BasicValueEnum = match op {
                    BinOp::Add
                    | BinOp::Sub
                    | BinOp::Mul
                    | BinOp::SDiv
                    | BinOp::SRem
                    | BinOp::And
                    | BinOp::Or
                    | BinOp::Xor
                    | BinOp::Shl
                    | BinOp::AShr
                    | BinOp::LShr => {
                        let av = self.operand_int(a);
                        let bv = self.operand_int(b);
                        let r = match op {
                            BinOp::Add => self.builder.build_int_add(av, bv, "").unwrap(),
                            BinOp::Sub => self.builder.build_int_sub(av, bv, "").unwrap(),
                            BinOp::Mul => self.builder.build_int_mul(av, bv, "").unwrap(),
                            BinOp::SDiv => self.builder.build_int_signed_div(av, bv, "").unwrap(),
                            BinOp::SRem => self.builder.build_int_signed_rem(av, bv, "").unwrap(),
                            BinOp::And => self.builder.build_and(av, bv, "").unwrap(),
                            BinOp::Or => self.builder.build_or(av, bv, "").unwrap(),
                            BinOp::Xor => self.builder.build_xor(av, bv, "").unwrap(),
                            BinOp::Shl => self.builder.build_left_shift(av, bv, "").unwrap(),
                            BinOp::AShr => {
                                self.builder.build_right_shift(av, bv, true, "").unwrap()
                            }
                            BinOp::LShr => {
                                self.builder.build_right_shift(av, bv, false, "").unwrap()
                            }
                            _ => unreachable!(),
                        };
                        BasicValueEnum::IntValue(r)
                    }
                    BinOp::FAdd | BinOp::FSub | BinOp::FMul | BinOp::FDiv | BinOp::FRem => {
                        let av = self.operand(a).into_float_value();
                        let bv = self.operand(b).into_float_value();
                        let r = match op {
                            BinOp::FAdd => self.builder.build_float_add(av, bv, "").unwrap(),
                            BinOp::FSub => self.builder.build_float_sub(av, bv, "").unwrap(),
                            BinOp::FMul => self.builder.build_float_mul(av, bv, "").unwrap(),
                            BinOp::FDiv => self.builder.build_float_div(av, bv, "").unwrap(),
                            BinOp::FRem => self.builder.build_float_rem(av, bv, "").unwrap(),
                            _ => unreachable!(),
                        };
                        BasicValueEnum::FloatValue(r)
                    }
                };
                Some(r)
            }
            InstKind::ICmp(p, a, b) => {
                let pred = match p {
                    IPred::Eq => IntPredicate::EQ,
                    IPred::Ne => IntPredicate::NE,
                    IPred::Slt => IntPredicate::SLT,
                    IPred::Sgt => IntPredicate::SGT,
                    IPred::Sle => IntPredicate::SLE,
                    IPred::Sge => IntPredicate::SGE,
                };
                // Allow pointer compares (used by `=== null` / `!== null`
                // and the optional-chain / nullish dispatchers). LLVM's
                // build_int_compare accepts ptr-typed operands; mixing
                // one ptr + one i64 needs an explicit ptrtoint cast on
                // the ptr side.
                let av_basic = self.operand(a);
                let bv_basic = self.operand(b);
                let av_is_ptr = matches!(av_basic, BasicValueEnum::PointerValue(_));
                let bv_is_ptr = matches!(bv_basic, BasicValueEnum::PointerValue(_));
                let r = if av_is_ptr && bv_is_ptr {
                    self.builder
                        .build_int_compare(
                            pred,
                            av_basic.into_pointer_value(),
                            bv_basic.into_pointer_value(),
                            "",
                        )
                        .unwrap()
                } else if av_is_ptr || bv_is_ptr {
                    let i64_t = self.ctx.i64_type();
                    let av_int = if av_is_ptr {
                        self.builder
                            .build_ptr_to_int(av_basic.into_pointer_value(), i64_t, "")
                            .unwrap()
                    } else {
                        av_basic.into_int_value()
                    };
                    let bv_int = if bv_is_ptr {
                        self.builder
                            .build_ptr_to_int(bv_basic.into_pointer_value(), i64_t, "")
                            .unwrap()
                    } else {
                        bv_basic.into_int_value()
                    };
                    self.builder
                        .build_int_compare(pred, av_int, bv_int, "")
                        .unwrap()
                } else {
                    let av = av_basic.into_int_value();
                    let bv = bv_basic.into_int_value();
                    self.builder.build_int_compare(pred, av, bv, "").unwrap()
                };
                Some(BasicValueEnum::IntValue(r))
            }
            InstKind::FCmp(p, a, b) => {
                let av = self.operand(a).into_float_value();
                let bv = self.operand(b).into_float_value();
                let pred = match p {
                    FPred::Oeq => FloatPredicate::OEQ,
                    FPred::One => FloatPredicate::ONE,
                    FPred::Olt => FloatPredicate::OLT,
                    FPred::Ogt => FloatPredicate::OGT,
                    FPred::Ole => FloatPredicate::OLE,
                    FPred::Oge => FloatPredicate::OGE,
                    FPred::Une => FloatPredicate::UNE,
                };
                let r = self.builder.build_float_compare(pred, av, bv, "").unwrap();
                Some(BasicValueEnum::IntValue(r))
            }
            InstKind::SiToFp(op) => {
                let v = self.operand_int(op);
                let f = ctx_f64(self.ctx);
                let r = self.builder.build_signed_int_to_float(v, f, "").unwrap();
                Some(BasicValueEnum::FloatValue(r))
            }
            InstKind::FpToSi(op) => {
                let v = self.operand(op).into_float_value();
                let i = self.ctx.i64_type();
                let r = self.builder.build_float_to_signed_int(v, i, "").unwrap();
                Some(BasicValueEnum::IntValue(r))
            }
            InstKind::ZExtBoolToI64(op) => {
                let v = self.operand_int(op);
                let i64_ty = self.ctx.i64_type();
                let r = self.builder.build_int_z_extend(v, i64_ty, "").unwrap();
                Some(BasicValueEnum::IntValue(r))
            }
            InstKind::BitCastF64ToI64(op) => {
                // T-10.d.ii — pun the f64's IEEE 754 bit pattern as i64
                // for the ANY_F64 tagged-slot stash. LLVM `bitcast`
                // preserves bits exactly (vs `fptosi` which truncates).
                let v = self.operand(op).into_float_value();
                let i64_ty = self.ctx.i64_type();
                let r = self.builder.build_bit_cast(v, i64_ty, "").unwrap();
                Some(r)
            }
            InstKind::BitCastI64ToF64(op) => {
                let v = self.operand_int(op);
                let f64_ty = self.ctx.f64_type();
                let r = self.builder.build_bit_cast(v, f64_ty, "").unwrap();
                Some(r)
            }
            InstKind::IntToPtr(op) => {
                // T-15.g.6.c — i64 → ptr (opaque pointer at LLVM 22).
                // Used by the await Member-access dispatch when
                // Promise<T>'s inner T is heap-typed: runtime helper
                // returns int64_t per its C ABI; SSA needs the result
                // typed as ptr-shape so downstream Member / Index
                // instructions dispatch correctly.
                let v = self.operand_int(op);
                let ptr_ty = self.ctx.ptr_type(AddressSpace::default());
                let r = self.builder.build_int_to_ptr(v, ptr_ty, "").unwrap();
                Some(BasicValueEnum::PointerValue(r))
            }
            InstKind::TruncI64ToBool(op) => {
                // T-15.g.6.c — i64 → i1 narrow. Symmetric reverse
                // of ZExtBoolToI64. Pack/unpack across the Promise's
                // int64_t value slot.
                let v = self.operand_int(op);
                let i1_ty = self.ctx.bool_type();
                let r = self.builder.build_int_truncate(v, i1_ty, "").unwrap();
                Some(BasicValueEnum::IntValue(r))
            }
            InstKind::StringRef(sid) => {
                let g = self.string_globals[sid.0 as usize];
                Some(BasicValueEnum::PointerValue(g.as_pointer_value()))
            }
            InstKind::StaticStrRef(sid) => {
                let g = self.static_str_globals[sid.0 as usize];
                Some(BasicValueEnum::PointerValue(g.as_pointer_value()))
            }
            InstKind::GlobalRef(name) => {
                let g = self
                    .data_globals
                    .get(name)
                    .or_else(|| self.vtable_globals.get(name))
                    .unwrap_or_else(|| panic!("ssa-inkwell: unknown global `{name}`"));
                Some(BasicValueEnum::PointerValue(g.as_pointer_value()))
            }
            InstKind::Call(fid, args) => {
                // M6.1 / Array<string> — coerce ptr ↔ i64 args at the
                // call boundary. SSA's i64 / Ptr / Str / Obj / Arr /
                // FnSig / Closure are all 8-byte values but LLVM IR's
                // verifier requires explicit ptrtoint / inttoptr at call
                // sites where the function expected one but got the
                // other. (Cranelift is size-based and accepts either
                // silently, hence the JIT path was working before this
                // patch.) Only fires when the type kinds genuinely
                // differ — same-shape calls are zero-cost.
                let callee = self.fn_map[fid.0 as usize];
                let expected = callee.get_type().get_param_types();
                let i64_t = self.ctx.i64_type();
                let f64_t = self.ctx.f64_type();
                let ptr_t = self.ctx.ptr_type(AddressSpace::default());
                let mut argv: Vec<BasicMetadataValueEnum> = Vec::with_capacity(args.len());
                for (i, a) in args.iter().enumerate() {
                    let raw = self.operand(a);
                    let coerced: BasicValueEnum = if i < expected.len() {
                        match expected[i] {
                            BasicMetadataTypeEnum::IntType(it) => match raw {
                                BasicValueEnum::PointerValue(p) => {
                                    self.builder.build_ptr_to_int(p, i64_t, "").unwrap().into()
                                }
                                BasicValueEnum::FloatValue(f) => {
                                    // Float arg into an int param —
                                    // truncate via fptosi (matches JS
                                    // ToInt32 / ToUint32 prefix on
                                    // Math.imul / charAt-with-float-index
                                    // / parseInt-with-float-radix).
                                    let _ = it;
                                    self.builder
                                        .build_float_to_signed_int(f, i64_t, "")
                                        .unwrap()
                                        .into()
                                }
                                _ => raw,
                            },
                            BasicMetadataTypeEnum::FloatType(_) => match raw {
                                BasicValueEnum::IntValue(v) => self
                                    .builder
                                    .build_signed_int_to_float(v, f64_t, "")
                                    .unwrap()
                                    .into(),
                                _ => raw,
                            },
                            BasicMetadataTypeEnum::PointerType(_) => {
                                if let BasicValueEnum::IntValue(v) = raw {
                                    self.builder.build_int_to_ptr(v, ptr_t, "").unwrap().into()
                                } else {
                                    raw
                                }
                            }
                            _ => raw,
                        }
                    } else {
                        raw
                    };
                    argv.push(coerced.into());
                }
                let call = self.builder.build_call(callee, &argv, "").unwrap();
                let kind = call.try_as_basic_value();
                if kind.is_basic() {
                    Some(kind.unwrap_basic())
                } else {
                    None // void call
                }
            }
            InstKind::Alloca(t) => {
                let bt = basic_type(self.ctx, *t);
                let p = self.builder.build_alloca(bt, "").unwrap();
                Some(BasicValueEnum::PointerValue(p))
            }
            InstKind::AllocaBytes(n) => {
                // i8 array of n elements — yields a `[N x i8]*` of
                // exactly N bytes, 1-byte aligned by default. We bump
                // alignment to 8 since both SplitIter and Substr have
                // 8-byte fields.
                let i8_t = self.ctx.i8_type();
                let arr_t = i8_t.array_type(*n as u32);
                let p = self.builder.build_alloca(arr_t, "").unwrap();
                p.as_instruction().unwrap().set_alignment(8).unwrap();
                Some(BasicValueEnum::PointerValue(p))
            }
            InstKind::Load(t, ptr, offset) => {
                let bt = basic_type(self.ctx, *t);
                let p = self.operand(ptr).into_pointer_value();
                let p = if *offset == 0 {
                    p
                } else {
                    let i64_t = self.ctx.i64_type();
                    let i8_t = self.ctx.i8_type();
                    unsafe {
                        self.builder
                            .build_in_bounds_gep(i8_t, p, &[i64_t.const_int(*offset, false)], "")
                            .unwrap()
                    }
                };
                let v = self.builder.build_load(bt, p, "").unwrap();
                Some(v)
            }
            InstKind::Store(val, ptr, offset) => {
                let v = self.operand(val);
                let p = self.operand(ptr).into_pointer_value();
                let p = if *offset == 0 {
                    p
                } else {
                    let i64_t = self.ctx.i64_type();
                    let i8_t = self.ctx.i8_type();
                    unsafe {
                        self.builder
                            .build_in_bounds_gep(i8_t, p, &[i64_t.const_int(*offset, false)], "")
                            .unwrap()
                    }
                };
                self.builder.build_store(p, v).unwrap();
                None
            }
            InstKind::LoadDyn(t, base, off) => {
                let bt = basic_type(self.ctx, *t);
                let p = self.operand(base).into_pointer_value();
                let i8_t = self.ctx.i8_type();
                let off_v = self.operand_int(off);
                let addr = unsafe {
                    self.builder
                        .build_in_bounds_gep(i8_t, p, &[off_v], "")
                        .unwrap()
                };
                let v = self.builder.build_load(bt, addr, "").unwrap();
                Some(v)
            }
            InstKind::StoreDyn(val, base, off) => {
                let v = self.operand(val);
                let p = self.operand(base).into_pointer_value();
                let i8_t = self.ctx.i8_type();
                let off_v = self.operand_int(off);
                let addr = unsafe {
                    self.builder
                        .build_in_bounds_gep(i8_t, p, &[off_v], "")
                        .unwrap()
                };
                self.builder.build_store(addr, v).unwrap();
                None
            }
            InstKind::FnAddr(fid) => {
                // Take the address of an imported fn — Inkwell's
                // FunctionValue exposes its global address via
                // `as_global_value().as_pointer_value()`.
                let target = self.fn_map[fid.0 as usize];
                let p = target.as_global_value().as_pointer_value();
                Some(BasicValueEnum::PointerValue(p))
            }
            InstKind::CallIndirect(sig_id, ptr, args) => {
                // Look up the interned signature, build the LLVM
                // FunctionType, then build_indirect_call.
                let (params, ret) = self.ssa_module.signature(*sig_id).clone();
                let fn_t = build_fn_type(self.ctx, &params, ret);
                let p = self.operand(ptr).into_pointer_value();
                let argv: Vec<BasicMetadataValueEnum> =
                    args.iter().map(|a| self.operand(a).into()).collect();
                let call = self
                    .builder
                    .build_indirect_call(fn_t, p, &argv, "")
                    .unwrap();
                let kind = call.try_as_basic_value();
                if kind.is_basic() {
                    Some(kind.unwrap_basic())
                } else {
                    None
                }
            }
        };

        if let (Some(r), Some(v)) = (inst.result, result_val) {
            self.value_map.insert(r.0, v);
        }
    }

    fn lower_term(&self, t: &Terminator) {
        match t {
            Terminator::Br(b) => {
                let bb = self.block_map[&b.0];
                self.builder.build_unconditional_branch(bb).unwrap();
            }
            Terminator::CondBr {
                cond,
                then_blk,
                else_blk,
            } => {
                let cv = self.operand_int(cond); // i1
                let tb = self.block_map[&then_blk.0];
                let eb = self.block_map[&else_blk.0];
                self.builder.build_conditional_branch(cv, tb, eb).unwrap();
            }
            Terminator::Ret(maybe) => match maybe {
                Some(o) => {
                    let v = self.operand(o);
                    // M4.3 — same ptr↔i64 cast as the Call boundary,
                    // applied at Ret. Throw's `ret <sentinel>` always
                    // emits ConstI64(0); when the fn's signature
                    // returns ptr-shaped (string / obj / arr / closure),
                    // LLVM rejects `ret i64` against `ret ptr` without
                    // an explicit inttoptr.
                    let ret_ty = self.llvm_fn.get_type().get_return_type();
                    let coerced: BasicValueEnum = match (v, ret_ty) {
                        (BasicValueEnum::IntValue(iv), Some(rt)) if rt.is_pointer_type() => {
                            let ptr_t = self.ctx.ptr_type(AddressSpace::default());
                            self.builder.build_int_to_ptr(iv, ptr_t, "").unwrap().into()
                        }
                        (BasicValueEnum::PointerValue(pv), Some(rt)) if rt.is_int_type() => {
                            let i64_t = self.ctx.i64_type();
                            self.builder.build_ptr_to_int(pv, i64_t, "").unwrap().into()
                        }
                        _ => v,
                    };
                    self.builder.build_return(Some(&coerced)).unwrap();
                }
                None => {
                    self.builder.build_return(None).unwrap();
                }
            },
            Terminator::Unreachable => {
                self.builder.build_unreachable().unwrap();
            }
        }
    }

    fn operand(&self, o: &Operand) -> BasicValueEnum<'ctx> {
        match o {
            Operand::Value(v) => *self
                .value_map
                .get(&v.0)
                .unwrap_or_else(|| panic!("unmapped SSA value {}", v.0)),
            Operand::ConstI64(n) => {
                BasicValueEnum::IntValue(self.ctx.i64_type().const_int(*n as u64, true))
            }
            Operand::ConstI32(n) => {
                BasicValueEnum::IntValue(self.ctx.i32_type().const_int(*n as u64, true))
            }
            Operand::ConstF64(n) => BasicValueEnum::FloatValue(self.ctx.f64_type().const_float(*n)),
            Operand::ConstBool(b) => {
                BasicValueEnum::IntValue(self.ctx.bool_type().const_int(*b as u64, false))
            }
            Operand::ConstPtrNull => BasicValueEnum::PointerValue(
                self.ctx
                    .ptr_type(inkwell::AddressSpace::default())
                    .const_null(),
            ),
        }
    }

    fn operand_int(&self, o: &Operand) -> IntValue<'ctx> {
        self.operand(o).into_int_value()
    }
}

fn ctx_f64<'ctx>(ctx: &'ctx Context) -> inkwell::types::FloatType<'ctx> {
    ctx.f64_type()
}
