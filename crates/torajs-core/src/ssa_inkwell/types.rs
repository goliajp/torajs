//! SSA Type → Inkwell type helpers + `declare_ssa_fn`.
//!
//! Four small free fns that translate between the SSA layer's
//! `Type` enum and Inkwell's type system. Used by `declare_ssa_fn`
//! (synthesizing LLVM signatures from SSA function shapes) and by
//! the FnLower path's alloca / load / store sites.
//!
//! Extracted from `ssa_inkwell.rs` god-file decomposition (2026-05-25,
//! batch 7).
//!
//! - `declare_ssa_fn` — `s::Function` → `FunctionValue`. Special-
//!   cases `main` to widen the LLVM signature with argc/argv (and
//!   names it `__main_argc_argv` on wasm32-wasi so `wasi-libc`'s
//!   `__main_void` resolves the user entry).
//! - `build_fn_type` — SSA `(params, ret)` → `FunctionType`.
//! - `basic_meta_type` — Type → `BasicMetadataTypeEnum` (for fn
//!   signature param slots). Void unrepresentable.
//! - `basic_type` — Type → `BasicTypeEnum` (for alloca / load /
//!   store width). Void unrepresentable.

use inkwell::AddressSpace;
use inkwell::context::Context;
use inkwell::module::Module as LlvmModule;
use inkwell::types::{BasicMetadataTypeEnum, BasicTypeEnum, FunctionType};
use inkwell::values::FunctionValue;

use super::CompileTarget;
use crate::ssa::{self as s, Type};

pub(super) fn declare_ssa_fn<'ctx>(
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

pub(super) fn build_fn_type<'ctx>(
    ctx: &'ctx Context,
    params: &[Type],
    ret: Type,
) -> FunctionType<'ctx> {
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

pub(super) fn basic_meta_type<'ctx>(ctx: &'ctx Context, t: Type) -> BasicMetadataTypeEnum<'ctx> {
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
pub(super) fn basic_type<'ctx>(ctx: &'ctx Context, t: Type) -> BasicTypeEnum<'ctx> {
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
