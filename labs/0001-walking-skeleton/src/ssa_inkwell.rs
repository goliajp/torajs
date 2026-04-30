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

use inkwell::{FloatPredicate, IntPredicate};
use inkwell::OptimizationLevel;
use inkwell::context::Context;
use inkwell::module::Module as LlvmModule;
use inkwell::passes::PassBuilderOptions;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
};
use inkwell::AddressSpace;
use inkwell::types::{BasicMetadataTypeEnum, BasicTypeEnum, FunctionType};
use inkwell::values::{BasicMetadataValueEnum, BasicValueEnum, FunctionValue, IntValue};

use crate::ssa::{self as s, BinOp, FPred, IPred, InstKind, Module, Operand, Terminator, Type};

#[derive(Debug)]
pub enum CompileError {
    Verify(String),
    Pass(String),
    Emit(String),
    Link(String),
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
pub fn compile(ssa_module: &Module, out_path: &Path, opt: &str) -> Result<(), CompileError> {
    let ctx = Context::create();
    let llvm_module = ctx.create_module("torajs");
    let builder = ctx.create_builder();

    // Pass A: declare libc decls + the intrinsics whose body the backend owns.
    let putchar = declare_putchar(&ctx, &llvm_module);
    let malloc = declare_malloc(&ctx, &llvm_module);
    let memcpy = declare_memcpy(&ctx, &llvm_module);
    let memcmp = declare_memcmp(&ctx, &llvm_module);

    // Pass B: emit string-literal globals (LLVM `[N x i8]` private constants).
    // Indexed by StringId so callsites resolve via slice indexing.
    let string_globals: Vec<inkwell::values::GlobalValue> = ssa_module
        .strings
        .iter()
        .enumerate()
        .map(|(i, bytes)| emit_string_global(&ctx, &llvm_module, i, bytes))
        .collect();

    // Pass C: walk every SSA function and create a corresponding LLVM
    // FunctionValue. Backend-owned intrinsics get a body here; everything
    // else gets a declaration that pass D fills in.
    let free = declare_free(&ctx, &llvm_module);
    let realloc = declare_realloc(&ctx, &llvm_module);
    let mut fn_map: Vec<FunctionValue> = Vec::with_capacity(ssa_module.funcs.len());
    for f in &ssa_module.funcs {
        let llvm_fn = match f.name.as_str() {
            "print_i64" => define_print_i64(&ctx, &llvm_module, putchar),
            "print_f64" => define_print_f64(&ctx, &llvm_module),
            "print_bool" => define_print_bool(&ctx, &llvm_module, putchar),
            "__torajs_str_alloc" => {
                define_str_alloc(&ctx, &llvm_module, malloc, memcpy)
            }
            "__torajs_str_print" => define_str_print(&ctx, &llvm_module, putchar),
            "__torajs_str_drop" => define_str_drop(&ctx, &llvm_module, free),
            "__torajs_str_concat" => {
                define_str_concat(&ctx, &llvm_module, malloc, memcpy)
            }
            "__torajs_obj_alloc" => define_obj_alloc(&ctx, &llvm_module, malloc),
            "__torajs_obj_drop" => define_obj_drop(&ctx, &llvm_module, free),
            "__torajs_arr_alloc" => define_arr_alloc(&ctx, &llvm_module, malloc),
            "__torajs_arr_push" => define_arr_push(&ctx, &llvm_module, realloc),
            "__torajs_arr_reserve" => {
                define_arr_reserve(&ctx, &llvm_module, realloc)
            }
            "__torajs_arr_push_unchecked" => {
                define_arr_push_unchecked(&ctx, &llvm_module)
            }
            "__torajs_arr_drop" => define_arr_drop(&ctx, &llvm_module, free),
            "__torajs_str_slice" => {
                define_str_slice(&ctx, &llvm_module, malloc, memcpy)
            }
            "__torajs_str_char_code_at" => {
                define_str_char_code_at(&ctx, &llvm_module)
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
            "__torajs_str_index_of" => {
                define_str_index_of(&ctx, &llvm_module, memcmp)
            }
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
            "__torajs_throw_set" => {
                define_throw_set(&ctx, &llvm_module)
            }
            "__torajs_throw_check" => {
                define_throw_check(&ctx, &llvm_module)
            }
            "__torajs_throw_take" => {
                define_throw_take(&ctx, &llvm_module)
            }
            _ => declare_ssa_fn(&ctx, &llvm_module, f),
        };
        fn_map.push(llvm_fn);
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
        "__torajs_throw_set",
        "__torajs_throw_check",
        "__torajs_throw_take",
    ];
    for (i, f) in ssa_module.funcs.iter().enumerate() {
        if f.is_declaration() || intrinsics.contains(&f.name.as_str()) {
            continue;
        }
        let lower = FnLower {
            ctx: &ctx,
            builder: &builder,
            ssa_fn: f,
            llvm_fn: fn_map[i],
            fn_map: &fn_map,
            string_globals: &string_globals,
            ssa_module,
            block_map: HashMap::new(),
            value_map: HashMap::new(),
        };
        lower.run();
    }

    // Pass D: verify, optimize, emit, link.
    if let Err(e) = llvm_module.verify() {
        return Err(CompileError::Verify(e.to_string()));
    }

    Target::initialize_aarch64(&InitializationConfig::default());
    let triple = TargetMachine::get_default_triple();
    let target = Target::from_triple(&triple).map_err(|e| CompileError::Emit(e.to_string()))?;
    let cpu = TargetMachine::get_host_cpu_name().to_string();
    let features = TargetMachine::get_host_cpu_features().to_string();
    let machine = target
        .create_target_machine(
            &triple,
            &cpu,
            &features,
            OptimizationLevel::Less,
            RelocMode::PIC,
            CodeModel::Default,
        )
        .ok_or_else(|| CompileError::Emit("create_target_machine returned None".into()))?;

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

    // M6.1+ — torajs's C runtime. Pieces that are clearer in C than via
    // the inkwell IR-builder API (string split, array join, anything
    // future where IR builder verbosity outweighs the link-cost gain).
    // Embedded via include_str! and recompiled fresh per `tr build`;
    // adds ~10-30 ms to the AOT pipeline (negligible vs LLVM optimize).
    let c_runtime_src: &str = include_str!("runtime_str.c");
    let c_src_path: PathBuf = std::env::temp_dir().join(format!(
        "torajs-runtime-{}-{}.c",
        std::process::id(),
        rand_suffix()
    ));
    let c_obj_path: PathBuf = std::env::temp_dir().join(format!(
        "torajs-runtime-{}-{}.o",
        std::process::id(),
        rand_suffix()
    ));
    std::fs::write(&c_src_path, c_runtime_src)
        .map_err(|e| CompileError::Link(format!("write runtime.c: {e}")))?;
    let cc_status = Command::new("cc")
        .args(["-c", "-O2", "-o"])
        .arg(&c_obj_path)
        .arg(&c_src_path)
        .status()
        .map_err(|e| CompileError::Link(format!("spawning cc -c: {e}")))?;
    if !cc_status.success() {
        let _ = std::fs::remove_file(&c_src_path);
        return Err(CompileError::Link(format!(
            "cc -c runtime.c exited {cc_status}"
        )));
    }

    let status = Command::new("cc")
        .arg(&obj_path)
        .arg(&c_obj_path)
        .arg("-o")
        .arg(out_path)
        .status()
        .map_err(|e| CompileError::Link(format!("spawning cc: {e}")))?;
    let _ = std::fs::remove_file(&obj_path);
    let _ = std::fs::remove_file(&c_src_path);
    let _ = std::fs::remove_file(&c_obj_path);
    if !status.success() {
        return Err(CompileError::Link(format!("cc exited {status}")));
    }
    Ok(())
}

fn declare_putchar<'ctx>(ctx: &'ctx Context, m: &LlvmModule<'ctx>) -> FunctionValue<'ctx> {
    let i32_t = ctx.i32_type();
    let fn_t = i32_t.fn_type(&[i32_t.into()], false);
    m.add_function("putchar", fn_t, None)
}

fn declare_malloc<'ctx>(ctx: &'ctx Context, m: &LlvmModule<'ctx>) -> FunctionValue<'ctx> {
    let i64_t = ctx.i64_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let fn_t = ptr_t.fn_type(&[i64_t.into()], false);
    m.add_function("malloc", fn_t, None)
}

fn declare_realloc<'ctx>(ctx: &'ctx Context, m: &LlvmModule<'ctx>) -> FunctionValue<'ctx> {
    let i64_t = ctx.i64_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    // void* realloc(void *p, size_t new_size)
    let fn_t = ptr_t.fn_type(&[ptr_t.into(), i64_t.into()], false);
    m.add_function("realloc", fn_t, None)
}

fn declare_memcpy<'ctx>(ctx: &'ctx Context, m: &LlvmModule<'ctx>) -> FunctionValue<'ctx> {
    let i64_t = ctx.i64_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    // void* memcpy(void *dst, const void *src, size_t n)  — return ignored
    let fn_t = ptr_t.fn_type(&[ptr_t.into(), ptr_t.into(), i64_t.into()], false);
    m.add_function("memcpy", fn_t, None)
}

fn declare_free<'ctx>(ctx: &'ctx Context, m: &LlvmModule<'ctx>) -> FunctionValue<'ctx> {
    let void_t = ctx.void_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let fn_t = void_t.fn_type(&[ptr_t.into()], false);
    m.add_function("free", fn_t, None)
}

fn declare_memcmp<'ctx>(ctx: &'ctx Context, m: &LlvmModule<'ctx>) -> FunctionValue<'ctx> {
    let i32_t = ctx.i32_type();
    let i64_t = ctx.i64_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    // int memcmp(const void *a, const void *b, size_t n)
    let fn_t = i32_t.fn_type(&[ptr_t.into(), ptr_t.into(), i64_t.into()], false);
    m.add_function("memcmp", fn_t, None)
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

/// `__torajs_str_alloc(*const u8 src, u64 len) -> *StrRepr`
///
/// Build:
///     p = malloc(8 + len)
///     *(u64*)p = len
///     memcpy(p + 8, src, len)
///     return p
///
/// The returned pointer is a heap StrRepr. Caller's drop frees it.
fn define_str_alloc<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    malloc: FunctionValue<'ctx>,
    memcpy: FunctionValue<'ctx>,
) -> FunctionValue<'ctx> {
    let builder = ctx.create_builder();
    let i64_t = ctx.i64_type();
    let i8_t = ctx.i8_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let fn_t = ptr_t.fn_type(&[ptr_t.into(), i64_t.into()], false);
    let f = m.add_function("__torajs_str_alloc", fn_t, None);
    let entry = ctx.append_basic_block(f, "entry");
    builder.position_at_end(entry);

    let src = f.get_nth_param(0).unwrap().into_pointer_value();
    let len = f.get_nth_param(1).unwrap().into_int_value();

    let total = builder
        .build_int_add(len, i64_t.const_int(8, false), "total")
        .unwrap();
    let p = builder
        .build_call(malloc, &[total.into()], "p")
        .unwrap()
        .try_as_basic_value()
        .unwrap_basic()
        .into_pointer_value();

    // Store len at offset 0
    builder.build_store(p, len).unwrap();

    // Compute data pointer = p + 8 (byte offset)
    let data = unsafe {
        builder
            .build_in_bounds_gep(i8_t, p, &[i64_t.const_int(8, false)], "data")
            .unwrap()
    };
    builder
        .build_call(memcpy, &[data.into(), src.into(), len.into()], "_cp")
        .unwrap();

    builder.build_return(Some(&p)).unwrap();
    f
}

/// `__torajs_str_concat(*StrRepr a, *StrRepr b) -> *StrRepr`
///
/// Build:
///     a_len = *(u64*)a
///     b_len = *(u64*)b
///     total = a_len + b_len
///     p = malloc(8 + total)
///     *(u64*)p = total
///     memcpy(p + 8, a + 8, a_len)
///     memcpy(p + 8 + a_len, b + 8, b_len)
///     return p
///
/// TS-shape: read-only on operands. `a` and `b` keep their heaps —
/// caller's drops still fire normally on them.
fn define_str_concat<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    malloc: FunctionValue<'ctx>,
    memcpy: FunctionValue<'ctx>,
) -> FunctionValue<'ctx> {
    let builder = ctx.create_builder();
    let i64_t = ctx.i64_type();
    let i8_t = ctx.i8_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let fn_t = ptr_t.fn_type(&[ptr_t.into(), ptr_t.into()], false);
    let f = m.add_function("__torajs_str_concat", fn_t, None);
    let entry = ctx.append_basic_block(f, "entry");
    builder.position_at_end(entry);

    let a = f.get_nth_param(0).unwrap().into_pointer_value();
    let b = f.get_nth_param(1).unwrap().into_pointer_value();

    let a_len = builder.build_load(i64_t, a, "a_len").unwrap().into_int_value();
    let b_len = builder.build_load(i64_t, b, "b_len").unwrap().into_int_value();
    let total = builder.build_int_add(a_len, b_len, "total").unwrap();

    let alloc_size = builder
        .build_int_add(total, i64_t.const_int(8, false), "alloc_size")
        .unwrap();
    let p = builder
        .build_call(malloc, &[alloc_size.into()], "p")
        .unwrap()
        .try_as_basic_value()
        .unwrap_basic()
        .into_pointer_value();

    // Store total len at offset 0
    builder.build_store(p, total).unwrap();

    // p_data = p + 8
    let p_data = unsafe {
        builder
            .build_in_bounds_gep(i8_t, p, &[i64_t.const_int(8, false)], "p_data")
            .unwrap()
    };
    // a_data = a + 8
    let a_data = unsafe {
        builder
            .build_in_bounds_gep(i8_t, a, &[i64_t.const_int(8, false)], "a_data")
            .unwrap()
    };
    // memcpy(p_data, a_data, a_len)
    builder
        .build_call(memcpy, &[p_data.into(), a_data.into(), a_len.into()], "_cp_a")
        .unwrap();
    // p_data2 = p_data + a_len
    let p_data2 = unsafe {
        builder
            .build_in_bounds_gep(i8_t, p_data, &[a_len], "p_data2")
            .unwrap()
    };
    // b_data = b + 8
    let b_data = unsafe {
        builder
            .build_in_bounds_gep(i8_t, b, &[i64_t.const_int(8, false)], "b_data")
            .unwrap()
    };
    // memcpy(p_data2, b_data, b_len)
    builder
        .build_call(memcpy, &[p_data2.into(), b_data.into(), b_len.into()], "_cp_b")
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
    malloc: FunctionValue<'ctx>,
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

    let len = builder.build_load(i64_t, s, "len").unwrap().into_int_value();
    // start = max(0, min(start, len))
    let start_neg = builder
        .build_int_compare(IntPredicate::SLT, start, zero, "start_neg")
        .unwrap();
    let start_after_lo = builder
        .build_select(start_neg, zero, start, "start_lo")
        .unwrap()
        .into_int_value();
    let start_over = builder
        .build_int_compare(IntPredicate::SGT, start_after_lo, len, "start_over")
        .unwrap();
    let start_c = builder
        .build_select(start_over, len, start_after_lo, "start_c")
        .unwrap()
        .into_int_value();
    // end_lo = max(start_c, end)
    let end_under = builder
        .build_int_compare(IntPredicate::SLT, end, start_c, "end_under")
        .unwrap();
    let end_after_lo = builder
        .build_select(end_under, start_c, end, "end_lo")
        .unwrap()
        .into_int_value();
    let end_over = builder
        .build_int_compare(IntPredicate::SGT, end_after_lo, len, "end_over")
        .unwrap();
    let end_c = builder
        .build_select(end_over, len, end_after_lo, "end_c")
        .unwrap()
        .into_int_value();

    let new_len = builder
        .build_int_sub(end_c, start_c, "new_len")
        .unwrap();
    let alloc_size = builder
        .build_int_add(new_len, i64_t.const_int(8, false), "alloc_size")
        .unwrap();
    let p = builder
        .build_call(malloc, &[alloc_size.into()], "p")
        .unwrap()
        .try_as_basic_value()
        .unwrap_basic()
        .into_pointer_value();
    builder.build_store(p, new_len).unwrap();
    let p_data = unsafe {
        builder
            .build_in_bounds_gep(i8_t, p, &[i64_t.const_int(8, false)], "p_data")
            .unwrap()
    };
    let s_off = builder
        .build_int_add(start_c, i64_t.const_int(8, false), "s_off")
        .unwrap();
    let s_data = unsafe {
        builder
            .build_in_bounds_gep(i8_t, s, &[s_off], "s_data")
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
fn define_str_char_code_at<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
) -> FunctionValue<'ctx> {
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
    let len = builder.build_load(i64_t, s, "len").unwrap().into_int_value();
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
    let off = builder
        .build_int_add(i, i64_t.const_int(8, false), "off")
        .unwrap();
    let p = unsafe {
        builder
            .build_in_bounds_gep(i8_t, s, &[off], "p")
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
    let i64_t = ctx.i64_type();
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
    let s_len = builder.build_load(i64_t, s, "s_len").unwrap().into_int_value();
    let n_len = builder.build_load(i64_t, n, "n_len").unwrap().into_int_value();
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
    // s_off = 8 + (from_end ? s_len - n_len : 0)
    let s_off_pre = if from_end {
        let diff = builder
            .build_int_sub(s_len, n_len, "diff")
            .unwrap();
        builder
            .build_int_add(diff, i64_t.const_int(8, false), "s_off")
            .unwrap()
    } else {
        i64_t.const_int(8, false)
    };
    let s_data = unsafe {
        builder
            .build_in_bounds_gep(i8_t, s, &[s_off_pre], "s_data")
            .unwrap()
    };
    let n_data = unsafe {
        builder
            .build_in_bounds_gep(i8_t, n, &[i64_t.const_int(8, false)], "n_data")
            .unwrap()
    };
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
    let s_len = builder.build_load(i64_t, s, "s_len").unwrap().into_int_value();
    let sub_len = builder
        .build_load(i64_t, sub, "sub_len")
        .unwrap()
        .into_int_value();
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
    let max_i = builder
        .build_int_sub(s_len, sub_len, "max_i")
        .unwrap();
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

    // cmp: memcmp(s+8+i, sub+8, sub_len) == 0 ?
    builder.position_at_end(cmp_blk);
    let i_now2 = builder
        .build_load(i64_t, i_slot, "i_now2")
        .unwrap()
        .into_int_value();
    let s_off = builder
        .build_int_add(i_now2, i64_t.const_int(8, false), "s_off")
        .unwrap();
    let s_data = unsafe {
        builder
            .build_in_bounds_gep(i8_t, s, &[s_off], "s_data")
            .unwrap()
    };
    let sub_data = unsafe {
        builder
            .build_in_bounds_gep(i8_t, sub, &[i64_t.const_int(8, false)], "sub_data")
            .unwrap()
    };
    let r = builder
        .build_call(memcmp, &[s_data.into(), sub_data.into(), sub_len.into()], "r")
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
            .build_call(
                putchar,
                &[i32_t.const_int(ch as u64, false).into()],
                "",
            )
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

/// `print_f64(f64) -> void` — printf("%g\n", x). Uses libc printf for
/// formatting since Rust's float formatter would require recursive lib
/// calls we don't want at AOT time. `%g` matches Rust's `{}` formatter
/// for most values; minor edge-case divergence (-0 vs 0, NaN sign) is
/// acceptable for bench output.
fn define_print_f64<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
) -> FunctionValue<'ctx> {
    let i32_t = ctx.i32_type();
    let f64_t = ctx.f64_type();
    let void_t = ctx.void_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    // printf is variadic; declare with a single ptr param + variadic.
    let printf_t = i32_t.fn_type(&[ptr_t.into()], true);
    let printf = m
        .get_function("printf")
        .unwrap_or_else(|| m.add_function("printf", printf_t, None));

    // Format string global "%g\n\0".
    let fmt_bytes = b"%g\n\0";
    let arr_t = ctx.i8_type().array_type(fmt_bytes.len() as u32);
    let arr = ctx.const_string(fmt_bytes, false);
    let g = m.add_global(arr_t, None, ".f64fmt");
    g.set_initializer(&arr);
    g.set_constant(true);
    g.set_linkage(inkwell::module::Linkage::Private);
    g.set_unnamed_addr(true);

    let fn_t = void_t.fn_type(&[f64_t.into()], false);
    let f = m.add_function("print_f64", fn_t, None);
    let entry = ctx.append_basic_block(f, "entry");
    let builder = ctx.create_builder();
    builder.position_at_end(entry);
    let arg = f.get_nth_param(0).unwrap().into_float_value();
    builder
        .build_call(
            printf,
            &[g.as_pointer_value().into(), arg.into()],
            "_p",
        )
        .unwrap();
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

/// M4 — exception state. Two module-level i64 globals
/// (`__torajs_throw_active`, `__torajs_throw_value`) plus three runtime
/// helpers operating on them. Lowered code calls these (never the
/// globals directly) so the same shape works in both AOT (LLVM IR) and
/// JIT (Rust extern "C" fns over thread-local statics).
fn ensure_throw_globals<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
) -> (
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
    let value = match m.get_global("__torajs_throw_value") {
        Some(g) => g,
        None => {
            let g = m.add_global(i64_t, None, "__torajs_throw_value");
            g.set_initializer(&i64_t.const_int(0, false));
            g
        }
    };
    (active, value)
}

fn define_throw_set<'ctx>(ctx: &'ctx Context, m: &LlvmModule<'ctx>) -> FunctionValue<'ctx> {
    let (active_g, value_g) = ensure_throw_globals(ctx, m);
    let builder = ctx.create_builder();
    let i64_t = ctx.i64_type();
    let void_t = ctx.void_type();
    let fn_t = void_t.fn_type(&[i64_t.into()], false);
    let f = m.add_function("__torajs_throw_set", fn_t, None);
    let entry = ctx.append_basic_block(f, "entry");
    builder.position_at_end(entry);
    let v = f.get_nth_param(0).unwrap().into_int_value();
    builder
        .build_store(active_g.as_pointer_value(), i64_t.const_int(1, false))
        .unwrap();
    builder.build_store(value_g.as_pointer_value(), v).unwrap();
    builder.build_return(None).unwrap();
    f
}

fn define_throw_check<'ctx>(ctx: &'ctx Context, m: &LlvmModule<'ctx>) -> FunctionValue<'ctx> {
    let (active_g, _) = ensure_throw_globals(ctx, m);
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
    let (active_g, value_g) = ensure_throw_globals(ctx, m);
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

/// `__torajs_obj_alloc(u64 size) -> *void` — straight `malloc(size)`.
/// Used by ObjectLit lowering; lowerer passes the static struct size.
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

/// `__torajs_obj_drop(*void p) -> void` — straight `free(p)`. P2.4.c MVP
/// only supports objects with Copy or Str fields. P2.4.d will recursively
/// drop non-Copy fields (Strings, nested objects) before freeing the
/// outer struct — that's where the runtime needs the layout info.
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
///     total = 16 + initial_cap * 8
///     p = malloc(total)
///     *(u64*)p       = 0              // len = 0
///     *((u64*)p + 1) = initial_cap    // cap
///     return p
fn define_arr_alloc<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    malloc: FunctionValue<'ctx>,
) -> FunctionValue<'ctx> {
    let builder = ctx.create_builder();
    let i64_t = ctx.i64_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let fn_t = ptr_t.fn_type(&[i64_t.into()], false);
    let f = m.add_function("__torajs_arr_alloc", fn_t, None);
    let entry = ctx.append_basic_block(f, "entry");
    builder.position_at_end(entry);

    let cap = f.get_nth_param(0).unwrap().into_int_value();
    let cap_bytes = builder
        .build_int_mul(cap, i64_t.const_int(8, false), "cap_bytes")
        .unwrap();
    let total = builder
        .build_int_add(cap_bytes, i64_t.const_int(16, false), "total")
        .unwrap();
    let p = builder
        .build_call(malloc, &[total.into()], "p")
        .unwrap()
        .try_as_basic_value()
        .unwrap_basic()
        .into_pointer_value();
    // len = 0 at offset 0
    builder.build_store(p, i64_t.const_int(0, false)).unwrap();
    // cap at offset 8
    let cap_ptr = unsafe {
        builder
            .build_in_bounds_gep(i64_t, p, &[i64_t.const_int(1, false)], "cap_p")
            .unwrap()
    };
    builder.build_store(cap_ptr, cap).unwrap();
    builder.build_return(Some(&p)).unwrap();
    f
}

/// `__torajs_arr_push(*u8 arr, i64 val) -> *u8`
///
///     len = *(u64*)arr
///     cap = *((u64*)arr + 1)
///     if len == cap:
///       new_cap = cap == 0 ? 4 : cap * 2
///       new_total = 16 + new_cap * 8
///       arr = realloc(arr, new_total)
///       *((u64*)arr + 1) = new_cap
///     // store at offset 16 + len*8
///     *(i64*)(arr + 16 + len*8) = val
///     *(u64*)arr = len + 1
///     return arr
fn define_arr_push<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    realloc: FunctionValue<'ctx>,
) -> FunctionValue<'ctx> {
    let builder = ctx.create_builder();
    let i64_t = ctx.i64_type();
    let i8_t = ctx.i8_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let fn_t = ptr_t.fn_type(&[ptr_t.into(), i64_t.into()], false);
    let f = m.add_function("__torajs_arr_push", fn_t, None);
    let entry = ctx.append_basic_block(f, "entry");
    let grow_blk = ctx.append_basic_block(f, "grow");
    let store_blk = ctx.append_basic_block(f, "store");
    builder.position_at_end(entry);

    let arr_in = f.get_nth_param(0).unwrap().into_pointer_value();
    let val = f.get_nth_param(1).unwrap().into_int_value();

    let len = builder
        .build_load(i64_t, arr_in, "len")
        .unwrap()
        .into_int_value();
    let cap_ptr_in = unsafe {
        builder
            .build_in_bounds_gep(i64_t, arr_in, &[i64_t.const_int(1, false)], "cap_p")
            .unwrap()
    };
    let cap = builder
        .build_load(i64_t, cap_ptr_in, "cap")
        .unwrap()
        .into_int_value();
    let need_grow = builder
        .build_int_compare(IntPredicate::EQ, len, cap, "need_grow")
        .unwrap();
    builder
        .build_conditional_branch(need_grow, grow_blk, store_blk)
        .unwrap();

    // grow_blk: realloc with new_cap = (cap == 0 ? 4 : cap*2)
    builder.position_at_end(grow_blk);
    let cap_zero = builder
        .build_int_compare(IntPredicate::EQ, cap, i64_t.const_int(0, false), "cap_zero")
        .unwrap();
    let cap_x2 = builder
        .build_int_mul(cap, i64_t.const_int(2, false), "cap_x2")
        .unwrap();
    let new_cap = builder
        .build_select(
            cap_zero,
            i64_t.const_int(4, false),
            cap_x2,
            "new_cap",
        )
        .unwrap()
        .into_int_value();
    let new_cap_bytes = builder
        .build_int_mul(new_cap, i64_t.const_int(8, false), "new_cap_bytes")
        .unwrap();
    let new_total = builder
        .build_int_add(new_cap_bytes, i64_t.const_int(16, false), "new_total")
        .unwrap();
    let arr_grown = builder
        .build_call(realloc, &[arr_in.into(), new_total.into()], "arr_grown")
        .unwrap()
        .try_as_basic_value()
        .unwrap_basic()
        .into_pointer_value();
    let new_cap_p = unsafe {
        builder
            .build_in_bounds_gep(i64_t, arr_grown, &[i64_t.const_int(1, false)], "new_cap_p")
            .unwrap()
    };
    builder.build_store(new_cap_p, new_cap).unwrap();
    builder.build_unconditional_branch(store_blk).unwrap();

    // store_blk: phi the array pointer (entry → arr_in, grow → arr_grown).
    builder.position_at_end(store_blk);
    let phi = builder.build_phi(ptr_t, "arr").unwrap();
    phi.add_incoming(&[(&arr_in, entry), (&arr_grown, grow_blk)]);
    let arr = phi.as_basic_value().into_pointer_value();
    // slot_off = 16 + len * 8
    let len_x8 = builder
        .build_int_mul(len, i64_t.const_int(8, false), "len_x8")
        .unwrap();
    let slot_off = builder
        .build_int_add(len_x8, i64_t.const_int(16, false), "slot_off")
        .unwrap();
    let slot = unsafe {
        builder
            .build_in_bounds_gep(i8_t, arr, &[slot_off], "slot")
            .unwrap()
    };
    builder.build_store(slot, val).unwrap();
    let len_p1 = builder
        .build_int_add(len, i64_t.const_int(1, false), "len_p1")
        .unwrap();
    builder.build_store(arr, len_p1).unwrap();
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
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let fn_t = ptr_t.fn_type(&[ptr_t.into(), i64_t.into()], false);
    let f = m.add_function("__torajs_arr_reserve", fn_t, None);
    let entry = ctx.append_basic_block(f, "entry");
    let grow_blk = ctx.append_basic_block(f, "grow");
    let exit_blk = ctx.append_basic_block(f, "exit");
    builder.position_at_end(entry);
    let arr_in = f.get_nth_param(0).unwrap().into_pointer_value();
    let new_cap = f.get_nth_param(1).unwrap().into_int_value();
    let cap_p = unsafe {
        builder
            .build_in_bounds_gep(i64_t, arr_in, &[i64_t.const_int(1, false)], "cap_p")
            .unwrap()
    };
    let cap = builder
        .build_load(i64_t, cap_p, "cap")
        .unwrap()
        .into_int_value();
    let need_grow = builder
        .build_int_compare(IntPredicate::ULT, cap, new_cap, "need_grow")
        .unwrap();
    builder
        .build_conditional_branch(need_grow, grow_blk, exit_blk)
        .unwrap();
    // grow: realloc(p, 16 + new_cap * 8); store new_cap; pass to exit
    builder.position_at_end(grow_blk);
    let new_bytes = builder
        .build_int_mul(new_cap, i64_t.const_int(8, false), "")
        .unwrap();
    let new_total = builder
        .build_int_add(new_bytes, i64_t.const_int(16, false), "")
        .unwrap();
    let arr_grown = builder
        .build_call(realloc, &[arr_in.into(), new_total.into()], "arr_grown")
        .unwrap()
        .try_as_basic_value()
        .unwrap_basic()
        .into_pointer_value();
    let new_cap_p = unsafe {
        builder
            .build_in_bounds_gep(i64_t, arr_grown, &[i64_t.const_int(1, false)], "")
            .unwrap()
    };
    builder.build_store(new_cap_p, new_cap).unwrap();
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
    let len = builder
        .build_load(i64_t, arr, "len")
        .unwrap()
        .into_int_value();
    let len_x8 = builder
        .build_int_mul(len, i64_t.const_int(8, false), "")
        .unwrap();
    let slot_off = builder
        .build_int_add(len_x8, i64_t.const_int(16, false), "")
        .unwrap();
    let slot = unsafe {
        builder
            .build_in_bounds_gep(i8_t, arr, &[slot_off], "slot")
            .unwrap()
    };
    builder.build_store(slot, val).unwrap();
    let len_p1 = builder
        .build_int_add(len, i64_t.const_int(1, false), "")
        .unwrap();
    builder.build_store(arr, len_p1).unwrap();
    builder.build_return(None).unwrap();
    f
}

/// `__torajs_arr_drop(*u8 arr) -> void` — `free(arr)`. Caller has
/// already dropped non-Copy elements (M1.2 MVP only handles i64
/// elements which need no inner drop).
fn define_arr_drop<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    free: FunctionValue<'ctx>,
) -> FunctionValue<'ctx> {
    let builder = ctx.create_builder();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let void_t = ctx.void_type();
    let fn_t = void_t.fn_type(&[ptr_t.into()], false);
    let f = m.add_function("__torajs_arr_drop", fn_t, None);
    let entry = ctx.append_basic_block(f, "entry");
    builder.position_at_end(entry);
    let arg = f.get_nth_param(0).unwrap().into_pointer_value();
    builder.build_call(free, &[arg.into()], "_f").unwrap();
    builder.build_return(None).unwrap();
    f
}

/// `__torajs_str_drop(*StrRepr s) -> void` — `free(s)`. The runtime owns
/// the layout decision (see __torajs_str_alloc); free works because alloc
/// used libc malloc.
fn define_str_drop<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    free: FunctionValue<'ctx>,
) -> FunctionValue<'ctx> {
    let builder = ctx.create_builder();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let void_t = ctx.void_type();
    let fn_t = void_t.fn_type(&[ptr_t.into()], false);
    let f = m.add_function("__torajs_str_drop", fn_t, None);
    let entry = ctx.append_basic_block(f, "entry");
    builder.position_at_end(entry);
    let arg = f.get_nth_param(0).unwrap().into_pointer_value();
    builder.build_call(free, &[arg.into()], "_f").unwrap();
    builder.build_return(None).unwrap();
    f
}

/// `__torajs_str_print(*StrRepr s) -> void`
///
///     len = *(u64*)s
///     write(1 /*stdout*/, s + 8, len)
///     write(1, "\n", 1)
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
    let cond_b = ctx.append_basic_block(f, "cond");
    let body_b = ctx.append_basic_block(f, "body");
    let exit_b = ctx.append_basic_block(f, "exit");
    builder.position_at_end(entry);

    let s = f.get_nth_param(0).unwrap().into_pointer_value();

    // load len from offset 0
    let len = builder
        .build_load(i64_t, s, "len")
        .unwrap()
        .into_int_value();
    let data = unsafe {
        builder
            .build_in_bounds_gep(i8_t, s, &[i64_t.const_int(8, false)], "data")
            .unwrap()
    };
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
    builder.build_conditional_branch(cmp, body_b, exit_b).unwrap();

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
    let c = builder
        .build_load(i8_t, p, "")
        .unwrap()
        .into_int_value();
    let c32 = builder
        .build_int_z_extend(c, i32_t, "")
        .unwrap();
    builder
        .build_call(putchar, &[c32.into()], "")
        .unwrap();
    let next = builder
        .build_int_add(i_now, i64_t.const_int(1, false), "")
        .unwrap();
    builder.build_store(i_slot, next).unwrap();
    builder.build_unconditional_branch(cond_b).unwrap();

    // exit: putchar('\n'); ret void
    builder.position_at_end(exit_b);
    let newline = i32_t.const_int(b'\n' as u64, false);
    builder
        .build_call(putchar, &[newline.into()], "")
        .unwrap();
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
    let neg_arg = builder
        .build_int_neg(arg, "neg_arg")
        .unwrap();
    builder.build_store(n_a, neg_arg).unwrap();
    builder.build_unconditional_branch(loop1).unwrap();

    builder.position_at_end(zero_blk);
    let zero_ch = i32_t.const_int(b'0' as u64, false);
    builder.build_call(putchar, &[zero_ch.into()], "_z").unwrap();
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
) -> FunctionValue<'ctx> {
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
        Type::Ptr | Type::Str | Type::Obj(_) | Type::Arr(_) | Type::FnSig(_) | Type::Closure(_) => {
            ctx.ptr_type(AddressSpace::default()).fn_type(&param_metas, false)
        }
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
        Type::Ptr | Type::Str | Type::Obj(_) | Type::Arr(_) | Type::FnSig(_) | Type::Closure(_) => {
            ctx.ptr_type(AddressSpace::default()).into()
        }
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
        Type::Ptr | Type::Str | Type::Obj(_) | Type::Arr(_) | Type::FnSig(_) | Type::Closure(_) => {
            ctx.ptr_type(AddressSpace::default()).into()
        }
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
    /// Whole SSA module — needed by `InstKind::CallIndirect` to look up
    /// the signature interner. Read-only; no mutation. M2 Phase B Stage 3.
    ssa_module: &'a s::Module,
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
        for b in &self.ssa_fn.blocks {
            let bb = self.block_map[&b.id.0];
            self.builder.position_at_end(bb);
            for inst in &b.insts {
                self.lower_inst(inst);
            }
            self.lower_term(&b.term);
        }
    }

    fn lower_inst(&mut self, inst: &s::Inst) {
        let result_val = match &inst.kind {
            InstKind::BinOp(op, a, b) => {
                let r: BasicValueEnum = match op {
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::SDiv | BinOp::SRem
                    | BinOp::And | BinOp::Or | BinOp::Xor | BinOp::Shl | BinOp::AShr | BinOp::LShr => {
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
                            BinOp::AShr => self.builder.build_right_shift(av, bv, true, "").unwrap(),
                            BinOp::LShr => self.builder.build_right_shift(av, bv, false, "").unwrap(),
                            _ => unreachable!(),
                        };
                        BasicValueEnum::IntValue(r)
                    }
                    BinOp::FAdd | BinOp::FSub | BinOp::FMul | BinOp::FDiv => {
                        let av = self.operand(a).into_float_value();
                        let bv = self.operand(b).into_float_value();
                        let r = match op {
                            BinOp::FAdd => self.builder.build_float_add(av, bv, "").unwrap(),
                            BinOp::FSub => self.builder.build_float_sub(av, bv, "").unwrap(),
                            BinOp::FMul => self.builder.build_float_mul(av, bv, "").unwrap(),
                            BinOp::FDiv => self.builder.build_float_div(av, bv, "").unwrap(),
                            _ => unreachable!(),
                        };
                        BasicValueEnum::FloatValue(r)
                    }
                };
                Some(r)
            }
            InstKind::ICmp(p, a, b) => {
                let av = self.operand_int(a);
                let bv = self.operand_int(b);
                let pred = match p {
                    IPred::Eq => IntPredicate::EQ,
                    IPred::Ne => IntPredicate::NE,
                    IPred::Slt => IntPredicate::SLT,
                    IPred::Sgt => IntPredicate::SGT,
                    IPred::Sle => IntPredicate::SLE,
                    IPred::Sge => IntPredicate::SGE,
                };
                let r = self.builder.build_int_compare(pred, av, bv, "").unwrap();
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
                };
                let r = self.builder.build_float_compare(pred, av, bv, "").unwrap();
                Some(BasicValueEnum::IntValue(r))
            }
            InstKind::SiToFp(op) => {
                let v = self.operand_int(op);
                let f = ctx_f64(self.ctx);
                let r = self
                    .builder
                    .build_signed_int_to_float(v, f, "")
                    .unwrap();
                Some(BasicValueEnum::FloatValue(r))
            }
            InstKind::StringRef(sid) => {
                let g = self.string_globals[sid.0 as usize];
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
                let ptr_t = self.ctx.ptr_type(AddressSpace::default());
                let mut argv: Vec<BasicMetadataValueEnum> =
                    Vec::with_capacity(args.len());
                for (i, a) in args.iter().enumerate() {
                    let raw = self.operand(a);
                    let coerced: BasicValueEnum = if i < expected.len() {
                        match expected[i] {
                            BasicMetadataTypeEnum::IntType(_) => {
                                if let BasicValueEnum::PointerValue(p) = raw {
                                    self.builder
                                        .build_ptr_to_int(p, i64_t, "")
                                        .unwrap()
                                        .into()
                                } else {
                                    raw
                                }
                            }
                            BasicMetadataTypeEnum::PointerType(_) => {
                                if let BasicValueEnum::IntValue(v) = raw {
                                    self.builder
                                        .build_int_to_ptr(v, ptr_t, "")
                                        .unwrap()
                                        .into()
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
                            .build_in_bounds_gep(
                                i8_t,
                                p,
                                &[i64_t.const_int(*offset, false)],
                                "",
                            )
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
                            .build_in_bounds_gep(
                                i8_t,
                                p,
                                &[i64_t.const_int(*offset, false)],
                                "",
                            )
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
                            self.builder
                                .build_int_to_ptr(iv, ptr_t, "")
                                .unwrap()
                                .into()
                        }
                        (BasicValueEnum::PointerValue(pv), Some(rt)) if rt.is_int_type() => {
                            let i64_t = self.ctx.i64_type();
                            self.builder
                                .build_ptr_to_int(pv, i64_t, "")
                                .unwrap()
                                .into()
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
            Operand::ConstF64(n) => {
                BasicValueEnum::FloatValue(self.ctx.f64_type().const_float(*n))
            }
            Operand::ConstBool(b) => {
                BasicValueEnum::IntValue(self.ctx.bool_type().const_int(*b as u64, false))
            }
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
