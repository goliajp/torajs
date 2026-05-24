//! LLVM module-level global emission for string literals and
//! Phase-K.3 data globals.
//!
//! Three primitives:
//! 1. `emit_string_global` — `[N x i8]` private const per interned
//!    string literal (raw bytes, no NUL).
//! 2. `emit_static_str_global` — `.rodata` Str-shaped block so hot-
//!    loop literal use bypasses str_alloc + memcpy + str_drop.
//! 3. `emit_data_global` — Phase-K.3 module-level data globals
//!    (top-level `let X: T` for primitive Copy / Str / Arr / Obj).
//!
//! Extracted from `ssa_inkwell.rs` god-file decomposition
//! (2026-05-25, batch 3).

use inkwell::AddressSpace;
use inkwell::context::Context;
use inkwell::module::Module as LlvmModule;

use crate::ssa::{self as s, Type};

/// Mirror of `__TORAJS_FLAG_STATIC_LITERAL` in runtime_str.c. Encoded
/// here so the header u64 in `emit_static_str_global` can be built
/// without a runtime lookup.
pub(super) const STATIC_LITERAL_FLAG: u16 = 4;

/// Emit one `[N x i8]` private constant per interned string. Just the raw
/// bytes — no NUL terminator. The string runtime carries length explicitly
/// in the heap StrRepr's first 8 bytes.
pub(super) fn emit_string_global<'ctx>(
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
pub(super) fn emit_static_str_global<'ctx>(
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

/// Phase K.3 — emit one LLVM module-level data global per
/// `s::DataGlobal`. Zero-initialized; the SSA `main` fn lowers the
/// user's init expression and stores into the slot before any other
/// code runs. K.3 only registers primitive Copy types — string /
/// array / object globals are out of scope until a follow-up wires
/// up exit-time drop hooks.
pub(super) fn emit_data_global<'ctx>(
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
