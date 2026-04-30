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
extern "C" fn print_bool_runtime(b: i8) {
    println!("{}", b != 0);
}

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
/// fresh heap StrRepr holding `a.bytes ++ b.bytes`. Inputs are read-only —
/// `a` and `b` keep their heaps and remain droppable. Matches TS semantics
/// (`a + b` doesn't modify the operands).
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
        p
    }
}

// M6.1 — String methods. All operate on the StrRepr layout
// `[u64 len, u8 data[len]]` and mirror what JS / TS exposes on
// `String.prototype`. AOT side defines the same symbols in LLVM IR
// (`define_str_*` in ssa_inkwell.rs) so JIT / AOT stay in lockstep.

/// Helper: read a StrRepr's `(len, data_ptr)`.
unsafe fn str_view<'a>(s: *const u8) -> &'a [u8] {
    unsafe {
        let len = std::ptr::read(s as *const u64) as usize;
        std::slice::from_raw_parts(s.add(8), len)
    }
}

/// Helper: allocate a fresh StrRepr holding `bytes`.
fn str_from_bytes(bytes: &[u8]) -> *mut u8 {
    let total = 8 + bytes.len();
    let layout = std::alloc::Layout::from_size_align(total, 8).expect("layout");
    unsafe {
        let p = std::alloc::alloc(layout);
        if p.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        std::ptr::write(p as *mut u64, bytes.len() as u64);
        if !bytes.is_empty() {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), p.add(8), bytes.len());
        }
        p
    }
}

extern "C" fn str_slice_runtime(s: *const u8, start: i64, end: i64) -> *mut u8 {
    unsafe {
        let view = str_view(s);
        let len = view.len() as i64;
        let s_clamp = start.clamp(0, len) as usize;
        let e_clamp = end.clamp(start.max(0), len) as usize;
        str_from_bytes(&view[s_clamp..e_clamp])
    }
}

extern "C" fn str_char_code_at_runtime(s: *const u8, i: i64) -> i64 {
    unsafe {
        let view = str_view(s);
        if i < 0 || (i as usize) >= view.len() {
            return 0; // TS returns NaN for OOB; M6.1 stub returns 0 (i64-shape).
        }
        view[i as usize] as i64
    }
}

extern "C" fn str_starts_with_runtime(s: *const u8, prefix: *const u8) -> i8 {
    unsafe {
        let view = str_view(s);
        let pre = str_view(prefix);
        i8::from(view.starts_with(pre))
    }
}

extern "C" fn str_ends_with_runtime(s: *const u8, suffix: *const u8) -> i8 {
    unsafe {
        let view = str_view(s);
        let suf = str_view(suffix);
        i8::from(view.ends_with(suf))
    }
}

extern "C" fn str_index_of_runtime(s: *const u8, sub: *const u8) -> i64 {
    unsafe {
        let view = str_view(s);
        let sub = str_view(sub);
        match view.windows(sub.len().max(1)).position(|w| {
            // Empty `sub` matches at position 0 (matches TS).
            sub.is_empty() || w == sub
        }) {
            Some(i) if !sub.is_empty() || i == 0 => i as i64,
            _ => -1,
        }
    }
}

extern "C" fn str_includes_runtime(s: *const u8, sub: *const u8) -> i8 {
    i8::from(str_index_of_runtime(s, sub) >= 0)
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

// M1.2 — `Array<T>` runtime. Layout: `{u64 len, u64 cap, T data[cap]}`
// with uniform 8-byte slots. Identical layout in both backends.
//
// MVP: only i64 element type. Extension to f64/ptr/etc. comes in
// follow-up subsection by adding push variants per element type.

extern "C" fn arr_alloc_runtime(initial_cap: u64) -> *mut u8 {
    let total = 16usize + (initial_cap as usize) * 8;
    let p = unsafe { malloc(total) };
    if p.is_null() {
        eprintln!("arr_alloc: out of memory");
        std::process::abort();
    }
    unsafe {
        let header = p as *mut u64;
        *header = 0; // len = 0
        *header.add(1) = initial_cap; // cap = initial_cap
    }
    p
}

extern "C" fn arr_push_runtime(arr: *mut u8, val: i64) -> *mut u8 {
    let mut p = arr;
    unsafe {
        let header = p as *mut u64;
        let len = *header;
        let cap = *header.add(1);
        if len == cap {
            let new_cap = if cap == 0 { 4 } else { cap * 2 };
            let new_total = 16usize + (new_cap as usize) * 8;
            // Use libc realloc to preserve existing data.
            let np = libc_realloc(p, new_total);
            if np.is_null() {
                eprintln!("arr_push: realloc out of memory");
                std::process::abort();
            }
            p = np;
            let header = p as *mut u64;
            *header.add(1) = new_cap;
        }
        // Store val at offset 16 + len*8.
        let header = p as *mut u64;
        let slot_off = 16 + (len as usize) * 8;
        let slot = (p as *mut u8).add(slot_off) as *mut i64;
        *slot = val;
        *header = len + 1;
    }
    p
}

extern "C" fn arr_drop_runtime(p: *mut u8) {
    if p.is_null() {
        return;
    }
    unsafe { free(p) }
}

// M6.2 fast-path. `arr_reserve(arr, new_cap)` ensures the array has at
// least `new_cap` slots; reallocs once if cap < new_cap, otherwise
// no-op. Returns the (possibly new) ptr — caller stores it back into
// its slot.
extern "C" fn arr_reserve_runtime(arr: *mut u8, new_cap: u64) -> *mut u8 {
    let mut p = arr;
    unsafe {
        let header = p as *mut u64;
        let cap = *header.add(1);
        if cap < new_cap {
            let new_total = 16usize + (new_cap as usize) * 8;
            let np = libc_realloc(p, new_total);
            if np.is_null() {
                eprintln!("arr_reserve: realloc out of memory");
                std::process::abort();
            }
            p = np;
            let header = p as *mut u64;
            *header.add(1) = new_cap;
        }
    }
    p
}

// `arr_push_unchecked(arr, val)` writes val at the next slot without
// any capacity check. UB if cap < len + 1; safe only when paired with
// a preceding `arr_reserve` for a known upper bound. Used by M6.2's
// map/filter loop after a one-shot reserve.
extern "C" fn arr_push_unchecked_runtime(arr: *mut u8, val: i64) {
    unsafe {
        let header = arr as *mut u64;
        let len = *header;
        let slot_off = 16 + (len as usize) * 8;
        let slot = (arr as *mut u8).add(slot_off) as *mut i64;
        *slot = val;
        *header = len + 1;
    }
}

// M4 — exception state. Two thread-local i64 globals plus three
// runtime fns that ssa_lower emits as Call sites. Non-thread-safe
// (the JIT compiles + runs in one thread); the AOT path defines the
// same symbols in LLVM IR with module-level globals.
static mut TORAJS_THROW_ACTIVE: i64 = 0;
static mut TORAJS_THROW_VALUE: i64 = 0;

extern "C" fn throw_set_runtime(v: i64) {
    unsafe {
        TORAJS_THROW_ACTIVE = 1;
        TORAJS_THROW_VALUE = v;
    }
}

extern "C" fn throw_check_runtime() -> i64 {
    unsafe { TORAJS_THROW_ACTIVE }
}

extern "C" fn throw_take_runtime() -> i64 {
    unsafe {
        let v = TORAJS_THROW_VALUE;
        TORAJS_THROW_ACTIVE = 0;
        v
    }
}

unsafe extern "C" {
    fn realloc(p: *mut u8, size: usize) -> *mut u8;
}

#[inline]
fn libc_realloc(p: *mut u8, size: usize) -> *mut u8 {
    unsafe { realloc(p, size) }
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
extern "C" fn math_log_runtime(x: f64) -> f64 {
    x.ln()
}
extern "C" fn math_exp_runtime(x: f64) -> f64 {
    x.exp()
}
extern "C" fn math_pow_runtime(x: f64, y: f64) -> f64 {
    x.powf(y)
}
extern "C" fn math_min_runtime(x: f64, y: f64) -> f64 {
    x.min(y)
}
extern "C" fn math_max_runtime(x: f64, y: f64) -> f64 {
    x.max(y)
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
    jit_builder.symbol("print_bool", print_bool_runtime as *const u8);
    jit_builder.symbol("__torajs_str_alloc", str_alloc_runtime as *const u8);
    jit_builder.symbol("__torajs_str_print", str_print_runtime as *const u8);
    jit_builder.symbol("__torajs_str_drop", str_drop_runtime as *const u8);
    jit_builder.symbol("__torajs_str_concat", str_concat_runtime as *const u8);
    jit_builder.symbol("__torajs_obj_alloc", obj_alloc_runtime as *const u8);
    jit_builder.symbol("__torajs_obj_drop", obj_drop_runtime as *const u8);
    jit_builder.symbol("__torajs_arr_alloc", arr_alloc_runtime as *const u8);
    jit_builder.symbol("__torajs_arr_push", arr_push_runtime as *const u8);
    jit_builder.symbol("__torajs_arr_reserve", arr_reserve_runtime as *const u8);
    jit_builder.symbol(
        "__torajs_arr_push_unchecked",
        arr_push_unchecked_runtime as *const u8,
    );
    jit_builder.symbol("__torajs_str_slice", str_slice_runtime as *const u8);
    jit_builder.symbol(
        "__torajs_str_char_code_at",
        str_char_code_at_runtime as *const u8,
    );
    jit_builder.symbol(
        "__torajs_str_starts_with",
        str_starts_with_runtime as *const u8,
    );
    jit_builder.symbol(
        "__torajs_str_ends_with",
        str_ends_with_runtime as *const u8,
    );
    jit_builder.symbol(
        "__torajs_str_index_of",
        str_index_of_runtime as *const u8,
    );
    jit_builder.symbol(
        "__torajs_str_includes",
        str_includes_runtime as *const u8,
    );
    jit_builder.symbol("__torajs_arr_drop", arr_drop_runtime as *const u8);
    jit_builder.symbol("__torajs_math_sqrt", math_sqrt_runtime as *const u8);
    jit_builder.symbol("__torajs_math_abs", math_abs_runtime as *const u8);
    jit_builder.symbol("__torajs_math_floor", math_floor_runtime as *const u8);
    jit_builder.symbol("__torajs_math_ceil", math_ceil_runtime as *const u8);
    jit_builder.symbol("__torajs_math_log", math_log_runtime as *const u8);
    jit_builder.symbol("__torajs_math_exp", math_exp_runtime as *const u8);
    jit_builder.symbol("__torajs_math_pow", math_pow_runtime as *const u8);
    jit_builder.symbol("__torajs_math_min", math_min_runtime as *const u8);
    jit_builder.symbol("__torajs_math_max", math_max_runtime as *const u8);
    jit_builder.symbol("__torajs_throw_set", throw_set_runtime as *const u8);
    jit_builder.symbol("__torajs_throw_check", throw_check_runtime as *const u8);
    jit_builder.symbol("__torajs_throw_take", throw_take_runtime as *const u8);

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
        // Pointer-shaped types all lower to host pointer width.
        Type::Ptr | Type::Str | Type::Obj(_) | Type::Arr(_) | Type::FnSig(_) | Type::Closure(_) => {
            module.target_config().pointer_type()
        }
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
    ssa_module: &Module,
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
                Type::I64
                | Type::F64
                | Type::Ptr
                | Type::Str
                | Type::Obj(_)
                | Type::Arr(_)
                | Type::FnSig(_)
                | Type::Closure(_) => 8,
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
        InstKind::LoadDyn(t, base, off) => {
            // Dynamic offset — compute `addr = base + off` then load.
            // Both base and off are i64 / pointer-sized at the SSA layer.
            let p = operand(bcx, ptr_ty, value_map, data_refs, module, data_map, f, base);
            let o = operand(bcx, ptr_ty, value_map, data_refs, module, data_map, f, off);
            let addr = bcx.ins().iadd(p, o);
            let ty = clif_type(module, *t);
            Some(bcx.ins().load(ty, MemFlags::new(), addr, 0))
        }
        InstKind::StoreDyn(val, base, off) => {
            let v = operand(bcx, ptr_ty, value_map, data_refs, module, data_map, f, val);
            let p = operand(bcx, ptr_ty, value_map, data_refs, module, data_map, f, base);
            let o = operand(bcx, ptr_ty, value_map, data_refs, module, data_map, f, off);
            let addr = bcx.ins().iadd(p, o);
            bcx.ins().store(MemFlags::new(), v, addr, 0);
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
        InstKind::FnAddr(fid) => {
            // Take the address of an imported fn. M2 Phase B Stage 3.
            let target = fn_map[fid.0 as usize];
            let func_ref = *func_refs
                .entry(fid.0)
                .or_insert_with(|| module.declare_func_in_func(target, bcx.func));
            Some(bcx.ins().func_addr(ptr_ty, func_ref))
        }
        InstKind::CallIndirect(sig_id, ptr, args) => {
            // Build a CLIF Signature from the SSA signature interner.
            let (params, ret) = ssa_module.signature(*sig_id).clone();
            let mut sig = Signature::new(module.target_config().default_call_conv);
            for p in &params {
                sig.params.push(AbiParam::new(clif_type(module, *p)));
            }
            if ret != Type::Void {
                sig.returns.push(AbiParam::new(clif_type(module, ret)));
            }
            let sig_ref = bcx.import_signature(sig);
            let p = operand(bcx, ptr_ty, value_map, data_refs, module, data_map, f, ptr);
            let argv: Vec<ir::Value> = args
                .iter()
                .map(|a| {
                    operand(bcx, ptr_ty, value_map, data_refs, module, data_map, f, a)
                })
                .collect();
            let call = bcx.ins().call_indirect(sig_ref, p, &argv);
            let results = bcx.inst_results(call);
            results.first().copied()
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
