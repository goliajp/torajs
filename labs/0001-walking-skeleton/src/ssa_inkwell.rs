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
    let write = declare_write(&ctx, &llvm_module);

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
    let mut fn_map: Vec<FunctionValue> = Vec::with_capacity(ssa_module.funcs.len());
    for f in &ssa_module.funcs {
        let llvm_fn = match f.name.as_str() {
            "print_i64" => define_print_i64(&ctx, &llvm_module, putchar),
            "__torajs_str_alloc" => {
                define_str_alloc(&ctx, &llvm_module, malloc, memcpy)
            }
            "__torajs_str_print" => define_str_print(&ctx, &llvm_module, write),
            "__torajs_str_drop" => define_str_drop(&ctx, &llvm_module, free),
            _ => declare_ssa_fn(&ctx, &llvm_module, f),
        };
        fn_map.push(llvm_fn);
    }

    // Pass D: lower bodies for every SSA function that has blocks AND isn't
    // a backend-owned intrinsic.
    let intrinsics = [
        "print_i64",
        "__torajs_str_alloc",
        "__torajs_str_print",
        "__torajs_str_drop",
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

    let status = Command::new("cc")
        .arg(&obj_path)
        .arg("-o")
        .arg(out_path)
        .status()
        .map_err(|e| CompileError::Link(format!("spawning cc: {e}")))?;
    let _ = std::fs::remove_file(&obj_path);
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

fn declare_write<'ctx>(ctx: &'ctx Context, m: &LlvmModule<'ctx>) -> FunctionValue<'ctx> {
    let i32_t = ctx.i32_type();
    let i64_t = ctx.i64_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    // ssize_t write(int fd, const void *buf, size_t count) — return ignored
    let fn_t = i64_t.fn_type(&[i32_t.into(), ptr_t.into(), i64_t.into()], false);
    m.add_function("write", fn_t, None)
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
/// Two write(2) syscalls. Should fold to a single in LLVM with a small
/// stack-buffer prep, but we keep it simple — the perf impact on bench
/// is in the noise (no string-heavy case yet).
fn define_str_print<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    write: FunctionValue<'ctx>,
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
    builder.position_at_end(entry);

    let s = f.get_nth_param(0).unwrap().into_pointer_value();

    // load len from offset 0
    let len = builder
        .build_load(i64_t, s, "len")
        .unwrap()
        .into_int_value();

    // data = s + 8
    let data = unsafe {
        builder
            .build_in_bounds_gep(i8_t, s, &[i64_t.const_int(8, false)], "data")
            .unwrap()
    };

    let stdout_fd = i32_t.const_int(1, false);
    builder
        .build_call(write, &[stdout_fd.into(), data.into(), len.into()], "_w")
        .unwrap();

    // Trailing newline. A 1-byte stack alloca holding '\n'.
    let nl_slot = builder.build_alloca(i8_t, "nl").unwrap();
    builder
        .build_store(nl_slot, i8_t.const_int(b'\n' as u64, false))
        .unwrap();
    builder
        .build_call(
            write,
            &[stdout_fd.into(), nl_slot.into(), i64_t.const_int(1, false).into()],
            "_wn",
        )
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
    let loop1 = ctx.append_basic_block(f, "loop1");
    let dump = ctx.append_basic_block(f, "dump");
    let loop2 = ctx.append_basic_block(f, "loop2");
    let pop = ctx.append_basic_block(f, "pop");
    let done = ctx.append_basic_block(f, "done");

    builder.position_at_end(entry);
    let buf = builder.build_alloca(i64_t.array_type(20), "buf").unwrap();
    let cnt_a = builder.build_alloca(i64_t, "count").unwrap();
    builder
        .build_store(cnt_a, i64_t.const_int(0, false))
        .unwrap();
    let n_a = builder.build_alloca(i64_t, "n").unwrap();
    let arg = f.get_nth_param(0).unwrap().into_int_value();
    builder.build_store(n_a, arg).unwrap();
    builder.build_unconditional_branch(loop1).unwrap();

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
        Type::Ptr | Type::Str => ctx.ptr_type(AddressSpace::default()).fn_type(&param_metas, false),
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
        Type::Ptr | Type::Str => ctx.ptr_type(AddressSpace::default()).into(),
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
        Type::Ptr | Type::Str => ctx.ptr_type(AddressSpace::default()).into(),
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
                let callee = self.fn_map[fid.0 as usize];
                let argv: Vec<BasicMetadataValueEnum> =
                    args.iter().map(|a| self.operand(a).into()).collect();
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
            InstKind::Load(t, ptr) => {
                let bt = basic_type(self.ctx, *t);
                let p = self.operand(ptr).into_pointer_value();
                let v = self.builder.build_load(bt, p, "").unwrap();
                Some(v)
            }
            InstKind::Store(val, ptr) => {
                let v = self.operand(val);
                let p = self.operand(ptr).into_pointer_value();
                self.builder.build_store(p, v).unwrap();
                None
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
                    self.builder.build_return(Some(&v)).unwrap();
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
