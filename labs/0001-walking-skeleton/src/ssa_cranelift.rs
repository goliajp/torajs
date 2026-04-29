// SSA → Cranelift IR (CLIF) → in-process JIT execution. Powers the new
// `tr jit foo.ts` subcommand (will become `tr run` after the tree-walk
// interpreter retires in a follow-up cleanup).
//
// Trade-off vs the LLVM AOT path (ssa_inkwell):
//   - codegen quality: weaker than LLVM -O2/-O3 (no auto-vectorize, no loop
//     idiom recognition for popcount → cnt.16b). For our cases this means
//     2-5× slower run_ms than the AOT row.
//   - compile time: Cranelift compiles in single-digit ms — ~10× faster
//     than LLVM's ~40 ms. So the total `compile + run` budget is what
//     dominates `tr jit` mode, not just run_ms.
//   - in-process JIT: no link step, no temp file. Function pointer is
//     just `mem::transmute<*const u8, fn()->i32>` once finalize_definitions
//     publishes the code page.
//
// Runtime trampolines: print_i64 / print_str are implemented in Rust and
// registered as JIT symbols; calls from JIT'd code resolve to those Rust
// fn pointers via Cranelift's libcall mechanism.

use std::collections::HashMap;
use std::mem;

use cranelift_codegen::ir::{
    AbiParam, Function as ClifFunction, InstBuilder, MemFlags, Signature, UserFuncName,
    condcodes, types as ctypes,
};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_codegen::{Context, ir};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{DataDescription, FuncId as CfFuncId, Linkage, Module as CfModule};

use crate::ssa::{
    self as s, BinOp, FPred, IPred, InstKind, Module, Operand, Terminator, Type,
};

#[derive(Debug)]
pub enum JitError {
    Module(String),
    Build(String),
    Verify(String),
}

impl std::fmt::Display for JitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JitError::Module(s) => write!(f, "JIT module: {s}"),
            JitError::Build(s) => write!(f, "JIT build: {s}"),
            JitError::Verify(s) => write!(f, "CLIF verify: {s}"),
        }
    }
}

impl From<cranelift_module::ModuleError> for JitError {
    fn from(e: cranelift_module::ModuleError) -> Self {
        JitError::Module(e.to_string())
    }
}

/// Runtime print of a 64-bit integer + newline. Registered with the JIT so
/// emitted `call print_i64(...)` resolves here.
extern "C" fn print_i64_runtime(n: i64) {
    println!("{n}");
}

/// Float counterpart — `console.log(<f64>)` routes here. Uses Rust's
/// default `{}` formatter; we rely on libc `%g` matching for the AOT
/// path. Minor edge-case divergence (NaN sign, -0) is acceptable for
/// bench output.
extern "C" fn print_f64_runtime(x: f64) {
    println!("{x}");
}

/// `__torajs_str_alloc(*const u8 src, u64 len) -> *StrRepr` — copy `len`
/// bytes from `src` into a fresh heap StrRepr `{u64 len; u8 data[]}`.
/// Returns the pointer to the StrRepr (which is also the pointer to the
/// length prefix). Layout matches what the Inkwell backend emits, so a
/// program JIT'd via Cranelift produces the same heap representation as
/// when AOT-compiled — interoperable should we ever pass strings across.
extern "C" fn str_alloc_runtime(src: *const u8, len: u64) -> *mut u8 {
    let total = 8 + len as usize;
    let layout = std::alloc::Layout::from_size_align(total, 8).expect("layout");
    unsafe {
        let p = std::alloc::alloc(layout);
        if p.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        // Write len at offset 0
        std::ptr::write(p as *mut u64, len);
        // Copy bytes to offset 8
        if len > 0 {
            std::ptr::copy_nonoverlapping(src, p.add(8), len as usize);
        }
        p
    }
}

/// `__torajs_str_concat(*StrRepr a, *StrRepr b) -> *StrRepr` — allocates a
/// fresh heap StrRepr holding `a.bytes ++ b.bytes`, then frees both inputs.
/// Caller transfers ownership of a and b to this fn (lowerer marks the
/// source bindings moved so end-of-fn drop skips them).
extern "C" fn str_concat_runtime(a: *mut u8, b: *mut u8) -> *mut u8 {
    unsafe {
        let a_len = std::ptr::read(a as *const u64) as usize;
        let b_len = std::ptr::read(b as *const u64) as usize;
        let total = a_len + b_len;
        let alloc_size = 8 + total;
        let layout =
            std::alloc::Layout::from_size_align(alloc_size, 8).expect("layout");
        let p = std::alloc::alloc(layout);
        if p.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        std::ptr::write(p as *mut u64, total as u64);
        if a_len > 0 {
            std::ptr::copy_nonoverlapping(a.add(8), p.add(8), a_len);
        }
        if b_len > 0 {
            std::ptr::copy_nonoverlapping(b.add(8), p.add(8 + a_len), b_len);
        }
        // Free a and b — using their original layouts.
        let a_layout =
            std::alloc::Layout::from_size_align(8 + a_len, 8).expect("a layout");
        std::alloc::dealloc(a, a_layout);
        let b_layout =
            std::alloc::Layout::from_size_align(8 + b_len, 8).expect("b layout");
        std::alloc::dealloc(b, b_layout);
        p
    }
}

// Object alloc/drop go through libc directly so we don't have to track
// per-pointer Layouts (unlike strings, where the length is in the
// StrRepr header). Inkwell backend uses the same libc pair, so AOT
// and JIT produce equivalent heap layouts.
unsafe extern "C" {
    fn malloc(size: usize) -> *mut u8;
    fn free(p: *mut u8);
}

extern "C" fn obj_alloc_runtime(size: u64) -> *mut u8 {
    if size == 0 {
        return std::ptr::null_mut();
    }
    let p = unsafe { malloc(size as usize) };
    if p.is_null() {
        eprintln!("obj_alloc: out of memory");
        std::process::abort();
    }
    p
}

extern "C" fn obj_drop_runtime(p: *mut u8) {
    if p.is_null() {
        return;
    }
    unsafe { free(p) }
}

// Math.* wrappers — each routes to the corresponding f64 method on Rust's
// std float ops. The AOT path uses libc via Inkwell-emitted IR; both paths
// produce identical results since libm and Rust's stdlib agree on these.
extern "C" fn math_sqrt_runtime(x: f64) -> f64 {
    x.sqrt()
}
extern "C" fn math_abs_runtime(x: f64) -> f64 {
    x.abs()
}
extern "C" fn math_floor_runtime(x: f64) -> f64 {
    x.floor()
}
extern "C" fn math_ceil_runtime(x: f64) -> f64 {
    x.ceil()
}

/// `__torajs_str_drop(*StrRepr s) -> void` — release the heap StrRepr.
/// Layout must match what `str_alloc_runtime` produced: total size = 8+len.
extern "C" fn str_drop_runtime(s: *mut u8) {
    if s.is_null() {
        return;
    }
    unsafe {
        let len = std::ptr::read(s as *const u64) as usize;
        let total = 8 + len;
        let layout = std::alloc::Layout::from_size_align(total, 8).expect("layout");
        std::alloc::dealloc(s, layout);
    }
}

/// `__torajs_str_print(*StrRepr s) -> void` — load len from offset 0,
/// write the bytes plus a trailing newline. Uses Rust's println! for
/// portability; the bench JIT code path is rarely the bottleneck.
extern "C" fn str_print_runtime(s: *const u8) {
    if s.is_null() {
        println!();
        return;
    }
    unsafe {
        let len = std::ptr::read(s as *const u64) as usize;
        let bytes = std::slice::from_raw_parts(s.add(8), len);
        println!("{}", String::from_utf8_lossy(bytes));
    }
}

/// Compile + execute the SSA module's `main` function in-process. Returns
/// main's i32 return value (0 on success per C ABI).
pub fn execute(ssa_module: &Module) -> Result<i32, JitError> {
    // Build a JIT module pre-loaded with our intrinsic symbols. JITBuilder
    // expects the host triple's libcall set; this ABIs through to the
    // standard C runtime for division / floating-point library calls.
    let mut flag_builder = settings::builder();
    // PIC isn't required for in-process JIT; the default is fine. We do
    // set opt_level so codegen quality matches what the user expects.
    flag_builder
        .set("opt_level", "speed")
        .map_err(|e| JitError::Build(format!("setting opt_level: {e}")))?;
    let isa_builder = cranelift_native::builder()
        .map_err(|e| JitError::Build(format!("native isa builder: {e}")))?;
    let isa = isa_builder
        .finish(settings::Flags::new(flag_builder))
        .map_err(|e| JitError::Build(format!("isa finish: {e}")))?;

    let mut jit_builder =
        JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());
    jit_builder.symbol("print_i64", print_i64_runtime as *const u8);
    jit_builder.symbol("print_f64", print_f64_runtime as *const u8);
    jit_builder.symbol("__torajs_str_alloc", str_alloc_runtime as *const u8);
    jit_builder.symbol("__torajs_str_print", str_print_runtime as *const u8);
    jit_builder.symbol("__torajs_str_drop", str_drop_runtime as *const u8);
    jit_builder.symbol("__torajs_str_concat", str_concat_runtime as *const u8);
    jit_builder.symbol("__torajs_obj_alloc", obj_alloc_runtime as *const u8);
    jit_builder.symbol("__torajs_obj_drop", obj_drop_runtime as *const u8);
    jit_builder.symbol("__torajs_math_sqrt", math_sqrt_runtime as *const u8);
    jit_builder.symbol("__torajs_math_abs", math_abs_runtime as *const u8);
    jit_builder.symbol("__torajs_math_floor", math_floor_runtime as *const u8);
    jit_builder.symbol("__torajs_math_ceil", math_ceil_runtime as *const u8);

    let mut module = JITModule::new(jit_builder);
    let ptr_ty = module.target_config().pointer_type();

    // Pass A: declare every SSA function in the JIT module so callsites
    // can resolve. Intrinsics use `Linkage::Import` — their bodies live
    // in this Rust binary, registered above as raw symbols.
    let mut fn_map: Vec<CfFuncId> = Vec::with_capacity(ssa_module.funcs.len());
    for f in &ssa_module.funcs {
        let sig = build_signature(&module, f);
        let linkage = if f.is_declaration() {
            Linkage::Import
        } else {
            Linkage::Local
        };
        let fid = module.declare_function(&f.name, linkage, &sig)?;
        fn_map.push(fid);
    }

    // Pass B: declare + define the string globals. JIT module supports
    // arbitrary data segments addressable via symbol_value at use sites.
    let mut data_map: Vec<cranelift_module::DataId> =
        Vec::with_capacity(ssa_module.strings.len());
    for (i, bytes) in ssa_module.strings.iter().enumerate() {
        let did = module.declare_data(
            &format!(".str{i}"),
            Linkage::Local,
            /*writable=*/ false,
            /*tls=*/ false,
        )?;
        let mut desc = DataDescription::new();
        desc.define(bytes.clone().into_boxed_slice());
        module.define_data(did, &desc)?;
        data_map.push(did);
    }

    // Pass C: lower bodies. Skip declarations (the runtime trampolines
    // above provide them).
    for (i, f) in ssa_module.funcs.iter().enumerate() {
        if f.is_declaration() {
            continue;
        }
        lower_fn(&mut module, ptr_ty, ssa_module, f, fn_map[i], &fn_map, &data_map)?;
    }

    module
        .finalize_definitions()
        .map_err(|e| JitError::Module(e.to_string()))?;

    // Locate `main` and call it.
    let main_id = ssa_module
        .funcs
        .iter()
        .position(|f| f.name == "main")
        .ok_or_else(|| JitError::Build("module has no `main` function".into()))?;
    let main_ptr = module.get_finalized_function(fn_map[main_id]);
    let main_fn: extern "C" fn() -> i32 = unsafe { mem::transmute(main_ptr) };
    Ok(main_fn())
}

fn build_signature<M: CfModule>(module: &M, f: &s::Function) -> Signature {
    let mut sig = module.make_signature();
    for &p in &f.params {
        let ty = clif_type(module, f.value_type(p));
        sig.params.push(AbiParam::new(ty));
    }
    if f.ret != Type::Void {
        sig.returns.push(AbiParam::new(clif_type(module, f.ret)));
    }
    sig
}

fn clif_type<M: CfModule>(module: &M, t: Type) -> ir::Type {
    match t {
        Type::I64 => ctypes::I64,
        Type::I32 => ctypes::I32,
        Type::F64 => ctypes::F64,
        // Cranelift represents booleans as `i8`; this matches what icmp /
        // fcmp produce.
        Type::Bool => ctypes::I8,
        // Both opaque pointer + Str lower to host pointer width.
        Type::Ptr | Type::Str | Type::Obj(_) => module.target_config().pointer_type(),
        Type::Void => panic!("Type::Void has no CLIF representation"),
    }
}

fn lower_fn(
    module: &mut JITModule,
    ptr_ty: ir::Type,
    ssa_module: &Module,
    f: &s::Function,
    fid: CfFuncId,
    fn_map: &[CfFuncId],
    data_map: &[cranelift_module::DataId],
) -> Result<(), JitError> {
    let sig = build_signature(module, f);
    let mut ctx = Context::new();
    ctx.func = ClifFunction::with_name_signature(UserFuncName::user(0, fid.as_u32()), sig);

    let mut fbctx = FunctionBuilderContext::new();
    let mut bcx = FunctionBuilder::new(&mut ctx.func, &mut fbctx);

    // Pre-create CLIF blocks for every SSA block.
    let mut block_map: HashMap<u32, ir::Block> = HashMap::new();
    for b in &f.blocks {
        let cb = bcx.create_block();
        block_map.insert(b.id.0, cb);
    }
    let entry_clif = block_map[&f.blocks[0].id.0];
    bcx.append_block_params_for_function_params(entry_clif);

    // Map ssa::ValueId → ir::Value. Params come from the entry block's
    // function-param slots; everything else is filled in by lower_inst.
    let mut value_map: HashMap<u32, ir::Value> = HashMap::new();
    let entry_params = bcx.block_params(entry_clif).to_vec();
    for (i, &p) in f.params.iter().enumerate() {
        value_map.insert(p.0, entry_params[i]);
    }

    // Per-function refs to other functions (for call) and data (for
    // string_ref). Cranelift declares these per-function via
    // declare_func_in_func / declare_data_in_func.
    let mut func_refs: HashMap<u32, ir::FuncRef> = HashMap::new();
    let mut data_refs: HashMap<u32, ir::GlobalValue> = HashMap::new();

    for b in &f.blocks {
        let cb = block_map[&b.id.0];
        bcx.switch_to_block(cb);
        for inst in &b.insts {
            lower_inst(
                &mut bcx,
                module,
                ptr_ty,
                ssa_module,
                f,
                inst,
                &mut value_map,
                fn_map,
                data_map,
                &mut func_refs,
                &mut data_refs,
            );
        }
        lower_term(&mut bcx, &b.term, &block_map, &value_map);
    }

    // All block predecessors known by now.
    bcx.seal_all_blocks();
    bcx.finalize();

    // Verify before defining (cheap, surfaces our codegen bugs early).
    if let Err(e) = cranelift_codegen::verify_function(&ctx.func, module.isa()) {
        return Err(JitError::Verify(e.to_string()));
    }

    module.define_function(fid, &mut ctx).map_err(|e| {
        JitError::Module(format!("define_function {}: {e}", f.name))
    })?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn lower_inst(
    bcx: &mut FunctionBuilder<'_>,
    module: &mut JITModule,
    ptr_ty: ir::Type,
    _ssa_module: &Module,
    f: &s::Function,
    inst: &s::Inst,
    value_map: &mut HashMap<u32, ir::Value>,
    fn_map: &[CfFuncId],
    data_map: &[cranelift_module::DataId],
    func_refs: &mut HashMap<u32, ir::FuncRef>,
    data_refs: &mut HashMap<u32, ir::GlobalValue>,
) {
    let result_val: Option<ir::Value> = match &inst.kind {
        InstKind::BinOp(op, a, b) => {
            let av = operand(bcx, ptr_ty, value_map, data_refs, module, data_map, f, a);
            let bv = operand(bcx, ptr_ty, value_map, data_refs, module, data_map, f, b);
            let r = match op {
                BinOp::Add => bcx.ins().iadd(av, bv),
                BinOp::Sub => bcx.ins().isub(av, bv),
                BinOp::Mul => bcx.ins().imul(av, bv),
                BinOp::SDiv => bcx.ins().sdiv(av, bv),
                BinOp::SRem => bcx.ins().srem(av, bv),
                BinOp::And => bcx.ins().band(av, bv),
                BinOp::Or => bcx.ins().bor(av, bv),
                BinOp::Xor => bcx.ins().bxor(av, bv),
                BinOp::Shl => bcx.ins().ishl(av, bv),
                BinOp::AShr => bcx.ins().sshr(av, bv),
                BinOp::LShr => bcx.ins().ushr(av, bv),
                BinOp::FAdd => bcx.ins().fadd(av, bv),
                BinOp::FSub => bcx.ins().fsub(av, bv),
                BinOp::FMul => bcx.ins().fmul(av, bv),
                BinOp::FDiv => bcx.ins().fdiv(av, bv),
            };
            Some(r)
        }
        InstKind::ICmp(p, a, b) => {
            let av = operand(bcx, ptr_ty, value_map, data_refs, module, data_map, f, a);
            let bv = operand(bcx, ptr_ty, value_map, data_refs, module, data_map, f, b);
            let cc = match p {
                IPred::Eq => condcodes::IntCC::Equal,
                IPred::Ne => condcodes::IntCC::NotEqual,
                IPred::Slt => condcodes::IntCC::SignedLessThan,
                IPred::Sgt => condcodes::IntCC::SignedGreaterThan,
                IPred::Sle => condcodes::IntCC::SignedLessThanOrEqual,
                IPred::Sge => condcodes::IntCC::SignedGreaterThanOrEqual,
            };
            Some(bcx.ins().icmp(cc, av, bv))
        }
        InstKind::FCmp(p, a, b) => {
            let av = operand(bcx, ptr_ty, value_map, data_refs, module, data_map, f, a);
            let bv = operand(bcx, ptr_ty, value_map, data_refs, module, data_map, f, b);
            let cc = match p {
                FPred::Oeq => condcodes::FloatCC::Equal,
                FPred::One => condcodes::FloatCC::NotEqual,
                FPred::Olt => condcodes::FloatCC::LessThan,
                FPred::Ogt => condcodes::FloatCC::GreaterThan,
                FPred::Ole => condcodes::FloatCC::LessThanOrEqual,
                FPred::Oge => condcodes::FloatCC::GreaterThanOrEqual,
            };
            Some(bcx.ins().fcmp(cc, av, bv))
        }
        InstKind::Call(fid, args) => {
            let target = fn_map[fid.0 as usize];
            let func_ref = *func_refs
                .entry(fid.0)
                .or_insert_with(|| module.declare_func_in_func(target, bcx.func));
            let argv: Vec<ir::Value> = args
                .iter()
                .map(|a| {
                    operand(bcx, ptr_ty, value_map, data_refs, module, data_map, f, a)
                })
                .collect();
            let call = bcx.ins().call(func_ref, &argv);
            let results = bcx.inst_results(call);
            results.first().copied()
        }
        InstKind::Alloca(t) => {
            let bytes = match t {
                Type::I64 | Type::F64 | Type::Ptr | Type::Str | Type::Obj(_) => 8,
                Type::I32 => 4,
                Type::Bool => 1,
                Type::Void => panic!("alloca of void"),
            };
            let slot = bcx.create_sized_stack_slot(ir::StackSlotData::new(
                ir::StackSlotKind::ExplicitSlot,
                bytes,
                0,
            ));
            Some(bcx.ins().stack_addr(ptr_ty, slot, 0))
        }
        InstKind::Load(t, ptr, offset) => {
            let p = operand(bcx, ptr_ty, value_map, data_refs, module, data_map, f, ptr);
            let ty = clif_type(module, *t);
            // Cranelift's load takes an i32 offset directly. Cast u64 → i32
            // (offsets are always small for our object layouts; clamp at
            // i32::MAX would be silly for >2 GB structs).
            Some(bcx.ins().load(ty, MemFlags::new(), p, *offset as i32))
        }
        InstKind::Store(val, ptr, offset) => {
            let v = operand(bcx, ptr_ty, value_map, data_refs, module, data_map, f, val);
            let p = operand(bcx, ptr_ty, value_map, data_refs, module, data_map, f, ptr);
            bcx.ins().store(MemFlags::new(), v, p, *offset as i32);
            None
        }
        InstKind::SiToFp(op) => {
            let v = operand(bcx, ptr_ty, value_map, data_refs, module, data_map, f, op);
            Some(bcx.ins().fcvt_from_sint(ctypes::F64, v))
        }
        InstKind::StringRef(sid) => {
            let did = data_map[sid.0 as usize];
            let gv = *data_refs
                .entry(sid.0)
                .or_insert_with(|| module.declare_data_in_func(did, bcx.func));
            Some(bcx.ins().symbol_value(ptr_ty, gv))
        }
    };

    if let (Some(r), Some(v)) = (inst.result, result_val) {
        value_map.insert(r.0, v);
    }
}

#[allow(clippy::too_many_arguments)]
fn operand(
    bcx: &mut FunctionBuilder<'_>,
    ptr_ty: ir::Type,
    value_map: &HashMap<u32, ir::Value>,
    data_refs: &mut HashMap<u32, ir::GlobalValue>,
    module: &mut JITModule,
    data_map: &[cranelift_module::DataId],
    _f: &s::Function,
    op: &Operand,
) -> ir::Value {
    match op {
        Operand::Value(v) => *value_map
            .get(&v.0)
            .unwrap_or_else(|| panic!("unmapped SSA value {}", v.0)),
        Operand::ConstI64(n) => bcx.ins().iconst(ctypes::I64, *n),
        Operand::ConstI32(n) => bcx.ins().iconst(ctypes::I32, *n as i64),
        Operand::ConstBool(b) => bcx.ins().iconst(ctypes::I8, *b as i64),
        Operand::ConstF64(n) => bcx.ins().f64const(*n),
    }
    // (We don't reach data_map / data_refs / ptr_ty here, but they're
    // threaded through so future Operand variants — e.g. inline string
    // literals — can land in this function without a refactor.)
    .tap(|_| {
        let _ = (data_refs, module, data_map, ptr_ty);
    })
}

trait Tap: Sized {
    fn tap<F: FnOnce(&Self)>(self, f: F) -> Self {
        f(&self);
        self
    }
}
impl<T> Tap for T {}

fn lower_term(
    bcx: &mut FunctionBuilder<'_>,
    t: &Terminator,
    block_map: &HashMap<u32, ir::Block>,
    value_map: &HashMap<u32, ir::Value>,
) {
    match t {
        Terminator::Br(b) => {
            bcx.ins().jump(block_map[&b.0], &[]);
        }
        Terminator::CondBr {
            cond,
            then_blk,
            else_blk,
        } => {
            let cv = match cond {
                Operand::Value(v) => value_map[&v.0],
                Operand::ConstBool(true) => bcx.ins().iconst(ctypes::I8, 1),
                Operand::ConstBool(false) => bcx.ins().iconst(ctypes::I8, 0),
                other => panic!("cond_br with non-bool operand: {other:?}"),
            };
            bcx.ins().brif(
                cv,
                block_map[&then_blk.0],
                &[],
                block_map[&else_blk.0],
                &[],
            );
        }
        Terminator::Ret(maybe) => match maybe {
            Some(o) => {
                let v = match o {
                    Operand::Value(v) => value_map[&v.0],
                    Operand::ConstI64(n) => bcx.ins().iconst(ctypes::I64, *n),
                    Operand::ConstI32(n) => bcx.ins().iconst(ctypes::I32, *n as i64),
                    Operand::ConstF64(n) => bcx.ins().f64const(*n),
                    Operand::ConstBool(b) => bcx.ins().iconst(ctypes::I8, *b as i64),
                };
                bcx.ins().return_(&[v]);
            }
            None => {
                bcx.ins().return_(&[]);
            }
        },
        Terminator::Unreachable => {
            bcx.ins().trap(ir::TrapCode::user(1).expect("trap code 1"));
        }
    }
}
