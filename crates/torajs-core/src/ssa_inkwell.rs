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
use std::process::Command;
use std::sync::Mutex;

use inkwell::AddressSpace;
use inkwell::OptimizationLevel;
use inkwell::attributes::{Attribute, AttributeLoc};
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
    let memcpy = declare_memcpy(&ctx, &llvm_module, target);
    let memmove = declare_memmove(&ctx, &llvm_module, target);
    let memcmp = declare_memcmp(&ctx, &llvm_module, target);

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
    let realloc = declare_realloc(&ctx, &llvm_module, target);
    let str_free = declare_str_free(&ctx, &llvm_module);
    let str_alloc_pooled = declare_str_alloc_pooled(&ctx, &llvm_module);
    let arr_free = declare_arr_free(&ctx, &llvm_module);
    let arr_alloc_pooled = declare_arr_alloc_pooled(&ctx, &llvm_module);
    let mut fn_map: Vec<FunctionValue> = Vec::with_capacity(ssa_module.funcs.len());
    for f in &ssa_module.funcs {
        let llvm_fn = match f.name.as_str() {
            "print_i64" => define_print_i64(&ctx, &llvm_module, putchar),
            "print_f64" => define_print_f64(&ctx, &llvm_module),
            "print_bool" => define_print_bool(&ctx, &llvm_module, putchar),
            "__torajs_str_alloc" => {
                let f = define_str_alloc(&ctx, &llvm_module, str_alloc_pooled, memcpy);
                // hot — every literal materialization + every concat/slice
                // result. Body is pool fast-path / malloc + header init + memcpy.
                mark_alwaysinline(&ctx, f);
                f
            }
            "__torajs_str_print" => define_str_print(&ctx, &llvm_module, putchar),
            "__torajs_str_drop" => {
                // hot in any non-trivial program (every Str scope-end
                // dec). Body is NULL check + atomic-like dec + cond
                // pool-or-free.
                let f = define_str_drop(&ctx, &llvm_module, str_free);
                mark_alwaysinline(&ctx, f);
                f
            }
            "__torajs_str_concat" => {
                define_str_concat(&ctx, &llvm_module, str_alloc_pooled, memcpy)
            }
            "__torajs_obj_alloc" => define_obj_alloc(&ctx, &llvm_module, malloc),
            "__torajs_obj_drop" => define_obj_drop(&ctx, &llvm_module, free),
            "__torajs_arr_alloc" => {
                // hot — every Array literal materialization. Body is
                // a single tail call into the pool-aware C runtime
                // helper; alwaysinline so the call collapses at LTO.
                let f = define_arr_alloc(&ctx, &llvm_module, arr_alloc_pooled);
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
            "__torajs_arr_push" => define_arr_push(&ctx, &llvm_module, realloc, memmove),
            "__torajs_arr_reserve" => define_arr_reserve(&ctx, &llvm_module, realloc),
            "__torajs_arr_push_unchecked" => {
                let f = define_arr_push_unchecked(&ctx, &llvm_module);
                // hot intrinsic, body is ~5 instructions; force-inline so
                // map / filter / spread loops don't pay a fn-call per push
                mark_alwaysinline(&ctx, f);
                f
            }
            "__torajs_arr_shift" => {
                /* T-13.5 deque shift, body has its own alwaysinline marker
                 * inside define_arr_shift since it's small AND in a tight
                 * loop (fifo-queue pattern). */
                define_arr_shift(&ctx, &llvm_module)
            }
            "__torajs_arr_drop" => {
                let f = define_arr_drop(&ctx, &llvm_module, arr_free);
                // every Array scope-end + every refcounted-elem-walk
                // bottom hits this; body is NULL check + dec + cond free
                mark_alwaysinline(&ctx, f);
                f
            }
            "__torajs_str_slice" => define_str_slice(&ctx, &llvm_module, str_alloc_pooled, memcpy),
            "__torajs_str_char_code_at" => {
                let f = define_str_char_code_at(&ctx, &llvm_module);
                // hot intrinsic — body is bounds check + byte load + zext.
                // Force-inline so per-iter `s.charCodeAt(i)` collapses to
                // a single byte load in lex / parse hot loops
                mark_alwaysinline(&ctx, f);
                f
            }
            "__torajs_str_starts_with" => define_str_prefix_suffix_check(
                &ctx,
                &llvm_module,
                memcmp,
                "__torajs_str_starts_with",
                false,
            ),
            "__torajs_str_ends_with" => define_str_prefix_suffix_check(
                &ctx,
                &llvm_module,
                memcmp,
                "__torajs_str_ends_with",
                true,
            ),
            "__torajs_str_index_of" => define_str_index_of(&ctx, &llvm_module, memcmp),
            "__torajs_str_includes" => {
                // index_of must be defined first — it is, since the
                // pass-A loop iterates ssa_module.funcs in declaration
                // order and we declare str_index_of before str_includes
                // in ssa_lower.
                let index_of = llvm_module
                    .get_function("__torajs_str_index_of")
                    .expect("str_index_of must be defined first");
                define_str_includes(&ctx, &llvm_module, index_of)
            }
            "__torajs_math_sqrt" => {
                define_math_unary(&ctx, &llvm_module, "__torajs_math_sqrt", "sqrt")
            }
            "__torajs_math_abs" => {
                define_math_unary(&ctx, &llvm_module, "__torajs_math_abs", "fabs")
            }
            "__torajs_math_floor" => {
                define_math_unary(&ctx, &llvm_module, "__torajs_math_floor", "floor")
            }
            "__torajs_math_ceil" => {
                define_math_unary(&ctx, &llvm_module, "__torajs_math_ceil", "ceil")
            }
            "__torajs_math_log" => {
                define_math_unary(&ctx, &llvm_module, "__torajs_math_log", "log")
            }
            "__torajs_math_exp" => {
                define_math_unary(&ctx, &llvm_module, "__torajs_math_exp", "exp")
            }
            "__torajs_math_pow" => {
                define_math_binary(&ctx, &llvm_module, "__torajs_math_pow", "pow")
            }
            "__torajs_math_min" => {
                define_math_binary(&ctx, &llvm_module, "__torajs_math_min", "fmin")
            }
            "__torajs_math_max" => {
                define_math_binary(&ctx, &llvm_module, "__torajs_math_max", "fmax")
            }
            // Note: `__torajs_math_round` is defined in runtime_str.c
            // (via `floor(x + 0.5)`) NOT via libc `round` — JS spec
            // rounds half-values toward +∞ (round(-2.5) === -2) but
            // libc rounds away from zero (round(-2.5) === -3).
            "__torajs_math_trunc" => {
                define_math_unary(&ctx, &llvm_module, "__torajs_math_trunc", "trunc")
            }
            "__torajs_math_sin" => {
                define_math_unary(&ctx, &llvm_module, "__torajs_math_sin", "sin")
            }
            "__torajs_math_cos" => {
                define_math_unary(&ctx, &llvm_module, "__torajs_math_cos", "cos")
            }
            "__torajs_math_tan" => {
                define_math_unary(&ctx, &llvm_module, "__torajs_math_tan", "tan")
            }
            "__torajs_math_asin" => {
                define_math_unary(&ctx, &llvm_module, "__torajs_math_asin", "asin")
            }
            "__torajs_math_acos" => {
                define_math_unary(&ctx, &llvm_module, "__torajs_math_acos", "acos")
            }
            "__torajs_math_atan" => {
                define_math_unary(&ctx, &llvm_module, "__torajs_math_atan", "atan")
            }
            "__torajs_math_atan2" => {
                define_math_binary(&ctx, &llvm_module, "__torajs_math_atan2", "atan2")
            }
            "__torajs_math_log2" => {
                define_math_unary(&ctx, &llvm_module, "__torajs_math_log2", "log2")
            }
            "__torajs_math_log10" => {
                define_math_unary(&ctx, &llvm_module, "__torajs_math_log10", "log10")
            }
            "__torajs_math_cbrt" => {
                define_math_unary(&ctx, &llvm_module, "__torajs_math_cbrt", "cbrt")
            }
            "__torajs_math_sinh" => {
                define_math_unary(&ctx, &llvm_module, "__torajs_math_sinh", "sinh")
            }
            "__torajs_math_cosh" => {
                define_math_unary(&ctx, &llvm_module, "__torajs_math_cosh", "cosh")
            }
            "__torajs_math_tanh" => {
                define_math_unary(&ctx, &llvm_module, "__torajs_math_tanh", "tanh")
            }
            "__torajs_math_asinh" => {
                define_math_unary(&ctx, &llvm_module, "__torajs_math_asinh", "asinh")
            }
            "__torajs_math_acosh" => {
                define_math_unary(&ctx, &llvm_module, "__torajs_math_acosh", "acosh")
            }
            "__torajs_math_atanh" => {
                define_math_unary(&ctx, &llvm_module, "__torajs_math_atanh", "atanh")
            }
            "__torajs_math_expm1" => {
                define_math_unary(&ctx, &llvm_module, "__torajs_math_expm1", "expm1")
            }
            "__torajs_math_log1p" => {
                define_math_unary(&ctx, &llvm_module, "__torajs_math_log1p", "log1p")
            }
            "__torajs_throw_set" => define_throw_set(&ctx, &llvm_module),
            "__torajs_throw_check" => define_throw_check(&ctx, &llvm_module),
            "__torajs_throw_take" => define_throw_take(&ctx, &llvm_module),
            "__torajs_throw_take_tag" => define_throw_take_tag(&ctx, &llvm_module),
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
        "__torajs_str_alloc",
        "__torajs_str_print",
        "__torajs_str_drop",
        "__torajs_str_concat",
        "__torajs_obj_alloc",
        "__torajs_obj_drop",
        "__torajs_arr_alloc",
        "__torajs_arr_push",
        "__torajs_arr_shift",
        "__torajs_arr_reserve",
        "__torajs_arr_push_unchecked",
        "__torajs_arr_drop",
        "__torajs_str_slice",
        "__torajs_str_char_code_at",
        "__torajs_str_starts_with",
        "__torajs_str_ends_with",
        "__torajs_str_index_of",
        "__torajs_str_includes",
        "__torajs_math_sqrt",
        "__torajs_math_abs",
        "__torajs_math_floor",
        "__torajs_math_ceil",
        "__torajs_math_log",
        "__torajs_math_exp",
        "__torajs_math_pow",
        "__torajs_math_min",
        "__torajs_math_max",
        "__torajs_math_round",
        "__torajs_math_trunc",
        "__torajs_math_sin",
        "__torajs_math_cos",
        "__torajs_math_tan",
        "__torajs_math_asin",
        "__torajs_math_acos",
        "__torajs_math_atan",
        "__torajs_math_atan2",
        "__torajs_math_log2",
        "__torajs_math_log10",
        "__torajs_math_cbrt",
        "__torajs_math_sinh",
        "__torajs_math_cosh",
        "__torajs_math_tanh",
        "__torajs_math_asinh",
        "__torajs_math_acosh",
        "__torajs_math_atanh",
        "__torajs_math_expm1",
        "__torajs_math_log1p",
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

    // M6.1+ — torajs's C runtime. Pieces that are clearer in C than
    // via the inkwell IR-builder API (string split, array join,
    // anything future where IR builder verbosity outweighs the
    // link-cost gain). Embedded via include_str! in torajs-runtime
    // and recompiled fresh per `tr build`; adds ~10-30 ms to the
    // AOT pipeline per C TU (negligible vs LLVM optimize).
    //
    // Each .c declares its own copy of __torajs_heap_header_t (binary
    // compatible) and links against __torajs_rc_dec from runtime_str.c.
    // Each compiles to its own .o; all link with the LLVM-emitted main .o.
    //
    // v0.3 #6 Graduation — C sources live in the torajs-runtime crate
    // so their ABI is locked behind a stable crate boundary. Sources
    // enumerated in `torajs_runtime::SOURCES` so adding a new TU is
    // a single line in lib.rs (no per-file scaffolding here). v0.5
    // T-15 added runtime_promise.c via this path.
    let pid = std::process::id();
    let mut c_paths: Vec<PathBuf> = Vec::with_capacity(torajs_runtime::SOURCES.len());
    let mut o_paths: Vec<PathBuf> = Vec::with_capacity(torajs_runtime::SOURCES.len());
    for (filename, _) in torajs_runtime::SOURCES {
        let stem = filename.trim_end_matches(".c");
        c_paths.push(
            std::env::temp_dir().join(format!("torajs-runtime-{stem}-{pid}-{}.c", rand_suffix())),
        );
        o_paths.push(
            std::env::temp_dir().join(format!("torajs-runtime-{stem}-{pid}-{}.o", rand_suffix())),
        );
    }
    for (idx, (filename, src)) in torajs_runtime::SOURCES.iter().enumerate() {
        std::fs::write(&c_paths[idx], src)
            .map_err(|e| CompileError::Link(format!("write {filename}: {e}")))?;
    }
    // T-20 (v0.6.0) — for wasm32-wasi, use LLVM 22 clang with the
    // wasm32-wasip1 triple + wasi-libc sysroot from Homebrew. cc on
    // macOS is Apple's clang which doesn't have the WebAssembly
    // backend. wasi_paths_for_target() locates the brew-installed
    // toolchain at runtime so the developer doesn't have to set
    // env vars (the prefix lookup is one process spawn at compile
    // time, dominated by LLVM's optimize pass anyway).
    let (cc_cmd, cc_target_args, cc_opt_arg, link_cmd_name): (&str, Vec<String>, &str, &str) =
        match target {
            CompileTarget::Native => ("cc", Vec::new(), "-O3", "cc"),
            CompileTarget::Wasm32Wasi => {
                let (clang_path, sysroot) = wasi_paths_for_target()?;
                (
                    Box::leak(clang_path.into_boxed_str()),
                    vec![
                        "--target=wasm32-wasip1".into(),
                        format!("--sysroot={sysroot}"),
                    ],
                    "-O2", // wasm-ld + LTO + O3 hits a verifier issue in
                    // LLVM 22; O2 is the documented stable level
                    // for the wasm backend (matches Emscripten's
                    // default).
                    "wasm-ld",
                )
            }
        };
    // -flto lets the linker inline cross-TU calls between the
    // LLVM-emitted object and the C runtime.
    for (idx, (filename, _)) in torajs_runtime::SOURCES.iter().enumerate() {
        let mut cmd = Command::new(cc_cmd);
        cmd.args(["-c"]).arg(cc_opt_arg);
        for ta in &cc_target_args {
            cmd.arg(ta);
        }
        if matches!(target, CompileTarget::Native) {
            cmd.arg("-flto").arg("-g");
        }
        let status = cmd
            .arg("-o")
            .arg(&o_paths[idx])
            .arg(&c_paths[idx])
            .status()
            .map_err(|e| CompileError::Link(format!("spawning cc -c ({filename}): {e}")))?;
        if !status.success() {
            for p in &c_paths {
                let _ = std::fs::remove_file(p);
            }
            for p in o_paths.iter().take(idx) {
                let _ = std::fs::remove_file(p);
            }
            return Err(CompileError::Link(format!(
                "cc -c {filename} exited {status}"
            )));
        }
    }

    // v0.3 #4 D-2 — `-g` keeps DWARF live through the link stage.
    // On macOS the linker writes a separate `.dSYM` bundle alongside
    // the binary by default; D-4 will pick the right resolver path
    // for `atos` symbolication. Cost is link-time only — runtime
    // perf unaffected.
    //
    // T-20 (v0.6.0) — for wasm32-wasi, link via wasm-ld with the
    // wasi-libc sysroot. The wasi-sdk's libc.a + libwasi-emulated-
    // mman + crt1-command.o provide the wasi syscall ABI; without
    // these wasm-ld can't resolve printf / malloc / fopen / etc.
    let mut link_cmd = Command::new(link_cmd_name);
    match target {
        CompileTarget::Native => {
            link_cmd.arg("-flto").arg("-g").arg(&obj_path);
            for op in &o_paths {
                link_cmd.arg(op);
            }
            /* T-21 (v0.6.0) — runtime_fetch.c uses libcurl for the
             * sync HTTP fetch. Only link libcurl when the user
             * program actually references `fetch(...)`; otherwise
             * dyld would still load libcurl + its TLS deps at
             * process start, regressing every short-running case
             * by ~0.7ms (fifo-queue-100k / stack-pop-1m / startup).
             *
             * Detection: scan the SSA module for any Call whose
             * callee is the fetch_sync intrinsic (declared by
             * ssa_lower only when the program contains a `fetch`
             * call site). Keep this conditional sharp — adding
             * libcurl for a feature the program doesn't use is
             * dead weight. */
            if module_uses_fetch(ssa_module) {
                link_cmd.arg("-lcurl");
            }
            // V3-16 — shared-lib output: cc's `-shared` flag asks
            // ld for a position-independent dylib (no main, no
            // crt1). On macOS this becomes `-dynamiclib` under the
            // hood; cc handles the per-platform translation.
            // `-fPIC` makes every per-TU object position-
            // independent so the loader can map at any address.
            // `-undefined dynamic_lookup` defers symbol resolution
            // for runtime intrinsics (`__torajs_str_alloc`, etc)
            // to the host process — when the dylib is loaded into
            // a tora-emitted binary, the host already has those
            // symbols and the loader binds them.
            if matches!(kind, OutputKind::SharedLib) {
                link_cmd.arg("-shared").arg("-fPIC");
                #[cfg(target_os = "macos")]
                link_cmd.arg("-Wl,-undefined,dynamic_lookup");
            }
            link_cmd.arg("-o").arg(out_path);
        }
        CompileTarget::Wasm32Wasi => {
            let (_clang_path, sysroot) = wasi_paths_for_target()?;
            // wasm-ld doesn't pull libc on its own; pass the wasi-
            // sysroot lib directories explicitly + the crt entry
            // object so `_start` lands at module init.
            link_cmd.arg(format!("{sysroot}/lib/wasm32-wasip1/crt1-command.o"));
            link_cmd.arg(&obj_path);
            for op in &o_paths {
                link_cmd.arg(op);
            }
            link_cmd
                .arg(format!("-L{sysroot}/lib/wasm32-wasip1"))
                .arg("-lc")
                .arg("--no-entry") // crt1-command.o supplies _start
                .arg("--export=_start")
                .arg("-o")
                .arg(out_path);
        }
    }
    let status = link_cmd
        .status()
        .map_err(|e| CompileError::Link(format!("spawning {link_cmd_name}: {e}")))?;
    // v0.3 #4 D-2 — macOS: consolidate DWARF from per-TU .o files
    // into a `.dSYM` bundle alongside the binary. atos / lldb find
    // it automatically by name. Without this, the .o files we're
    // about to delete take their DWARF with them and backtraces
    // can't resolve to source. linux embeds DWARF directly in the
    // binary so no consolidation step is needed.
    #[cfg(target_os = "macos")]
    if source_path.is_some() && matches!(target, CompileTarget::Native) {
        // Silence dsymutil's `warning: (arm64) /tmp/lto.o unable to
        // open object file` — that's the LTO temp .o which the
        // linker has already deleted by the time dsymutil runs;
        // benign but pollutes stderr's first line and breaks
        // test262's classifier (it reads the leading line to
        // decide incompat vs bug).
        let _ = Command::new("dsymutil")
            .arg(out_path)
            .stderr(std::process::Stdio::null())
            .status();
    }
    let _ = std::fs::remove_file(&obj_path);
    for p in &c_paths {
        let _ = std::fs::remove_file(p);
    }
    for p in &o_paths {
        let _ = std::fs::remove_file(p);
    }
    if !status.success() {
        return Err(CompileError::Link(format!("cc exited {status}")));
    }
    Ok(())
}

/// T-20 (v0.6.0) — locate the LLVM 22 clang + wasi-libc sysroot
/// installed by Homebrew. Both are required to compile + link
/// wasm32-wasip1 binaries; macOS's system clang doesn't have the
/// WebAssembly backend and there's no canonical wasi sysroot path.
/// `brew --prefix <pkg>` is one process spawn at compile time —
/// dominated by LLVM's optimize pass which runs unconditionally.
fn wasi_paths_for_target() -> Result<(String, String), CompileError> {
    fn brew_prefix(pkg: &str) -> Result<String, CompileError> {
        let out = Command::new("brew")
            .args(["--prefix", pkg])
            .output()
            .map_err(|e| {
                CompileError::Link(format!(
                    "wasm32-wasi target needs `brew --prefix {pkg}`: {e} \
                     (install via `brew install {pkg}`)"
                ))
            })?;
        if !out.status.success() {
            return Err(CompileError::Link(format!(
                "brew --prefix {pkg} exited {} — install via `brew install {pkg}`",
                out.status
            )));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }
    let llvm_prefix = brew_prefix("llvm@22")?;
    let wasi_prefix = brew_prefix("wasi-libc")?;
    let clang_path = format!("{llvm_prefix}/bin/clang");
    let sysroot = format!("{wasi_prefix}/share/wasi-sysroot");
    Ok((clang_path, sysroot))
}

fn declare_putchar<'ctx>(ctx: &'ctx Context, m: &LlvmModule<'ctx>) -> FunctionValue<'ctx> {
    let i32_t = ctx.i32_type();
    let fn_t = i32_t.fn_type(&[i32_t.into()], false);
    m.add_function("putchar", fn_t, None)
}

/// T-20.b (v0.6.0) — pick the libc fn name based on target. On
/// native, IR calls libc directly with `i64` size args (matches
/// the platform's 64-bit size_t). On wasm32-wasi, libc has 32-bit
/// size_t and wasm makes function-type identity part of the type
/// system; routing through the `__torajs_libc_*` bridge in
/// `runtime_libc_bridge.c` keeps the IR-side i64 ABI uniform
/// while the C bridge does the (size_t)i64 truncation.
fn libc_name(native: &'static str, target: CompileTarget) -> &'static str {
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

fn declare_malloc<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    target: CompileTarget,
) -> FunctionValue<'ctx> {
    let i64_t = ctx.i64_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let fn_t = ptr_t.fn_type(&[i64_t.into()], false);
    m.add_function(libc_name("malloc", target), fn_t, None)
}

fn declare_realloc<'ctx>(
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

fn declare_memcpy<'ctx>(
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

fn declare_memmove<'ctx>(
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

fn declare_free<'ctx>(
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
/// runtime_str.c. Pushes short-string blocks onto a thread-local LIFO
/// for reuse by the next short-Str alloc; falls back to libc free for
/// blocks too large to pool.
fn declare_str_free<'ctx>(ctx: &'ctx Context, m: &LlvmModule<'ctx>) -> FunctionValue<'ctx> {
    let void_t = ctx.void_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let fn_t = void_t.fn_type(&[ptr_t.into()], false);
    m.add_function("__torajs_str_free", fn_t, None)
}

/// `__torajs_arr_free(void *p)` — pool-aware arr free. Defined in
/// runtime_str.c. Routes split-block allocations (flagged in the
/// universal header) to a thread-local cache indexed by `cap` so
/// tight `s.split(sep)` loops recycle the exact same block every
/// iter instead of mallocing per call.
fn declare_arr_free<'ctx>(ctx: &'ctx Context, m: &LlvmModule<'ctx>) -> FunctionValue<'ctx> {
    let void_t = ctx.void_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let fn_t = void_t.fn_type(&[ptr_t.into()], false);
    m.add_function("__torajs_arr_free", fn_t, None)
}

/// `__torajs_str_alloc_pooled(uint64_t len) -> uint8_t*` — pool-aware
/// Str alloc. Pops a recently-freed short-Str block when one fits;
/// otherwise calls malloc + initializes the header. Defined in
/// runtime_str.c. Inkwell's str_alloc IR fn delegates here so the
/// LLVM-emitted hot path picks up the pool too.
fn declare_str_alloc_pooled<'ctx>(ctx: &'ctx Context, m: &LlvmModule<'ctx>) -> FunctionValue<'ctx> {
    let i64_t = ctx.i64_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let fn_t = ptr_t.fn_type(&[i64_t.into()], false);
    m.add_function("__torajs_str_alloc_pooled", fn_t, None)
}

/// `__torajs_split_iter_next(*iter, *out_substr) -> bool` — defined
/// fully in inkwell IR (instead of as a `cc`-compiled C function)
/// so LLVM can inline the body across the call boundary at -O3.
/// Verified by disassembly: post-this-change `evalRpn`'s inner loop
/// no longer issues a `bl` to split_iter_next; the byte scan and
/// substr emit are spliced directly into the caller's iter loop.
///
/// The C-side `__torajs_split_iter_next` body in runtime_str.c is
/// removed when this is wired up — keeping both definitions would
/// produce a duplicate-symbol linker error. SplitIter struct layout
/// (parent +0, parent_len +8, sep_data +16, sep_len +24, pos +32,
/// exhausted +40) and emit_substr layout (header +0, len +8, parent
/// +16, offset +24) match the C struct + helper exactly so init /
/// drop (still C-side) interop seamlessly.
fn define_split_iter_next<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    target: CompileTarget,
) -> FunctionValue<'ctx> {
    let builder = ctx.create_builder();
    let i64_t = ctx.i64_type();
    let i32_t = ctx.i32_type();
    let i8_t = ctx.i8_type();
    let i16_t = ctx.i16_type();
    let bool_t = ctx.bool_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let fn_t = bool_t.fn_type(&[ptr_t.into(), ptr_t.into()], false);
    let f = m.add_function("__torajs_split_iter_next", fn_t, None);

    let entry = ctx.append_basic_block(f, "entry");
    let load_state = ctx.append_basic_block(f, "load_state");
    let empty_sep_blk = ctx.append_basic_block(f, "empty_sep");
    let empty_emit = ctx.append_basic_block(f, "empty_emit");
    let single_sep_blk = ctx.append_basic_block(f, "single_sep");
    let scan_loop = ctx.append_basic_block(f, "scan_loop");
    let scan_step = ctx.append_basic_block(f, "scan_step");
    let scan_done = ctx.append_basic_block(f, "scan_done");
    let multi_sep_blk = ctx.append_basic_block(f, "multi_sep");
    let multi_loop = ctx.append_basic_block(f, "multi_loop");
    let multi_check_match = ctx.append_basic_block(f, "multi_check");
    let multi_step = ctx.append_basic_block(f, "multi_step");
    let multi_done = ctx.append_basic_block(f, "multi_done");
    let emit_blk = ctx.append_basic_block(f, "emit");
    let advance_pos_blk = ctx.append_basic_block(f, "advance_pos");
    let mark_exhausted_blk = ctx.append_basic_block(f, "mark_exhausted");
    // empty_sep's "no more chars" early-exit: marks exhausted AND
    // returns false (didn't yield). Distinct from mark_exhausted_blk
    // which returns true (yielded then ran out).
    let exhaust_and_false_blk = ctx.append_basic_block(f, "exhaust_and_false");
    let return_true = ctx.append_basic_block(f, "ret_true");
    let return_false = ctx.append_basic_block(f, "ret_false");

    builder.position_at_end(entry);
    let iter = f.get_nth_param(0).unwrap().into_pointer_value();
    let out = f.get_nth_param(1).unwrap().into_pointer_value();

    let gep = |b: &inkwell::builder::Builder<'ctx>,
               base: inkwell::values::PointerValue<'ctx>,
               off: u64,
               name: &str|
     -> inkwell::values::PointerValue<'ctx> {
        unsafe {
            b.build_in_bounds_gep(i8_t, base, &[i64_t.const_int(off, false)], name)
                .unwrap()
        }
    };

    // exhausted byte at iter+40
    let exh_p = gep(&builder, iter, 40, "exh_p");
    let exh = builder
        .build_load(i8_t, exh_p, "exh")
        .unwrap()
        .into_int_value();
    let is_exh = builder
        .build_int_compare(IntPredicate::NE, exh, i8_t.const_int(0, false), "is_exh")
        .unwrap();
    builder
        .build_conditional_branch(is_exh, return_false, load_state)
        .unwrap();

    // load_state: read parent / parent_len / sep_data / sep_len / pos.
    builder.position_at_end(load_state);
    let parent = builder
        .build_load(ptr_t, iter, "parent")
        .unwrap()
        .into_pointer_value();
    let parent_len_p = gep(&builder, iter, 8, "plen_p");
    let parent_len = builder
        .build_load(i64_t, parent_len_p, "plen")
        .unwrap()
        .into_int_value();
    let sep_data_p = gep(&builder, iter, 16, "sd_p");
    let sep_data = builder
        .build_load(ptr_t, sep_data_p, "sd")
        .unwrap()
        .into_pointer_value();
    let sep_len_p = gep(&builder, iter, 24, "sl_p");
    let sep_len = builder
        .build_load(i64_t, sep_len_p, "sl")
        .unwrap()
        .into_int_value();
    let pos_p = gep(&builder, iter, 32, "pos_p");
    let pos = builder
        .build_load(i64_t, pos_p, "pos")
        .unwrap()
        .into_int_value();
    // parent bytes start at parent + STR_HDR_DATA_OFF (= 16).
    let parent_bytes = gep(&builder, parent, 16, "pbytes");

    // Branch on sep_len: 0 → empty_sep, 1 → single_sep, else multi_sep.
    let sl_zero = builder
        .build_int_compare(IntPredicate::EQ, sep_len, i64_t.const_int(0, false), "sl_z")
        .unwrap();
    let single_or_multi = ctx.append_basic_block(f, "single_or_multi");
    builder
        .build_conditional_branch(sl_zero, empty_sep_blk, single_or_multi)
        .unwrap();

    builder.position_at_end(single_or_multi);
    let sl_one = builder
        .build_int_compare(
            IntPredicate::EQ,
            sep_len,
            i64_t.const_int(1, false),
            "sl_one",
        )
        .unwrap();
    builder
        .build_conditional_branch(sl_one, single_sep_blk, multi_sep_blk)
        .unwrap();

    // empty_sep: if pos >= parent_len → exhaust+ret 0; else emit single
    // char view and advance pos.
    builder.position_at_end(empty_sep_blk);
    let pos_ge_plen = builder
        .build_int_compare(IntPredicate::UGE, pos, parent_len, "pos_ge_plen")
        .unwrap();
    builder
        .build_conditional_branch(pos_ge_plen, exhaust_and_false_blk, empty_emit)
        .unwrap();
    builder.position_at_end(empty_emit);
    // empty_sep emits len=1; the next pos = pos+1 (computed here so
    // it's defined in this predecessor of emit_blk for the phi).
    let pos_p1_for_empty = builder
        .build_int_add(pos, i64_t.const_int(1, false), "pos_p1")
        .unwrap();
    builder.build_unconditional_branch(emit_blk).unwrap();

    // single_sep: scan from pos for first occurrence of sep_data[0].
    builder.position_at_end(single_sep_blk);
    let b = builder
        .build_load(i8_t, sep_data, "b")
        .unwrap()
        .into_int_value();
    builder.build_unconditional_branch(scan_loop).unwrap();
    // scan_loop: phi k starting at pos; if k >= plen → scan_done with k=plen
    builder.position_at_end(scan_loop);
    let k_phi = builder.build_phi(i64_t, "k").unwrap();
    k_phi.add_incoming(&[(&pos, single_sep_blk)]);
    let k_val = k_phi.as_basic_value().into_int_value();
    let k_ge_plen = builder
        .build_int_compare(IntPredicate::UGE, k_val, parent_len, "k_ge")
        .unwrap();
    let scan_check_byte = ctx.append_basic_block(f, "scan_check");
    builder
        .build_conditional_branch(k_ge_plen, scan_done, scan_check_byte)
        .unwrap();
    builder.position_at_end(scan_check_byte);
    let byte_ptr = unsafe {
        builder
            .build_in_bounds_gep(i8_t, parent_bytes, &[k_val], "bp")
            .unwrap()
    };
    let byte_val = builder
        .build_load(i8_t, byte_ptr, "by")
        .unwrap()
        .into_int_value();
    let byte_eq = builder
        .build_int_compare(IntPredicate::EQ, byte_val, b, "by_eq")
        .unwrap();
    builder
        .build_conditional_branch(byte_eq, scan_done, scan_step)
        .unwrap();
    builder.position_at_end(scan_step);
    let k_next = builder
        .build_int_add(k_val, i64_t.const_int(1, false), "k_n")
        .unwrap();
    k_phi.add_incoming(&[(&k_next, scan_step)]);
    builder.build_unconditional_branch(scan_loop).unwrap();
    builder.position_at_end(scan_done);
    let len_single = builder.build_int_sub(k_val, pos, "len_single").unwrap();
    builder.build_unconditional_branch(emit_blk).unwrap();

    // multi_sep: scan with memcmp at each candidate position.
    builder.position_at_end(multi_sep_blk);
    builder.build_unconditional_branch(multi_loop).unwrap();
    builder.position_at_end(multi_loop);
    let mk_phi = builder.build_phi(i64_t, "mk").unwrap();
    mk_phi.add_incoming(&[(&pos, multi_sep_blk)]);
    let mk_val = mk_phi.as_basic_value().into_int_value();
    // if mk + sep_len > parent_len → done with k = parent_len
    let mk_plus_sl = builder.build_int_add(mk_val, sep_len, "mk_sl").unwrap();
    let mk_oob = builder
        .build_int_compare(IntPredicate::UGT, mk_plus_sl, parent_len, "mk_oob")
        .unwrap();
    let multi_oob = ctx.append_basic_block(f, "multi_oob");
    builder
        .build_conditional_branch(mk_oob, multi_oob, multi_check_match)
        .unwrap();
    builder.position_at_end(multi_oob);
    builder.build_unconditional_branch(multi_done).unwrap();
    builder.position_at_end(multi_check_match);
    // memcmp(parent_bytes + mk, sep_data, sep_len)
    let cand_ptr = unsafe {
        builder
            .build_in_bounds_gep(i8_t, parent_bytes, &[mk_val], "cand")
            .unwrap()
    };
    // T-20.b — `m.get_function` must use the same target-resolved
    // name we declared with above. On wasm32-wasi the bridge
    // intercepts `memcmp` → `__torajs_libc_memcmp`.
    let memcmp_fn = m
        .get_function(libc_name("memcmp", target))
        .expect("memcmp declared");
    let cmp = builder
        .build_call(
            memcmp_fn,
            &[cand_ptr.into(), sep_data.into(), sep_len.into()],
            "cmp",
        )
        .unwrap()
        .try_as_basic_value()
        .unwrap_basic()
        .into_int_value();
    let cmp_eq = builder
        .build_int_compare(IntPredicate::EQ, cmp, i32_t.const_int(0, false), "cmp_eq")
        .unwrap();
    builder
        .build_conditional_branch(cmp_eq, multi_done, multi_step)
        .unwrap();
    builder.position_at_end(multi_step);
    let mk_n = builder
        .build_int_add(mk_val, i64_t.const_int(1, false), "mk_n")
        .unwrap();
    mk_phi.add_incoming(&[(&mk_n, multi_step)]);
    builder.build_unconditional_branch(multi_loop).unwrap();
    builder.position_at_end(multi_done);
    // k = (mk_oob ? parent_len : mk)
    let k_multi_phi = builder.build_phi(i64_t, "k_multi").unwrap();
    k_multi_phi.add_incoming(&[(&parent_len, multi_oob), (&mk_val, multi_check_match)]);
    let k_multi = k_multi_phi.as_basic_value().into_int_value();
    let len_multi = builder.build_int_sub(k_multi, pos, "len_multi").unwrap();
    builder.build_unconditional_branch(emit_blk).unwrap();

    // emit_blk — phi over (which path, k value, len value, advance_kind).
    // Sources:
    //   empty_emit  → k = pos+1 unused; emit len=1, set new_pos=pos+1
    //   scan_done   → k = k_val; emit len=k-pos; advance to (k+1) if k<plen else exhaust
    //   multi_done  → k = k_multi; emit len=k-pos; advance to (k+sep_len) if k<plen else exhaust
    builder.position_at_end(emit_blk);
    let k_phi_emit = builder.build_phi(i64_t, "k_emit").unwrap();
    let len_phi_emit = builder.build_phi(i64_t, "len_emit").unwrap();
    let stride_phi_emit = builder.build_phi(i64_t, "stride_emit").unwrap();
    // Phi MUST come before any non-phi instruction in this block.
    let is_empty_phi = builder.build_phi(bool_t, "is_empty").unwrap();
    is_empty_phi.add_incoming(&[
        (&bool_t.const_int(1, false), empty_emit),
        (&bool_t.const_int(0, false), scan_done),
        (&bool_t.const_int(0, false), multi_done),
    ]);
    // empty_emit: k = pos+1 (defined in empty_emit), len = 1,
    // stride = 0 (next pos = k+0 = pos+1).
    k_phi_emit.add_incoming(&[(&pos_p1_for_empty, empty_emit)]);
    len_phi_emit.add_incoming(&[(&i64_t.const_int(1, false), empty_emit)]);
    stride_phi_emit.add_incoming(&[(&i64_t.const_int(0, false), empty_emit)]);
    // scan_done: k = k_val, len = k - pos (computed in scan_done),
    // stride = 1 (single-byte sep)
    k_phi_emit.add_incoming(&[(&k_val, scan_done)]);
    len_phi_emit.add_incoming(&[(&len_single, scan_done)]);
    stride_phi_emit.add_incoming(&[(&i64_t.const_int(1, false), scan_done)]);
    // multi_done: k = k_multi, len = k_multi - pos (computed in
    // multi_done), stride = sep_len
    k_phi_emit.add_incoming(&[(&k_multi, multi_done)]);
    len_phi_emit.add_incoming(&[(&len_multi, multi_done)]);
    stride_phi_emit.add_incoming(&[(&sep_len, multi_done)]);

    let k_final = k_phi_emit.as_basic_value().into_int_value();
    let len_final = len_phi_emit.as_basic_value().into_int_value();
    let stride_final = stride_phi_emit.as_basic_value().into_int_value();

    // Write substr at out: header u64 (STATIC_LITERAL=4 in flags
    // bits 48..64), len, parent, offset=pos.
    let header_u64 = i64_t.const_int((STATIC_LITERAL_FLAG as u64) << 48, false);
    builder.build_store(out, header_u64).unwrap();
    let out_len_p = gep(&builder, out, 8, "ol_p");
    builder.build_store(out_len_p, len_final).unwrap();
    let out_parent_p = gep(&builder, out, 16, "op_p");
    builder.build_store(out_parent_p, parent).unwrap();
    let out_off_p = gep(&builder, out, 24, "oo_p");
    builder.build_store(out_off_p, pos).unwrap();

    // Decide advance: if k_final == parent_len → exhaust; else pos = k + stride.
    // For empty_sep path (stride=1, k=pos+1): if pos+1 == plen, exhaust on next call;
    // we already set pos = pos+1 below in advance_pos_blk. The exhaust path is
    // reserved for "no more sep found" cases.
    let k_eq_plen = builder
        .build_int_compare(IntPredicate::EQ, k_final, parent_len, "k_eq_plen")
        .unwrap();
    // For empty_sep we always advance (caller will hit exhausted check next time).
    // Distinguish via a phi-tracked flag would add complexity; instead, use
    // (k_eq_plen) AND (stride != 1 OR k > pos+1)... simpler heuristic:
    // empty_sep emits len=1, so len_final==1 AND stride==1 AND parent_len > 0.
    // Conservative: only mark exhausted when len_final != 1 && k_eq_plen, OR
    // when stride > 1 && k_eq_plen. Both single-byte and multi-byte "no more
    // sep" cases produce k == parent_len; empty-sep always produces k = pos+1
    // which equals parent_len iff pos+1 == parent_len, which is the natural
    // last char — caller will see exhausted on the *next* call via the
    // pos>=parent_len check at the empty_sep entry, so we only need to advance
    // pos here, never set exhausted from the empty_sep path.
    //
    // Use len_final as discriminator: empty_sep is the only path with
    // len=1 AND stride=1 simultaneously (single-byte sep produces stride=1
    // but len = k - pos which is only 1 when there are no leading non-sep
    // bytes). Distinguish via separate phi tracking would be cleaner —
    // add an `is_empty_sep` bool phi.
    let is_empty = is_empty_phi.as_basic_value().into_int_value();
    let exhaust_now = builder
        .build_and(
            k_eq_plen,
            builder.build_not(is_empty, "not_empty").unwrap(),
            "exhaust_now",
        )
        .unwrap();
    builder
        .build_conditional_branch(exhaust_now, mark_exhausted_blk, advance_pos_blk)
        .unwrap();

    builder.position_at_end(advance_pos_blk);
    let new_pos = builder
        .build_int_add(k_final, stride_final, "new_pos")
        .unwrap();
    builder.build_store(pos_p, new_pos).unwrap();
    builder.build_unconditional_branch(return_true).unwrap();

    builder.position_at_end(mark_exhausted_blk);
    builder
        .build_store(exh_p, i8_t.const_int(1, false))
        .unwrap();
    builder.build_unconditional_branch(return_true).unwrap();

    builder.position_at_end(exhaust_and_false_blk);
    builder
        .build_store(exh_p, i8_t.const_int(1, false))
        .unwrap();
    builder.build_unconditional_branch(return_false).unwrap();

    builder.position_at_end(return_true);
    builder
        .build_return(Some(&bool_t.const_int(1, false)))
        .unwrap();
    builder.position_at_end(return_false);
    builder
        .build_return(Some(&bool_t.const_int(0, false)))
        .unwrap();

    let _ = i16_t; // suppress unused warning
    f
}

/// `__torajs_arr_alloc_pooled(uint64_t cap) -> void*` — pool-aware
/// Array alloc. For cap ≤ POOL_CAP_MAX, scans the cap-indexed LIFO
/// for a matching block; falls through to malloc + header init on
/// miss. Defined in runtime_str.c. Inkwell's arr_alloc IR fn
/// delegates here so fn-local literal allocs (`let xs = [a, b, c]`
/// inside a tight loop) reuse the same block per iter instead of
/// mallocing.
fn declare_arr_alloc_pooled<'ctx>(ctx: &'ctx Context, m: &LlvmModule<'ctx>) -> FunctionValue<'ctx> {
    let i64_t = ctx.i64_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let fn_t = ptr_t.fn_type(&[i64_t.into()], false);
    m.add_function("__torajs_arr_alloc_pooled", fn_t, None)
}

fn declare_memcmp<'ctx>(
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

/// Phase B refcount: every Str heap object begins with the universal
/// 8-byte heap header `__torajs_heap_header_t` (refcount@0, type_tag@4,
/// flags@6), followed by `len@8`, then `bytes@16`. The values below are
/// the offsets used by every Str-producing inkwell IR site, kept in
/// lock-step with `__TORAJS_STR_HDR_SIZE` and the macros defined in
/// `runtime_str.c`. If you change one, change the other.
const STR_HDR_REFCOUNT_OFF: u64 = 0;
const STR_HDR_TYPE_TAG_OFF: u64 = 4;
const STR_HDR_FLAGS_OFF: u64 = 6;
const STR_HDR_LEN_OFF: u64 = 8;
const STR_HDR_DATA_OFF: u64 = 16;
const STR_HDR_TAG_STR: u64 = 0;

/// Emit the inkwell IR sequence that materializes a fresh Str heap
/// object: malloc(16 + len), init refcount=1 / type_tag=STR / flags=0 /
/// len, return the pointer (data area uninitialized — caller fills the
/// `len` bytes at `p + STR_HDR_DATA_OFF`).
///
/// Single-source-of-truth for every str-producing inkwell function so
/// the header init never drifts between sites.
fn emit_str_alloc_header<'ctx>(
    _ctx: &'ctx Context,
    builder: &inkwell::builder::Builder<'ctx>,
    str_alloc_pooled: FunctionValue<'ctx>,
    len: inkwell::values::IntValue<'ctx>,
) -> inkwell::values::PointerValue<'ctx> {
    // Header init + pool fast-path lives in __torajs_str_alloc_pooled
    // (runtime_str.c). LTO inlines it into the caller, so this stays
    // a single call from the IR's perspective but expands to the
    // pool-pop fast path / malloc + 2-store header init.
    builder
        .build_call(str_alloc_pooled, &[len.into()], "str_p")
        .unwrap()
        .try_as_basic_value()
        .unwrap_basic()
        .into_pointer_value()
}

#[allow(dead_code)]
fn emit_str_alloc_header_unused_legacy<'ctx>(
    ctx: &'ctx Context,
    builder: &inkwell::builder::Builder<'ctx>,
    malloc: FunctionValue<'ctx>,
    len: inkwell::values::IntValue<'ctx>,
) -> inkwell::values::PointerValue<'ctx> {
    let i64_t = ctx.i64_type();
    let i32_t = ctx.i32_type();
    let i16_t = ctx.i16_type();
    let i8_t = ctx.i8_type();

    let total = builder
        .build_int_add(len, i64_t.const_int(STR_HDR_DATA_OFF, false), "str_total")
        .unwrap();
    let p = builder
        .build_call(malloc, &[total.into()], "str_p")
        .unwrap()
        .try_as_basic_value()
        .unwrap_basic()
        .into_pointer_value();

    // refcount = 1 (already at offset 0; no GEP needed)
    builder.build_store(p, i32_t.const_int(1, false)).unwrap();
    // type_tag @ +4
    let tag_ptr = unsafe {
        builder
            .build_in_bounds_gep(
                i8_t,
                p,
                &[i64_t.const_int(STR_HDR_TYPE_TAG_OFF, false)],
                "str_tag",
            )
            .unwrap()
    };
    builder
        .build_store(tag_ptr, i16_t.const_int(STR_HDR_TAG_STR, false))
        .unwrap();
    // flags @ +6
    let flags_ptr = unsafe {
        builder
            .build_in_bounds_gep(
                i8_t,
                p,
                &[i64_t.const_int(STR_HDR_FLAGS_OFF, false)],
                "str_flags",
            )
            .unwrap()
    };
    builder
        .build_store(flags_ptr, i16_t.const_int(0, false))
        .unwrap();
    // len @ +8
    let len_ptr = unsafe {
        builder
            .build_in_bounds_gep(
                i8_t,
                p,
                &[i64_t.const_int(STR_HDR_LEN_OFF, false)],
                "str_len_ptr",
            )
            .unwrap()
    };
    builder.build_store(len_ptr, len).unwrap();
    p
}

/// Get the Str's data byte pointer (`p + STR_HDR_DATA_OFF`). Used by
/// every site that reads or writes string bytes after the header.
fn str_data_ptr<'ctx>(
    ctx: &'ctx Context,
    builder: &inkwell::builder::Builder<'ctx>,
    str_ptr: inkwell::values::PointerValue<'ctx>,
    name: &str,
) -> inkwell::values::PointerValue<'ctx> {
    let i64_t = ctx.i64_type();
    let i8_t = ctx.i8_type();
    unsafe {
        builder
            .build_in_bounds_gep(
                i8_t,
                str_ptr,
                &[i64_t.const_int(STR_HDR_DATA_OFF, false)],
                name,
            )
            .unwrap()
    }
}

/// Load the Str's `len` field (`*(u64*)(p + STR_HDR_LEN_OFF)`).
fn str_len_load<'ctx>(
    ctx: &'ctx Context,
    builder: &inkwell::builder::Builder<'ctx>,
    str_ptr: inkwell::values::PointerValue<'ctx>,
    name: &str,
) -> inkwell::values::IntValue<'ctx> {
    let i64_t = ctx.i64_type();
    let i8_t = ctx.i8_type();
    let len_ptr = unsafe {
        builder
            .build_in_bounds_gep(
                i8_t,
                str_ptr,
                &[i64_t.const_int(STR_HDR_LEN_OFF, false)],
                &format!("{name}_lp"),
            )
            .unwrap()
    };
    builder
        .build_load(i64_t, len_ptr, name)
        .unwrap()
        .into_int_value()
}

/// Mark a function as `alwaysinline` — LLVM forces inlining at every
/// call site regardless of cost model. Used for hot, small intrinsics
/// (e.g. `__torajs_str_char_code_at`) where the per-call C-function-
/// boundary cost dwarfs the body. Must be called AFTER `add_function`
/// and BEFORE the body lowers; doesn't change function semantics.
fn mark_alwaysinline<'ctx>(ctx: &'ctx Context, f: FunctionValue<'ctx>) {
    let kind = Attribute::get_named_enum_kind_id("alwaysinline");
    let attr = ctx.create_enum_attribute(kind, 0);
    f.add_attribute(AttributeLoc::Function, attr);
}

/// T-24-prep (v0.6+1) — mark a function as `memory(none)` so LLVM's
/// LICM / GVN can hoist invariant loads through call sites. Applied
/// to user FnDecls whose SSA body is provably pure: no Store /
/// StoreDyn / Call / CallIndirect anywhere. The dominant win is
/// `id<T>(x: T): T { return x }`-shape generic helpers in tight
/// loops (generic-id-1m: `xs.length` reload through the call site
/// disappears once LLVM knows the call has zero memory effect).
///
/// Conservative on the false-negative side — Load/LoadDyn alone
/// would qualify for `memory(read)`, but that's harder to apply
/// safely (caller's stack alloca writes vs callee's heap reads
/// need explicit alias info LLVM can't infer cheaply); ship the
/// strict-none variant first, expand to read-only later if a
/// bench case proves the gap.
fn mark_memory_none<'ctx>(ctx: &'ctx Context, f: FunctionValue<'ctx>) {
    /* LLVM 22's memory effect attribute encodes (location, mod-ref)
     * pairs into a u64. memory(none) is the all-zero bitmask. */
    let kind = Attribute::get_named_enum_kind_id("memory");
    let attr = ctx.create_enum_attribute(kind, 0);
    f.add_attribute(AttributeLoc::Function, attr);
}

/// Walk a SSA Function's blocks + insts and return true iff the body
/// performs zero memory mutation AND zero unknown-effect calls.
/// Pure as defined here:
///   - no Store / StoreDyn (never writes memory observable to caller)
///   - no Call (we conservatively treat all callees as having effects;
///     refining this to "transitive purity" is a follow-up)
///   - no CallIndirect (function-pointer call → can be anything)
///   - no Alloca / AllocaBytes (these allocate stack but the caller
///     doesn't observe; technically pure but LLVM may still see the
///     `mem(none)` lie — safer to treat as "has memory effect" in
///     this conservative sweep).
///
/// Loads are fine — readonly memory access doesn't break memory(none)
/// in the strict sense for return values (LLVM treats memory(none) as
/// "no read AND no write"; a fn with Load wouldn't qualify here).
/// We err on the strict side: only fns with literally zero memory
/// inst kinds get tagged.
/// T-21 link-time gate. Walk every fn's instructions; return true iff
/// any Call targets a function named `__torajs_fetch_sync`. The
/// intrinsic is only declared (and only ever called) when ssa_lower
/// has lowered a `fetch(url)` site, so this doubles as "does the
/// program use fetch".
fn module_uses_fetch(module: &Module) -> bool {
    for f in &module.funcs {
        for blk in &f.blocks {
            for inst in &blk.insts {
                if let InstKind::Call(fid, _) = &inst.kind
                    && module.func_name(*fid) == "__torajs_fetch_sync"
                {
                    return true;
                }
            }
        }
    }
    false
}

fn ssa_fn_is_pure(f: &s::Function) -> bool {
    for blk in &f.blocks {
        for inst in &blk.insts {
            match &inst.kind {
                InstKind::Store(..)
                | InstKind::StoreDyn(..)
                | InstKind::Load(..)
                | InstKind::LoadDyn(..)
                | InstKind::Call(..)
                | InstKind::CallIndirect(..)
                | InstKind::Alloca(_)
                | InstKind::AllocaBytes(_) => return false,
                _ => {}
            }
        }
    }
    true
}

/// Tag a function as returning a fresh, non-aliasing pointer (libc
/// `malloc` semantics). Lets LLVM hoist invariant loads through
/// foreign writes — e.g. in rpn-eval-100k, `parts.length` (parts
/// from str_split) gets hoisted out of the inner loop because the
/// stack writes (stack from arr_alloc) provably can't alias it.
///
/// Apply only to allocators that genuinely return a fresh ptr each
/// call (str_alloc, arr_alloc, str_split, substr_create, ...).
/// `arr_push` / `arr_reserve` return the same ptr they got OR a
/// reallocated one — those are NOT noalias.
fn mark_noalias_ret<'ctx>(ctx: &'ctx Context, f: FunctionValue<'ctx>) {
    let kind = Attribute::get_named_enum_kind_id("noalias");
    let attr = ctx.create_enum_attribute(kind, 0);
    f.add_attribute(AttributeLoc::Return, attr);
}

/// Whitelist of intrinsics whose return is a fresh-from-alloc pointer
/// suitable for `noalias` tagging. The list is conservative — anything
/// that *might* return an existing pointer (arr_push / arr_reserve /
/// arr_unshift / arr_extend_unchecked) is excluded. Misuse here is
/// undefined behavior at the LLVM level (silent miscompile under
/// alias analysis), so additions need clear "always fresh" semantics.
fn is_alloc_intrinsic(name: &str) -> bool {
    matches!(
        name,
        // Str constructors
        "__torajs_str_alloc"
        | "__torajs_str_alloc_pooled"
        | "__torajs_str_concat"
        | "__torajs_str_slice"
        | "__torajs_str_substring"
        | "__torajs_str_repeat"
        | "__torajs_str_to_upper"
        | "__torajs_str_to_lower"
        | "__torajs_str_trim"
        | "__torajs_str_trim_start"
        | "__torajs_str_trim_end"
        | "__torajs_str_pad_start"
        | "__torajs_str_pad_end"
        | "__torajs_str_at"
        | "__torajs_str_from_char_code"
        | "__torajs_str_replace"
        | "__torajs_str_replace_all"
        | "__torajs_substr_to_owned"
        // Substr constructors
        | "__torajs_substr_create"
        | "__torajs_substr_slice"
        | "__torajs_substr_substring"
        | "__torajs_substr_trim"
        | "__torajs_substr_trim_start"
        | "__torajs_substr_trim_end"
        | "__torajs_substr_concat_substr_str"
        | "__torajs_substr_concat_str_substr"
        | "__torajs_substr_concat_substr_substr"
        // Array constructors that always return a fresh block
        | "__torajs_arr_alloc"
        | "__torajs_arr_alloc_pooled"
        | "__torajs_arr_slice"
        // Object / closure / regex / date constructors
        | "__torajs_obj_alloc"
        // String split returns a single fresh block (header + slots
        // + inline substr structs); does not alias its inputs.
        | "__torajs_str_split"
        | "__torajs_str_match_regex"
        | "__torajs_str_replace_regex"
        | "__torajs_str_replace_all_regex"
        | "__torajs_str_split_regex"
        | "__torajs_str_match_all_regex"
        | "__torajs_regex_compile"
        | "__torajs_regex_exec"
        | "__torajs_date_alloc_now"
        | "__torajs_date_alloc_ms"
        | "__torajs_date_alloc_iso"
        | "__torajs_date_alloc_components"
        | "__torajs_date_to_iso_string"
        | "__torajs_process_argv"
        | "__torajs_process_cwd"
        | "__torajs_process_platform"
        | "__torajs_process_getenv"
        | "__torajs_fs_read_file_sync"
    )
}

/// Phase 2A refcount: every Arr heap object begins with the same
/// 8-byte universal heap header `__torajs_heap_header_t` (refcount@0,
/// type_tag@4, flags@6), followed by `len@8` (u64), `cap@16` (u32),
/// `head@20` (u32), and `slots@24`. T-13.5 Array deque: cap was
/// shrunk from u64 to u32 to free 4 bytes for `head_offset`, the
/// physical-slot offset of logical[0]. `arr.shift()` is now O(1)
/// (head++/len--); push compacts when phys_used == cap and head>0.
/// Mirrors `__TORAJS_ARR_HDR_SIZE` and friends in `runtime_str.c`.
const ARR_HDR_LEN_OFF: u64 = 8;
const ARR_HDR_CAP_OFF: u64 = 16;
const ARR_HDR_HEAD_OFF: u64 = 20;
const ARR_HDR_DATA_OFF: u64 = 24;
const ARR_HDR_TAG_ARR: u64 = 2;

/// Emit `malloc(ARR_HDR_DATA_OFF + cap*8)` + universal-header init
/// (refcount=1 / type_tag=ARR / flags=0) + len/cap stores. Caller
/// fills slot data starting at `p + ARR_HDR_DATA_OFF`.
#[allow(dead_code)]
fn emit_arr_alloc_header<'ctx>(
    ctx: &'ctx Context,
    builder: &inkwell::builder::Builder<'ctx>,
    malloc: FunctionValue<'ctx>,
    len: inkwell::values::IntValue<'ctx>,
    cap: inkwell::values::IntValue<'ctx>,
) -> inkwell::values::PointerValue<'ctx> {
    let i64_t = ctx.i64_type();
    let i32_t = ctx.i32_type();
    let i16_t = ctx.i16_type();
    let i8_t = ctx.i8_type();

    let cap_bytes = builder
        .build_int_mul(cap, i64_t.const_int(8, false), "arr_cap_bytes")
        .unwrap();
    let total = builder
        .build_int_add(
            cap_bytes,
            i64_t.const_int(ARR_HDR_DATA_OFF, false),
            "arr_total",
        )
        .unwrap();
    let p = builder
        .build_call(malloc, &[total.into()], "arr_p")
        .unwrap()
        .try_as_basic_value()
        .unwrap_basic()
        .into_pointer_value();

    // refcount @ +0 = 1
    builder.build_store(p, i32_t.const_int(1, false)).unwrap();
    // type_tag @ +4 = TAG_ARR
    let tag_ptr = unsafe {
        builder
            .build_in_bounds_gep(
                i8_t,
                p,
                &[i64_t.const_int(STR_HDR_TYPE_TAG_OFF, false)],
                "arr_tag",
            )
            .unwrap()
    };
    builder
        .build_store(tag_ptr, i16_t.const_int(ARR_HDR_TAG_ARR, false))
        .unwrap();
    // flags @ +6 = 0
    let flags_ptr = unsafe {
        builder
            .build_in_bounds_gep(
                i8_t,
                p,
                &[i64_t.const_int(STR_HDR_FLAGS_OFF, false)],
                "arr_flags",
            )
            .unwrap()
    };
    builder
        .build_store(flags_ptr, i16_t.const_int(0, false))
        .unwrap();
    // len @ +8
    let len_ptr = unsafe {
        builder
            .build_in_bounds_gep(
                i8_t,
                p,
                &[i64_t.const_int(ARR_HDR_LEN_OFF, false)],
                "arr_len_p",
            )
            .unwrap()
    };
    builder.build_store(len_ptr, len).unwrap();
    // cap @ +16
    let cap_ptr = unsafe {
        builder
            .build_in_bounds_gep(
                i8_t,
                p,
                &[i64_t.const_int(ARR_HDR_CAP_OFF, false)],
                "arr_cap_p",
            )
            .unwrap()
    };
    builder.build_store(cap_ptr, cap).unwrap();
    p
}

/// Get the Arr's logical-data byte pointer (`p + 24 + head*8`).
/// T-13.5: head_offset folds into the data ptr so existing slot
/// offset math (`data + i*8`) automatically resolves to the
/// correct physical slot for shifted arrays. For freshly-allocated
/// arrays head=0, the head*8 add collapses to 0 at LLVM-opt time
/// when arr_alloc is visible to the caller.
fn arr_data_ptr<'ctx>(
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
fn arr_raw_data_ptr<'ctx>(
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
fn arr_head_x8_load<'ctx>(
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
fn arr_head_load<'ctx>(
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
fn arr_len_load<'ctx>(
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
fn arr_cap_load<'ctx>(
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

/// Emit one `[N x i8]` private constant per interned string. Just the raw
/// bytes — no NUL terminator. The string runtime carries length explicitly
/// in the heap StrRepr's first 8 bytes.
fn emit_string_global<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    idx: usize,
    bytes: &[u8],
) -> inkwell::values::GlobalValue<'ctx> {
    let i8_t = ctx.i8_type();
    let arr_t = i8_t.array_type(bytes.len() as u32);
    let arr = ctx.const_string(bytes, false);
    let g = m.add_global(arr_t, None, &format!(".str{idx}"));
    g.set_initializer(&arr);
    g.set_constant(true);
    g.set_linkage(inkwell::module::Linkage::Private);
    g.set_unnamed_addr(true);
    g
}

/// `[hdr:8 (rc=1, tag=STR, flags=STATIC_LITERAL)] [len:8] [bytes:N]` —
/// drop-in Str object that lives in `.rodata`. rc_inc / rc_dec /
/// str_free / arr_free all short-circuit via the STATIC flag in the
/// header so the global is never written to (safe to mark constant).
///
/// Serves `intern_string_literal` callsites — every literal in a hot
/// loop now resolves to the same global ptr instead of paying a
/// per-iter str_alloc + memcpy + str_drop. Memory cost: one extra
/// 16-byte header per unique literal, paid once at link time.
fn emit_static_str_global<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    idx: usize,
    bytes: &[u8],
) -> inkwell::values::GlobalValue<'ctx> {
    let i8_t = ctx.i8_type();
    let i64_t = ctx.i64_type();
    let len = bytes.len() as u64;

    // Universal heap header packed into a single u64:
    //   refcount (u32) @ [0..32]   = 1 (irrelevant — rc_inc/dec no-op)
    //   type_tag (u16) @ [32..48]  = TAG_STR (= 0)
    //   flags    (u16) @ [48..64]  = STATIC_LITERAL (= 4)
    let header_u64: u64 = 1u64 | ((STATIC_LITERAL_FLAG as u64) << 48);
    let hdr = i64_t.const_int(header_u64, false);
    let len_v = i64_t.const_int(len, false);
    let bytes_arr = ctx.const_string(bytes, false);

    // Anonymous struct so the layout exactly matches `[u64, u64, [N x i8]]`
    // — the runtime reads the header at offset 0 and the bytes at offset 16.
    let body = ctx.const_struct(
        &[hdr.into(), len_v.into(), bytes_arr.into()],
        true, // packed — prevent LLVM from inserting padding between fields
    );
    let body_t = ctx.struct_type(
        &[
            i64_t.into(),
            i64_t.into(),
            i8_t.array_type(len as u32).into(),
        ],
        true,
    );
    let g = m.add_global(body_t, None, &format!(".sstr{idx}"));
    g.set_initializer(&body);
    g.set_constant(true);
    g.set_linkage(inkwell::module::Linkage::Private);
    g.set_unnamed_addr(true);
    g
}

/// Mirror of `__TORAJS_FLAG_STATIC_LITERAL` in runtime_str.c. Encoded
/// here so the header u64 above can be built without a runtime lookup.
const STATIC_LITERAL_FLAG: u16 = 4;

/// Phase K.3 — emit one LLVM module-level data global per
/// `s::DataGlobal`. Zero-initialized; the SSA `main` fn lowers the
/// user's init expression and stores into the slot before any other
/// code runs. K.3 only registers primitive Copy types — string /
/// array / object globals are out of scope until a follow-up wires
/// up exit-time drop hooks.
fn emit_data_global<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    g: &s::DataGlobal,
) -> inkwell::values::GlobalValue<'ctx> {
    match g.ty {
        Type::I64 => {
            let t = ctx.i64_type();
            let glob = m.add_global(t, None, &g.name);
            glob.set_initializer(&t.const_int(0, false));
            glob
        }
        Type::I32 => {
            let t = ctx.i32_type();
            let glob = m.add_global(t, None, &g.name);
            glob.set_initializer(&t.const_int(0, false));
            glob
        }
        Type::F64 => {
            let t = ctx.f64_type();
            let glob = m.add_global(t, None, &g.name);
            glob.set_initializer(&t.const_float(0.0));
            glob
        }
        Type::Bool => {
            let t = ctx.bool_type();
            let glob = m.add_global(t, None, &g.name);
            glob.set_initializer(&t.const_int(0, false));
            glob
        }
        // K.4 / K.6 — refcount-typed globals (Str / Arr / Obj). All
        // are ptr-shaped at SSA layer; the slot holds a heap pointer
        // and ssa_lower emits the per-type drop at fall-through
        // `main` exit via `emit_drop_value` (which walks array
        // elements / object fields recursively when refcounted).
        Type::Str | Type::Arr(_) | Type::Obj(_) => {
            let t = ctx.ptr_type(AddressSpace::default());
            let glob = m.add_global(t, None, &g.name);
            glob.set_initializer(&t.const_null());
            glob
        }
        other => panic!(
            "emit_data_global: unsupported global type {other:?} (K.6 supports primitive Copy + Str / Arr / Obj; Closure / FnSig are deferred)"
        ),
    }
}

/// `__torajs_str_alloc(*const u8 src, u64 len) -> *StrRepr`
///
/// Allocates a fresh Str heap object via `emit_str_alloc_header` (which
/// writes the universal refcount header + len field), then copies
/// `len` bytes from `src` into the data area at `p + STR_HDR_DATA_OFF`.
fn define_str_alloc<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    str_alloc_pooled: FunctionValue<'ctx>,
    memcpy: FunctionValue<'ctx>,
) -> FunctionValue<'ctx> {
    let builder = ctx.create_builder();
    let i64_t = ctx.i64_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let fn_t = ptr_t.fn_type(&[ptr_t.into(), i64_t.into()], false);
    let f = m.add_function("__torajs_str_alloc", fn_t, None);
    let entry = ctx.append_basic_block(f, "entry");
    builder.position_at_end(entry);

    let src = f.get_nth_param(0).unwrap().into_pointer_value();
    let len = f.get_nth_param(1).unwrap().into_int_value();

    // Pool-aware alloc with header pre-initialized inside the C helper.
    let p = builder
        .build_call(str_alloc_pooled, &[len.into()], "p")
        .unwrap()
        .try_as_basic_value()
        .unwrap_basic()
        .into_pointer_value();
    let data = str_data_ptr(ctx, &builder, p, "data");
    builder
        .build_call(memcpy, &[data.into(), src.into(), len.into()], "_cp")
        .unwrap();

    builder.build_return(Some(&p)).unwrap();
    f
}

/// `__torajs_str_concat(*StrRepr a, *StrRepr b) -> *StrRepr`
///
/// Allocates a fresh Str holding `a.bytes ++ b.bytes`. Operands are
/// read-only; the caller's drops still fire normally on them.
fn define_str_concat<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    str_alloc_pooled: FunctionValue<'ctx>,
    memcpy: FunctionValue<'ctx>,
) -> FunctionValue<'ctx> {
    let builder = ctx.create_builder();
    let i8_t = ctx.i8_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let fn_t = ptr_t.fn_type(&[ptr_t.into(), ptr_t.into()], false);
    let f = m.add_function("__torajs_str_concat", fn_t, None);
    let entry = ctx.append_basic_block(f, "entry");
    builder.position_at_end(entry);

    let a = f.get_nth_param(0).unwrap().into_pointer_value();
    let b = f.get_nth_param(1).unwrap().into_pointer_value();

    let a_len = str_len_load(ctx, &builder, a, "a_len");
    let b_len = str_len_load(ctx, &builder, b, "b_len");
    let total = builder.build_int_add(a_len, b_len, "total").unwrap();

    let p = emit_str_alloc_header(ctx, &builder, str_alloc_pooled, total);
    let p_data = str_data_ptr(ctx, &builder, p, "p_data");
    let a_data = str_data_ptr(ctx, &builder, a, "a_data");
    builder
        .build_call(
            memcpy,
            &[p_data.into(), a_data.into(), a_len.into()],
            "_cp_a",
        )
        .unwrap();
    // p_data2 = p_data + a_len
    let p_data2 = unsafe {
        builder
            .build_in_bounds_gep(i8_t, p_data, &[a_len], "p_data2")
            .unwrap()
    };
    let b_data = str_data_ptr(ctx, &builder, b, "b_data");
    builder
        .build_call(
            memcpy,
            &[p_data2.into(), b_data.into(), b_len.into()],
            "_cp_b",
        )
        .unwrap();

    builder.build_return(Some(&p)).unwrap();
    f
}

/// M6.1 — `__torajs_str_slice(*StrRepr s, i64 start, i64 end) -> *StrRepr`.
/// Bounds-clamp start ∈ [0, len], end ∈ [start, len], allocate a fresh
/// StrRepr holding `data[start..end]`. Inputs are read-only.
fn define_str_slice<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    str_alloc_pooled: FunctionValue<'ctx>,
    memcpy: FunctionValue<'ctx>,
) -> FunctionValue<'ctx> {
    let builder = ctx.create_builder();
    let i64_t = ctx.i64_type();
    let i8_t = ctx.i8_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let fn_t = ptr_t.fn_type(&[ptr_t.into(), i64_t.into(), i64_t.into()], false);
    let f = m.add_function("__torajs_str_slice", fn_t, None);
    let entry = ctx.append_basic_block(f, "entry");
    builder.position_at_end(entry);

    let s = f.get_nth_param(0).unwrap().into_pointer_value();
    let start = f.get_nth_param(1).unwrap().into_int_value();
    let end = f.get_nth_param(2).unwrap().into_int_value();
    let zero = i64_t.const_int(0, false);

    let len = str_len_load(ctx, &builder, s, "len");
    // V3-18 m1.h.36 — JS spec §21.1.3.21 negative-index handling:
    //   if start < 0 → start = max(len + start, 0)
    //   else         → start = min(start, len)
    // Same for end. Pre-fix str_slice clamped negative to 0,
    // so `"hello".slice(-2)` returned the whole string.
    let start_neg = builder
        .build_int_compare(IntPredicate::SLT, start, zero, "start_neg")
        .unwrap();
    let start_plus_len = builder.build_int_add(start, len, "start_plus_len").unwrap();
    let start_pl_neg = builder
        .build_int_compare(IntPredicate::SLT, start_plus_len, zero, "spl_neg")
        .unwrap();
    let start_norm_neg = builder
        .build_select(start_pl_neg, zero, start_plus_len, "start_norm_neg")
        .unwrap()
        .into_int_value();
    let start_after_lo = builder
        .build_select(start_neg, start_norm_neg, start, "start_lo")
        .unwrap()
        .into_int_value();
    let start_over = builder
        .build_int_compare(IntPredicate::SGT, start_after_lo, len, "start_over")
        .unwrap();
    let start_c = builder
        .build_select(start_over, len, start_after_lo, "start_c")
        .unwrap()
        .into_int_value();
    let end_neg = builder
        .build_int_compare(IntPredicate::SLT, end, zero, "end_neg")
        .unwrap();
    let end_plus_len = builder.build_int_add(end, len, "end_plus_len").unwrap();
    let end_pl_neg = builder
        .build_int_compare(IntPredicate::SLT, end_plus_len, zero, "epl_neg")
        .unwrap();
    let end_norm_neg = builder
        .build_select(end_pl_neg, zero, end_plus_len, "end_norm_neg")
        .unwrap()
        .into_int_value();
    let end_after_neg = builder
        .build_select(end_neg, end_norm_neg, end, "end_after_neg")
        .unwrap()
        .into_int_value();
    let end_under = builder
        .build_int_compare(IntPredicate::SLT, end_after_neg, start_c, "end_under")
        .unwrap();
    let end_after_lo = builder
        .build_select(end_under, start_c, end_after_neg, "end_lo")
        .unwrap()
        .into_int_value();
    let end_over = builder
        .build_int_compare(IntPredicate::SGT, end_after_lo, len, "end_over")
        .unwrap();
    let end_c = builder
        .build_select(end_over, len, end_after_lo, "end_c")
        .unwrap()
        .into_int_value();

    let new_len = builder.build_int_sub(end_c, start_c, "new_len").unwrap();
    let p = emit_str_alloc_header(ctx, &builder, str_alloc_pooled, new_len);
    let p_data = str_data_ptr(ctx, &builder, p, "p_data");
    let s_data_base = str_data_ptr(ctx, &builder, s, "s_data_base");
    let s_data = unsafe {
        builder
            .build_in_bounds_gep(i8_t, s_data_base, &[start_c], "s_data")
            .unwrap()
    };
    builder
        .build_call(
            memcpy,
            &[p_data.into(), s_data.into(), new_len.into()],
            "_cp",
        )
        .unwrap();
    builder.build_return(Some(&p)).unwrap();
    f
}

/// M6.1 — `__torajs_str_char_code_at(*StrRepr s, i64 i) -> i64`. Returns
/// the byte at index `i` zero-extended to i64. M6.1 stub: returns 0
/// for out-of-bounds (TS spec is NaN, but we don't have NaN-as-i64).
fn define_str_char_code_at<'ctx>(ctx: &'ctx Context, m: &LlvmModule<'ctx>) -> FunctionValue<'ctx> {
    let builder = ctx.create_builder();
    let i64_t = ctx.i64_type();
    let i8_t = ctx.i8_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let fn_t = i64_t.fn_type(&[ptr_t.into(), i64_t.into()], false);
    let f = m.add_function("__torajs_str_char_code_at", fn_t, None);
    let entry = ctx.append_basic_block(f, "entry");
    let oob_blk = ctx.append_basic_block(f, "oob");
    let load_blk = ctx.append_basic_block(f, "load");
    builder.position_at_end(entry);

    let s = f.get_nth_param(0).unwrap().into_pointer_value();
    let i = f.get_nth_param(1).unwrap().into_int_value();
    let len = str_len_load(ctx, &builder, s, "len");
    let zero = i64_t.const_int(0, false);
    let i_neg = builder
        .build_int_compare(IntPredicate::SLT, i, zero, "i_neg")
        .unwrap();
    let i_oor = builder
        .build_int_compare(IntPredicate::SGE, i, len, "i_oor")
        .unwrap();
    let oob = builder.build_or(i_neg, i_oor, "oob").unwrap();
    builder
        .build_conditional_branch(oob, oob_blk, load_blk)
        .unwrap();
    builder.position_at_end(oob_blk);
    builder.build_return(Some(&zero)).unwrap();
    builder.position_at_end(load_blk);
    let s_data = str_data_ptr(ctx, &builder, s, "s_data");
    let p = unsafe {
        builder
            .build_in_bounds_gep(i8_t, s_data, &[i], "p")
            .unwrap()
    };
    let b = builder.build_load(i8_t, p, "b").unwrap().into_int_value();
    let v = builder.build_int_z_extend(b, i64_t, "v").unwrap();
    builder.build_return(Some(&v)).unwrap();
    f
}

/// Helper: emits the `s.starts_with(prefix)` / `s.ends_with(suffix)`
/// shape — an i64 cmp on lens followed by a memcmp at the right offset.
/// `from_end` true picks the end-aligned offset.
fn define_str_prefix_suffix_check<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    memcmp: FunctionValue<'ctx>,
    name: &str,
    from_end: bool,
) -> FunctionValue<'ctx> {
    let builder = ctx.create_builder();
    let i8_t = ctx.i8_type();
    let bool_t = ctx.bool_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let fn_t = bool_t.fn_type(&[ptr_t.into(), ptr_t.into()], false);
    let f = m.add_function(name, fn_t, None);
    let entry = ctx.append_basic_block(f, "entry");
    let cmp_blk = ctx.append_basic_block(f, "cmp");
    let too_long = ctx.append_basic_block(f, "too_long");
    builder.position_at_end(entry);

    let s = f.get_nth_param(0).unwrap().into_pointer_value();
    let n = f.get_nth_param(1).unwrap().into_pointer_value();
    let s_len = str_len_load(ctx, &builder, s, "s_len");
    let n_len = str_len_load(ctx, &builder, n, "n_len");
    let too = builder
        .build_int_compare(IntPredicate::SGT, n_len, s_len, "too")
        .unwrap();
    builder
        .build_conditional_branch(too, too_long, cmp_blk)
        .unwrap();
    builder.position_at_end(too_long);
    builder
        .build_return(Some(&bool_t.const_int(0, false)))
        .unwrap();
    builder.position_at_end(cmp_blk);
    // s_off = from_end ? s_len - n_len : 0 (relative to s_data)
    let s_data_base = str_data_ptr(ctx, &builder, s, "s_data_base");
    let s_data = if from_end {
        let diff = builder.build_int_sub(s_len, n_len, "diff").unwrap();
        unsafe {
            builder
                .build_in_bounds_gep(i8_t, s_data_base, &[diff], "s_data")
                .unwrap()
        }
    } else {
        s_data_base
    };
    let n_data = str_data_ptr(ctx, &builder, n, "n_data");
    let r = builder
        .build_call(memcmp, &[s_data.into(), n_data.into(), n_len.into()], "r")
        .unwrap()
        .try_as_basic_value()
        .unwrap_basic()
        .into_int_value();
    let eq = builder
        .build_int_compare(
            IntPredicate::EQ,
            r,
            ctx.i32_type().const_int(0, false),
            "eq",
        )
        .unwrap();
    builder.build_return(Some(&eq)).unwrap();
    f
}

/// M6.1 — `__torajs_str_index_of(*StrRepr s, *StrRepr sub) -> i64`.
/// Naive byte-scan; returns first match index or -1. Empty `sub`
/// returns 0 (matches TS).
fn define_str_index_of<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    memcmp: FunctionValue<'ctx>,
) -> FunctionValue<'ctx> {
    let builder = ctx.create_builder();
    let i64_t = ctx.i64_type();
    let i8_t = ctx.i8_type();
    let i32_t = ctx.i32_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let fn_t = i64_t.fn_type(&[ptr_t.into(), ptr_t.into()], false);
    let f = m.add_function("__torajs_str_index_of", fn_t, None);
    let entry = ctx.append_basic_block(f, "entry");
    let empty_sub_blk = ctx.append_basic_block(f, "empty_sub");
    let header_blk = ctx.append_basic_block(f, "header");
    let body_blk = ctx.append_basic_block(f, "body");
    let cmp_blk = ctx.append_basic_block(f, "cmp");
    let found_blk = ctx.append_basic_block(f, "found");
    let next_blk = ctx.append_basic_block(f, "next");
    let not_found_blk = ctx.append_basic_block(f, "not_found");
    builder.position_at_end(entry);

    let s = f.get_nth_param(0).unwrap().into_pointer_value();
    let sub = f.get_nth_param(1).unwrap().into_pointer_value();
    let s_len = str_len_load(ctx, &builder, s, "s_len");
    let sub_len = str_len_load(ctx, &builder, sub, "sub_len");
    let zero = i64_t.const_int(0, false);
    let sub_empty = builder
        .build_int_compare(IntPredicate::EQ, sub_len, zero, "sub_empty")
        .unwrap();
    builder
        .build_conditional_branch(sub_empty, empty_sub_blk, header_blk)
        .unwrap();
    builder.position_at_end(empty_sub_blk);
    builder.build_return(Some(&zero)).unwrap();

    // i_slot = 0; max_i = s_len - sub_len
    builder.position_at_end(header_blk);
    let i_slot = builder.build_alloca(i64_t, "i").unwrap();
    builder.build_store(i_slot, zero).unwrap();
    let max_i = builder.build_int_sub(s_len, sub_len, "max_i").unwrap();
    let s_data_base = str_data_ptr(ctx, &builder, s, "s_data_base");
    let sub_data = str_data_ptr(ctx, &builder, sub, "sub_data");
    let header_inner = ctx.append_basic_block(f, "header_inner");
    builder.build_unconditional_branch(header_inner).unwrap();
    builder.position_at_end(header_inner);
    let i_now = builder
        .build_load(i64_t, i_slot, "i_now")
        .unwrap()
        .into_int_value();
    let cont = builder
        .build_int_compare(IntPredicate::SLE, i_now, max_i, "cont")
        .unwrap();
    builder
        .build_conditional_branch(cont, body_blk, not_found_blk)
        .unwrap();
    builder.position_at_end(body_blk);
    builder.build_unconditional_branch(cmp_blk).unwrap();

    // cmp: memcmp(s_data + i, sub_data, sub_len) == 0 ?
    builder.position_at_end(cmp_blk);
    let i_now2 = builder
        .build_load(i64_t, i_slot, "i_now2")
        .unwrap()
        .into_int_value();
    let s_data = unsafe {
        builder
            .build_in_bounds_gep(i8_t, s_data_base, &[i_now2], "s_data")
            .unwrap()
    };
    let r = builder
        .build_call(
            memcmp,
            &[s_data.into(), sub_data.into(), sub_len.into()],
            "r",
        )
        .unwrap()
        .try_as_basic_value()
        .unwrap_basic()
        .into_int_value();
    let eq = builder
        .build_int_compare(IntPredicate::EQ, r, i32_t.const_int(0, false), "eq")
        .unwrap();
    builder
        .build_conditional_branch(eq, found_blk, next_blk)
        .unwrap();

    builder.position_at_end(found_blk);
    let i_found = builder
        .build_load(i64_t, i_slot, "i_found")
        .unwrap()
        .into_int_value();
    builder.build_return(Some(&i_found)).unwrap();

    builder.position_at_end(next_blk);
    let i_then = builder
        .build_load(i64_t, i_slot, "i_then")
        .unwrap()
        .into_int_value();
    let i_next = builder
        .build_int_add(i_then, i64_t.const_int(1, false), "i_next")
        .unwrap();
    builder.build_store(i_slot, i_next).unwrap();
    builder.build_unconditional_branch(header_inner).unwrap();

    builder.position_at_end(not_found_blk);
    let neg_one = i64_t.const_int((-1_i64) as u64, true);
    builder.build_return(Some(&neg_one)).unwrap();
    f
}

/// M6.1 — `__torajs_str_includes(*StrRepr s, *StrRepr sub) -> bool`.
/// Defers to `__torajs_str_index_of` and tests `>= 0`.
fn define_str_includes<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    index_of: FunctionValue<'ctx>,
) -> FunctionValue<'ctx> {
    let builder = ctx.create_builder();
    let i64_t = ctx.i64_type();
    let bool_t = ctx.bool_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let fn_t = bool_t.fn_type(&[ptr_t.into(), ptr_t.into()], false);
    let f = m.add_function("__torajs_str_includes", fn_t, None);
    let entry = ctx.append_basic_block(f, "entry");
    builder.position_at_end(entry);
    let s = f.get_nth_param(0).unwrap();
    let sub = f.get_nth_param(1).unwrap();
    let r = builder
        .build_call(index_of, &[s.into(), sub.into()], "r")
        .unwrap()
        .try_as_basic_value()
        .unwrap_basic()
        .into_int_value();
    let cmp = builder
        .build_int_compare(IntPredicate::SGE, r, i64_t.const_int(0, false), "cmp")
        .unwrap();
    builder.build_return(Some(&cmp)).unwrap();
    f
}

/// `print_bool(bool) -> void` — putchar's `"true\n"` or `"false\n"`
/// per the bool input. M6.1 console.log dispatch routes Type::Bool
/// args here. (Same shared stdio buffer as print_i64 / str_print —
/// no ordering surprises.)
fn define_print_bool<'ctx>(
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
fn define_print_f64<'ctx>(ctx: &'ctx Context, m: &LlvmModule<'ctx>) -> FunctionValue<'ctx> {
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

/// One-arg f64→f64 wrapper around a libc math function. Used to expose
/// `Math.sqrt`, `Math.abs`, `Math.floor`, `Math.ceil` etc. — all share
/// the same shape:
///     fn __torajs_math_<op>(x: f64) -> f64 {
///         <libc_name>(x)
///     }
/// Constructed in three lines of LLVM IR. Saves us writing a separate
/// `define_*` for each method and centralizes the dispatch.
fn define_math_unary<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    fn_name: &str,
    libc_name: &str,
) -> FunctionValue<'ctx> {
    let f64_t = ctx.f64_type();
    // Re-declare libc fn (idempotent — LLVM dedupes by name).
    let libc_t = f64_t.fn_type(&[f64_t.into()], false);
    let libc_fn = m
        .get_function(libc_name)
        .unwrap_or_else(|| m.add_function(libc_name, libc_t, None));

    let fn_t = f64_t.fn_type(&[f64_t.into()], false);
    let f = m.add_function(fn_name, fn_t, None);
    let entry = ctx.append_basic_block(f, "entry");
    let builder = ctx.create_builder();
    builder.position_at_end(entry);
    let arg = f.get_nth_param(0).unwrap().into_float_value();
    let r = builder
        .build_call(libc_fn, &[arg.into()], "r")
        .unwrap()
        .try_as_basic_value()
        .unwrap_basic()
        .into_float_value();
    builder.build_return(Some(&r)).unwrap();
    f
}

/// Two-arg f64×f64→f64 wrapper. `Math.pow`, `Math.min`, `Math.max`.
fn define_math_binary<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    fn_name: &str,
    libc_name: &str,
) -> FunctionValue<'ctx> {
    let f64_t = ctx.f64_type();
    let libc_t = f64_t.fn_type(&[f64_t.into(), f64_t.into()], false);
    let libc_fn = m
        .get_function(libc_name)
        .unwrap_or_else(|| m.add_function(libc_name, libc_t, None));

    let fn_t = f64_t.fn_type(&[f64_t.into(), f64_t.into()], false);
    let f = m.add_function(fn_name, fn_t, None);
    let entry = ctx.append_basic_block(f, "entry");
    let builder = ctx.create_builder();
    builder.position_at_end(entry);
    let a = f.get_nth_param(0).unwrap().into_float_value();
    let b = f.get_nth_param(1).unwrap().into_float_value();
    let r = builder
        .build_call(libc_fn, &[a.into(), b.into()], "r")
        .unwrap()
        .try_as_basic_value()
        .unwrap_basic()
        .into_float_value();
    builder.build_return(Some(&r)).unwrap();
    f
}

/// M4 — exception state. Three module-level i64 globals
/// (`__torajs_throw_active`, `__torajs_throw_tag`, `__torajs_throw_value`)
/// plus four runtime helpers operating on them. Lowered code calls
/// these (never the globals directly) so the same shape works in both
/// AOT (LLVM IR) and JIT (Rust extern "C" fns over thread-local
/// statics).
///
/// P4.7 — `__torajs_throw_tag` added so a `throw <value>` can record
/// its dynamic-tag (ANY_I64 / ANY_F64 / ANY_BOOL / ANY_HEAP etc.)
/// alongside the i64-packed value. Catch sites with `: any`
/// annotation read both and reconstruct an Any-box; catch sites with
/// a typed `: T` annotation read only the value (ignoring tag) and
/// the existing type-cast machinery handles ptr↔i64 widening.
fn ensure_throw_globals<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
) -> (
    inkwell::values::GlobalValue<'ctx>,
    inkwell::values::GlobalValue<'ctx>,
    inkwell::values::GlobalValue<'ctx>,
) {
    let i64_t = ctx.i64_type();
    let active = match m.get_global("__torajs_throw_active") {
        Some(g) => g,
        None => {
            let g = m.add_global(i64_t, None, "__torajs_throw_active");
            g.set_initializer(&i64_t.const_int(0, false));
            g
        }
    };
    let tag = match m.get_global("__torajs_throw_tag") {
        Some(g) => g,
        None => {
            let g = m.add_global(i64_t, None, "__torajs_throw_tag");
            g.set_initializer(&i64_t.const_int(0, false));
            g
        }
    };
    let value = match m.get_global("__torajs_throw_value") {
        Some(g) => g,
        None => {
            let g = m.add_global(i64_t, None, "__torajs_throw_value");
            g.set_initializer(&i64_t.const_int(0, false));
            g
        }
    };
    (active, tag, value)
}

fn define_throw_set<'ctx>(ctx: &'ctx Context, m: &LlvmModule<'ctx>) -> FunctionValue<'ctx> {
    let (active_g, tag_g, value_g) = ensure_throw_globals(ctx, m);
    let builder = ctx.create_builder();
    let i64_t = ctx.i64_type();
    let void_t = ctx.void_type();
    // P4.7 — signature is now (tag, value). C callers (
    // torajs_throw_type_error, torajs_throw_range_error, fetch's
    // sync-err path) pass HEAP=4 since those errors are str ptrs.
    let fn_t = void_t.fn_type(&[i64_t.into(), i64_t.into()], false);
    let f = m.add_function("__torajs_throw_set", fn_t, None);
    let entry = ctx.append_basic_block(f, "entry");
    builder.position_at_end(entry);
    let tag = f.get_nth_param(0).unwrap().into_int_value();
    let v = f.get_nth_param(1).unwrap().into_int_value();
    builder
        .build_store(active_g.as_pointer_value(), i64_t.const_int(1, false))
        .unwrap();
    builder.build_store(tag_g.as_pointer_value(), tag).unwrap();
    builder.build_store(value_g.as_pointer_value(), v).unwrap();
    builder.build_return(None).unwrap();
    f
}

fn define_throw_check<'ctx>(ctx: &'ctx Context, m: &LlvmModule<'ctx>) -> FunctionValue<'ctx> {
    let (active_g, _, _) = ensure_throw_globals(ctx, m);
    let builder = ctx.create_builder();
    let i64_t = ctx.i64_type();
    let fn_t = i64_t.fn_type(&[], false);
    let f = m.add_function("__torajs_throw_check", fn_t, None);
    let entry = ctx.append_basic_block(f, "entry");
    builder.position_at_end(entry);
    let v = builder
        .build_load(i64_t, active_g.as_pointer_value(), "v")
        .unwrap()
        .into_int_value();
    builder.build_return(Some(&v)).unwrap();
    f
}

fn define_throw_take<'ctx>(ctx: &'ctx Context, m: &LlvmModule<'ctx>) -> FunctionValue<'ctx> {
    let (active_g, _, value_g) = ensure_throw_globals(ctx, m);
    let builder = ctx.create_builder();
    let i64_t = ctx.i64_type();
    let fn_t = i64_t.fn_type(&[], false);
    let f = m.add_function("__torajs_throw_take", fn_t, None);
    let entry = ctx.append_basic_block(f, "entry");
    builder.position_at_end(entry);
    let v = builder
        .build_load(i64_t, value_g.as_pointer_value(), "v")
        .unwrap()
        .into_int_value();
    builder
        .build_store(active_g.as_pointer_value(), i64_t.const_int(0, false))
        .unwrap();
    builder.build_return(Some(&v)).unwrap();
    f
}

/// P4.7 — peek the throw tag without clearing active. Catch sites
/// with `: any` annotation call this BEFORE throw_take so the tag
/// is captured (throw_take clears `__torajs_throw_active` as a
/// side effect but leaves the value/tag slots untouched). Typed-
/// tier catches don't call this — they only care about the i64
/// value and let the cast helper widen it.
fn define_throw_take_tag<'ctx>(ctx: &'ctx Context, m: &LlvmModule<'ctx>) -> FunctionValue<'ctx> {
    let (_, tag_g, _) = ensure_throw_globals(ctx, m);
    let builder = ctx.create_builder();
    let i64_t = ctx.i64_type();
    let fn_t = i64_t.fn_type(&[], false);
    let f = m.add_function("__torajs_throw_take_tag", fn_t, None);
    let entry = ctx.append_basic_block(f, "entry");
    builder.position_at_end(entry);
    let v = builder
        .build_load(i64_t, tag_g.as_pointer_value(), "v")
        .unwrap()
        .into_int_value();
    builder.build_return(Some(&v)).unwrap();
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
fn define_obj_alloc<'ctx>(
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
/// `__torajs_obj_drop(*void p) -> void` — plain `free(p)`. The
/// inline drop site (ssa_lower's emit_drop_value Type::Obj walk_blk)
/// gates on `is_class_sid` to call `__torajs_cycle_unbuffer` BEFORE
/// reaching here, so this stays a 1-instruction tail call.
fn define_obj_drop<'ctx>(
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

/// `__torajs_arr_alloc(u64 initial_cap) -> *u8`
///
/// Body is a single tail call into `__torajs_arr_alloc_pooled`
/// (runtime_str.c). The C-side helper owns the pool fast-path and
/// the malloc + header init slow-path; LTO inlines it back into the
/// caller so this stays the same shape as the str_alloc / str_drop
/// pattern.
fn define_arr_alloc<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    arr_alloc_pooled: FunctionValue<'ctx>,
) -> FunctionValue<'ctx> {
    let builder = ctx.create_builder();
    let i64_t = ctx.i64_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let fn_t = ptr_t.fn_type(&[i64_t.into()], false);
    let f = m.add_function("__torajs_arr_alloc", fn_t, None);
    let entry = ctx.append_basic_block(f, "entry");
    builder.position_at_end(entry);

    let cap = f.get_nth_param(0).unwrap().into_int_value();
    let p = builder
        .build_call(arr_alloc_pooled, &[cap.into()], "arr_p")
        .unwrap()
        .try_as_basic_value()
        .unwrap_basic()
        .into_pointer_value();
    builder.build_return(Some(&p)).unwrap();
    f
}

/// `__torajs_arr_push(*u8 arr, i64 val) -> *u8`
///
/// T-13.5 deque-aware:
///     len  = *(u64*)(arr + 8)
///     cap  = *(u32*)(arr + 16)  (zext)
///     head = *(u32*)(arr + 20)  (zext)
///     phys_used = head + len
///     if phys_used == cap:                         // physical end reached
///       if head > 0:
///         memmove(data, data + head*8, len*8)      // compact head→0
///         *(u32*)(arr + 20) = 0
///       if len == cap:                              // genuinely full
///         new_cap = cap==0 ? 4 : cap*2
///         arr = realloc(arr, 24 + new_cap*8)
///         *(u32*)(arr + 16) = new_cap               // 4-byte store, head intact
///     store val at logical[len] (= data + (head_after + len)*8)
///     *(u64*)(arr + 8) = len + 1
///
/// `arr_data_ptr` already folds head_offset into the data ptr, so
/// `data + len*8` resolves to the right physical slot. After compact,
/// head=0 so the head load at the use site returns 0 (we re-load it).
fn define_arr_push<'ctx>(
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

/// M6.2 fast-path. `arr_reserve(arr, new_cap) -> arr*` ensures
/// `cap >= new_cap`; reallocs once if needed, otherwise no-op. Returns
/// the (possibly new) ptr — caller stores it back into its slot.
fn define_arr_reserve<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    realloc: FunctionValue<'ctx>,
) -> FunctionValue<'ctx> {
    let builder = ctx.create_builder();
    let i64_t = ctx.i64_type();
    let i32_t = ctx.i32_type();
    let i8_t = ctx.i8_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let fn_t = ptr_t.fn_type(&[ptr_t.into(), i64_t.into()], false);
    let f = m.add_function("__torajs_arr_reserve", fn_t, None);
    let entry = ctx.append_basic_block(f, "entry");
    let grow_blk = ctx.append_basic_block(f, "grow");
    let exit_blk = ctx.append_basic_block(f, "exit");
    builder.position_at_end(entry);
    let arr_in = f.get_nth_param(0).unwrap().into_pointer_value();
    let new_cap = f.get_nth_param(1).unwrap().into_int_value();
    let cap = arr_cap_load(ctx, &builder, arr_in, "cap");
    let need_grow = builder
        .build_int_compare(IntPredicate::ULT, cap, new_cap, "need_grow")
        .unwrap();
    builder
        .build_conditional_branch(need_grow, grow_blk, exit_blk)
        .unwrap();
    // grow: realloc(p, ARR_HDR_DATA_OFF + new_cap * 8); store new_cap (4-byte u32); pass to exit
    builder.position_at_end(grow_blk);
    let new_bytes = builder
        .build_int_mul(new_cap, i64_t.const_int(8, false), "")
        .unwrap();
    let new_total = builder
        .build_int_add(new_bytes, i64_t.const_int(ARR_HDR_DATA_OFF, false), "")
        .unwrap();
    let arr_grown = builder
        .build_call(realloc, &[arr_in.into(), new_total.into()], "arr_grown")
        .unwrap()
        .try_as_basic_value()
        .unwrap_basic()
        .into_pointer_value();
    let new_cap_p = unsafe {
        builder
            .build_in_bounds_gep(
                i8_t,
                arr_grown,
                &[i64_t.const_int(ARR_HDR_CAP_OFF, false)],
                "",
            )
            .unwrap()
    };
    let new_cap_i32 = builder
        .build_int_truncate(new_cap, i32_t, "new_cap_i32")
        .unwrap();
    builder.build_store(new_cap_p, new_cap_i32).unwrap();
    builder.build_unconditional_branch(exit_blk).unwrap();
    // exit: phi arr → return
    builder.position_at_end(exit_blk);
    let phi = builder.build_phi(ptr_t, "arr").unwrap();
    phi.add_incoming(&[(&arr_in, entry), (&arr_grown, grow_blk)]);
    let arr = phi.as_basic_value().into_pointer_value();
    builder.build_return(Some(&arr)).unwrap();
    f
}

/// M6.2 fast-path. `arr_push_unchecked(arr, val)` — appends val
/// assuming cap >= len + 1. UB otherwise. Used after a one-shot
/// `arr_reserve` so the per-push capacity check is gone.
fn define_arr_push_unchecked<'ctx>(
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
        .build_int_mul(len, i64_t.const_int(8, false), "")
        .unwrap();
    let slot = unsafe {
        builder
            .build_in_bounds_gep(i8_t, data, &[len_x8], "slot")
            .unwrap()
    };
    builder.build_store(slot, val).unwrap();
    let len_p1 = builder
        .build_int_add(len, i64_t.const_int(1, false), "")
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

/// `__torajs_arr_shift(*u8 arr) -> i64`
///
/// T-13.5: O(1) deque shift via head_offset. Body is 4 memory ops
/// (load value + bump head + dec len). Promoted from C runtime to
/// inkwell-IR + alwaysinline at v0.6+1 perf checkpoint — the C
/// version's `__attribute__((always_inline))` doesn't survive the
/// native-object link boundary (inkwell emits `.o` not bitcode), so
/// the call stayed external in the linked binary even with -flto.
/// Defining the body in inkwell + alwaysinline = LLVM inlines at the
/// IR level before the .o ever forms; the `bl __torajs_arr_shift`
/// in fifo-queue's hot loop disappears.
fn define_arr_shift<'ctx>(ctx: &'ctx Context, m: &LlvmModule<'ctx>) -> FunctionValue<'ctx> {
    let builder = ctx.create_builder();
    let i64_t = ctx.i64_type();
    let i32_t = ctx.i32_type();
    let i8_t = ctx.i8_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let fn_t = i64_t.fn_type(&[ptr_t.into()], false);
    let f = m.add_function("__torajs_arr_shift", fn_t, None);
    mark_alwaysinline(ctx, f);
    let entry = ctx.append_basic_block(f, "entry");
    builder.position_at_end(entry);
    let arr = f.get_nth_param(0).unwrap().into_pointer_value();

    /* Load value at logical[0] = data + head*8 (data starts at +24). */
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

    /* head_offset += 1 (u32) */
    let head_inc = builder
        .build_int_add(head32, i32_t.const_int(1, false), "head_inc")
        .unwrap();
    builder.build_store(head_p, head_inc).unwrap();

    /* len -= 1 (u64) */
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

/// `__torajs_arr_drop(*u8 arr) -> void`
///
/// Phase 2A refcount: dec the universal heap header; free only at zero.
/// NULL passes through. Caller is responsible for walking refcounted
/// element types FIRST (`emit_drop_value Type::Arr` does this in
/// ssa_lower).
fn define_arr_drop<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    free: FunctionValue<'ctx>,
) -> FunctionValue<'ctx> {
    let builder = ctx.create_builder();
    let i32_t = ctx.i32_type();
    let i64_t = ctx.i64_type();
    let i8_t = ctx.i8_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let void_t = ctx.void_type();
    let fn_t = void_t.fn_type(&[ptr_t.into()], false);
    // T-29 — declare arrprops_drop_entry so we can call it from the
    // free path. Returns void; takes the array ptr. No-op for arrays
    // that never had `arr.x = v` written.
    let arrprops_drop_entry = m
        .get_function("__torajs_arrprops_drop_entry")
        .unwrap_or_else(|| m.add_function("__torajs_arrprops_drop_entry", fn_t, None));
    let f = m.add_function("__torajs_arr_drop", fn_t, None);
    let entry = ctx.append_basic_block(f, "entry");
    let static_check_blk = ctx.append_basic_block(f, "static_check");
    let dec_blk = ctx.append_basic_block(f, "dec");
    let free_blk = ctx.append_basic_block(f, "free");
    let ret_blk = ctx.append_basic_block(f, "ret");
    builder.position_at_end(entry);
    let arg = f.get_nth_param(0).unwrap().into_pointer_value();
    let null = ptr_t.const_null();
    let is_null = builder
        .build_int_compare(IntPredicate::EQ, arg, null, "is_null")
        .unwrap();
    builder
        .build_conditional_branch(is_null, ret_blk, static_check_blk)
        .unwrap();

    // STATIC_LITERAL flag check — `.rodata` Str-shaped globals must
    // never have rc decremented (the store would page-fault) or be
    // freed. Cheap u16 load + bit test, branch to ret on match.
    builder.position_at_end(static_check_blk);
    let i16_t = ctx.i16_type();
    let flags_ptr = unsafe {
        builder
            .build_in_bounds_gep(
                i8_t,
                arg,
                &[i64_t.const_int(STR_HDR_FLAGS_OFF, false)],
                "flags_p",
            )
            .unwrap()
    };
    let flags = builder
        .build_load(i16_t, flags_ptr, "flags")
        .unwrap()
        .into_int_value();
    let masked = builder
        .build_and(
            flags,
            i16_t.const_int(STATIC_LITERAL_FLAG as u64, false),
            "masked",
        )
        .unwrap();
    let is_static = builder
        .build_int_compare(
            IntPredicate::NE,
            masked,
            i16_t.const_int(0, false),
            "is_static",
        )
        .unwrap();
    builder
        .build_conditional_branch(is_static, ret_blk, dec_blk)
        .unwrap();

    builder.position_at_end(dec_blk);
    let rc_ptr = unsafe {
        builder
            .build_in_bounds_gep(
                i8_t,
                arg,
                &[i64_t.const_int(STR_HDR_REFCOUNT_OFF, false)],
                "rc_ptr",
            )
            .unwrap()
    };
    let rc_now = builder
        .build_load(i32_t, rc_ptr, "rc_now")
        .unwrap()
        .into_int_value();
    let rc_dec = builder
        .build_int_sub(rc_now, i32_t.const_int(1, false), "rc_dec")
        .unwrap();
    builder.build_store(rc_ptr, rc_dec).unwrap();
    let zero = i32_t.const_int(0, false);
    let is_zero = builder
        .build_int_compare(IntPredicate::EQ, rc_dec, zero, "is_zero")
        .unwrap();
    builder
        .build_conditional_branch(is_zero, free_blk, ret_blk)
        .unwrap();

    builder.position_at_end(free_blk);
    // T-29 — drop side-table props entry (no-op for arrays without
    // props). Called BEFORE free so the entry can read arr_ptr to
    // identify its bucket.
    builder
        .build_call(arrprops_drop_entry, &[arg.into()], "_apd")
        .unwrap();
    builder.build_call(free, &[arg.into()], "_f").unwrap();
    builder.build_unconditional_branch(ret_blk).unwrap();

    builder.position_at_end(ret_blk);
    builder.build_return(None).unwrap();
    f
}

/// `__torajs_str_drop(*StrRepr s) -> void`
///
/// Phase B refcount: decrement the universal heap header's refcount;
/// free the alloc only when it reaches zero. NULL passes through.
///
/// ```text
/// if (s == NULL) return;
/// u32 *rc = (u32*)s;          // refcount @ offset 0
/// *rc -= 1;
/// if (*rc == 0) free(s);
/// ```
fn define_str_drop<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    free: FunctionValue<'ctx>,
) -> FunctionValue<'ctx> {
    let builder = ctx.create_builder();
    let i32_t = ctx.i32_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let void_t = ctx.void_type();
    let fn_t = void_t.fn_type(&[ptr_t.into()], false);
    let f = m.add_function("__torajs_str_drop", fn_t, None);
    let entry = ctx.append_basic_block(f, "entry");
    let static_check_blk = ctx.append_basic_block(f, "static_check");
    let dec_blk = ctx.append_basic_block(f, "dec");
    let free_blk = ctx.append_basic_block(f, "free");
    let ret_blk = ctx.append_basic_block(f, "ret");
    builder.position_at_end(entry);
    let arg = f.get_nth_param(0).unwrap().into_pointer_value();
    let null = ptr_t.const_null();
    let is_null = builder
        .build_int_compare(IntPredicate::EQ, arg, null, "is_null")
        .unwrap();
    builder
        .build_conditional_branch(is_null, ret_blk, static_check_blk)
        .unwrap();

    let i64_t = ctx.i64_type();
    let i8_t = ctx.i8_type();
    let i16_t = ctx.i16_type();

    // STATIC_LITERAL flag check — same shape as define_arr_drop's;
    // see that fn for rationale. Without this the rc-store below
    // SIGBUS's on .rodata.
    builder.position_at_end(static_check_blk);
    let flags_ptr = unsafe {
        builder
            .build_in_bounds_gep(
                i8_t,
                arg,
                &[i64_t.const_int(STR_HDR_FLAGS_OFF, false)],
                "flags_p",
            )
            .unwrap()
    };
    let flags = builder
        .build_load(i16_t, flags_ptr, "flags")
        .unwrap()
        .into_int_value();
    let masked = builder
        .build_and(
            flags,
            i16_t.const_int(STATIC_LITERAL_FLAG as u64, false),
            "masked",
        )
        .unwrap();
    let is_static = builder
        .build_int_compare(
            IntPredicate::NE,
            masked,
            i16_t.const_int(0, false),
            "is_static",
        )
        .unwrap();
    builder
        .build_conditional_branch(is_static, ret_blk, dec_blk)
        .unwrap();

    builder.position_at_end(dec_blk);
    // refcount @ STR_HDR_REFCOUNT_OFF (offset 0). Explicit GEP so the
    // const documents the layout; LLVM mem2reg collapses the zero GEP.
    let rc_ptr = unsafe {
        builder
            .build_in_bounds_gep(
                i8_t,
                arg,
                &[i64_t.const_int(STR_HDR_REFCOUNT_OFF, false)],
                "rc_ptr",
            )
            .unwrap()
    };
    let rc_now = builder
        .build_load(i32_t, rc_ptr, "rc_now")
        .unwrap()
        .into_int_value();
    let rc_dec = builder
        .build_int_sub(rc_now, i32_t.const_int(1, false), "rc_dec")
        .unwrap();
    builder.build_store(rc_ptr, rc_dec).unwrap();
    let zero = i32_t.const_int(0, false);
    let is_zero = builder
        .build_int_compare(IntPredicate::EQ, rc_dec, zero, "is_zero")
        .unwrap();
    builder
        .build_conditional_branch(is_zero, free_blk, ret_blk)
        .unwrap();

    builder.position_at_end(free_blk);
    builder.build_call(free, &[arg.into()], "_f").unwrap();
    builder.build_unconditional_branch(ret_blk).unwrap();

    builder.position_at_end(ret_blk);
    builder.build_return(None).unwrap();
    f
}

/// `__torajs_str_print(*StrRepr s) -> void`
///
/// ```text
/// len = *(u64*)s
/// write(1 /*stdout*/, s + 8, len)
/// write(1, "\n", 1)
/// ```
///
/// `__torajs_str_print(*StrRepr)` — writes the bytes + trailing newline
/// through `putchar` (one byte at a time). Goes through the same stdio
/// buffer as `print_i64`, so mixed `console.log(5); console.log("hi")`
/// preserves source order. Earlier we used two `write(2)` syscalls; that
/// was 1-2 syscalls per print but it bypassed stdio's line buffer, so
/// numbers (which are putchar-buffered) flushed at exit AFTER strings
/// (which were already written), reordering output. Per-byte putchar
/// is the simplest cross-buffer-consistent fix; a fwrite/fputc pair via
/// libc's `stdout` global would be faster on long strings but needs
/// platform-specific symbol naming we don't want to wire up yet.
fn define_str_print<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    putchar: FunctionValue<'ctx>,
) -> FunctionValue<'ctx> {
    let builder = ctx.create_builder();
    let i64_t = ctx.i64_type();
    let i32_t = ctx.i32_type();
    let i8_t = ctx.i8_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let void_t = ctx.void_type();
    let fn_t = void_t.fn_type(&[ptr_t.into()], false);
    let f = m.add_function("__torajs_str_print", fn_t, None);
    let entry = ctx.append_basic_block(f, "entry");
    let null_b = ctx.append_basic_block(f, "null_print");
    let nonnull_b = ctx.append_basic_block(f, "nonnull");
    let cond_b = ctx.append_basic_block(f, "cond");
    let body_b = ctx.append_basic_block(f, "body");
    let exit_b = ctx.append_basic_block(f, "exit");
    builder.position_at_end(entry);

    let s = f.get_nth_param(0).unwrap().into_pointer_value();

    // NULL guard — Nullable<Str> slots and uncaptured regex group slots
    // pass NULL through. Print "null\n" rather than segfault on the
    // len read at offset +8.
    let null_check = builder.build_is_null(s, "is_null").unwrap();
    builder
        .build_conditional_branch(null_check, null_b, nonnull_b)
        .unwrap();

    builder.position_at_end(null_b);
    for ch in b"null\n" {
        let c32 = i32_t.const_int(*ch as u64, false);
        builder.build_call(putchar, &[c32.into()], "").unwrap();
    }
    builder.build_return(None).unwrap();

    builder.position_at_end(nonnull_b);
    let len = str_len_load(ctx, &builder, s, "len");
    let data = str_data_ptr(ctx, &builder, s, "data");
    // i_slot = 0
    let i_slot = builder.build_alloca(i64_t, "i").unwrap();
    builder
        .build_store(i_slot, i64_t.const_int(0, false))
        .unwrap();
    builder.build_unconditional_branch(cond_b).unwrap();

    // cond: i < len ? body : exit
    builder.position_at_end(cond_b);
    let i = builder
        .build_load(i64_t, i_slot, "")
        .unwrap()
        .into_int_value();
    let cmp = builder
        .build_int_compare(inkwell::IntPredicate::ULT, i, len, "")
        .unwrap();
    builder
        .build_conditional_branch(cmp, body_b, exit_b)
        .unwrap();

    // body: c = data[i]; putchar((i32) c); i = i + 1; back to cond
    builder.position_at_end(body_b);
    let i_now = builder
        .build_load(i64_t, i_slot, "")
        .unwrap()
        .into_int_value();
    let p = unsafe {
        builder
            .build_in_bounds_gep(i8_t, data, &[i_now], "")
            .unwrap()
    };
    let c = builder.build_load(i8_t, p, "").unwrap().into_int_value();
    let c32 = builder.build_int_z_extend(c, i32_t, "").unwrap();
    builder.build_call(putchar, &[c32.into()], "").unwrap();
    let next = builder
        .build_int_add(i_now, i64_t.const_int(1, false), "")
        .unwrap();
    builder.build_store(i_slot, next).unwrap();
    builder.build_unconditional_branch(cond_b).unwrap();

    // exit: putchar('\n'); ret void
    builder.position_at_end(exit_b);
    let newline = i32_t.const_int(b'\n' as u64, false);
    builder.build_call(putchar, &[newline.into()], "").unwrap();
    builder.build_return(None).unwrap();
    f
}

/// Build the body of `print_i64(i64 n)` directly in LLVM IR. Same shape as
/// labs/0002-inkwell-spike's `add_print_i64` — divide-by-10, push digits,
/// putchar them out in reverse, then putchar('\n'). LLVM mem2reg lifts the
/// allocas to SSA values at -O1+.
fn define_print_i64<'ctx>(
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

fn rand_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("{nanos:x}")
}
