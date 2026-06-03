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

/// Mirror of `crate::layout::STR_FLAG_IS_LATIN1` from torajs-str
/// (P11.1-S1). Static string literals are always Latin-1 at the S1
/// phase — every existing fixture's payload is ≤ 0xFF (ASCII subset)
/// so the bit-identical byte content satisfies Latin-1 semantics.
/// Build-time encoding decisions for non-ASCII literals land in S2.
pub(super) const STR_IS_LATIN1_FLAG: u16 = 2;

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

/// `[hdr:8 (rc=1, tag=STR, flags=STATIC_LITERAL|IS_LATIN1)] [length:4]
/// [_pad:4] [bytes:N]` — drop-in Str object that lives in `.rodata`.
/// rc_inc / rc_dec / str_free / arr_free all short-circuit via the
/// STATIC flag in the header so the global is never written to
/// (safe to mark constant).
///
/// Serves `intern_string_literal` callsites — every literal in a hot
/// loop now resolves to the same global ptr instead of paying a
/// per-iter str_alloc + memcpy + str_drop. Memory cost: one extra
/// 16-byte header per unique literal, paid once at link time.
///
/// ## P11.1-S1 layout (vs pre-S1 `[u64 header, u64 length, [N x i8]]`)
///
/// `length` moves from `u64 @8` to `u32 @8 + _pad u32 @12`, and the
/// `IS_LATIN1` flag bit is set in `flags` (S1 hard-codes Latin-1 —
/// all existing literals are ASCII subset of Latin-1, payload bytes
/// identical). LLVM struct shape:
/// `[u64 header, u32 length, u32 _pad, [N x i8] bytes]` — packed so
/// runtime `Load(I32, p, 8)` reads the length field exactly. The
/// `_pad u32` slot is zero-initialized; S2 / future P11.x phases
/// may repurpose it (hash slot, inline-cache hint).
pub(super) fn emit_static_str_global<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    idx: usize,
    bytes: &[u8],
) -> inkwell::values::GlobalValue<'ctx> {
    let i8_t = ctx.i8_type();
    let i32_t = ctx.i32_type();
    let i64_t = ctx.i64_type();
    let length = bytes.len() as u32;

    // Universal heap header packed into a single u64:
    //   refcount (u32) @ [0..32]   = 1 (irrelevant — rc_inc/dec no-op)
    //   type_tag (u16) @ [32..48]  = TAG_STR (= 0)
    //   flags    (u16) @ [48..64]  = STATIC_LITERAL | IS_LATIN1
    let flags_u16: u16 = STATIC_LITERAL_FLAG | STR_IS_LATIN1_FLAG;
    let header_u64: u64 = 1u64 | ((flags_u16 as u64) << 48);
    let hdr = i64_t.const_int(header_u64, false);
    let length_v = i32_t.const_int(length as u64, false);
    let pad_v = i32_t.const_int(0, false);
    let bytes_arr = ctx.const_string(bytes, false);

    // Anonymous struct so the layout exactly matches
    // `[u64 header, u32 length, u32 _pad, [N x i8]]` — the runtime
    // reads the header at offset 0, the length at offset 8, and
    // the payload bytes at offset 16.
    let body = ctx.const_struct(
        &[hdr.into(), length_v.into(), pad_v.into(), bytes_arr.into()],
        true, // packed — prevent LLVM from inserting padding between fields
    );
    let body_t = ctx.struct_type(
        &[
            i64_t.into(),
            i32_t.into(),
            i32_t.into(),
            i8_t.array_type(length).into(),
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
    /* 12-c-2 — primitive Copy data globals (I64 / I32 / F64 / Bool) get
     * `internal` linkage so LLVM's GlobalsAA + DSE can see no other
     * translation unit references them and promote
     * load-add-store-of-self loops to phi registers. Without `internal`,
     * `@i` stays as `external local_unnamed_addr global` and LLVM
     * conservatively keeps the per-iter `load @i / add 1 / store @i`
     * sequence (a 3-instr-per-iter memory dance) even when the phi
     * value is already available. Counter loops like array-sum-1m's
     * `while (i < N) { xs.push(i); i = i + 1; }` benefit directly.
     *
     * Refcount globals (Str / Arr / Obj at the bottom of this match)
     * stay External until a follow-up audits the exit-time drop hook
     * + the GlobalsAA interaction with rc_inc/rc_dec callsites.
     *
     * Safe because: torajs's `tr build` emits ONE LLVM module per
     * binary; the runtime staticlibs (libtorajs_*.a) don't reference
     * user-defined globals by name. */
    match g.ty {
        Type::I64 => {
            let t = ctx.i64_type();
            let glob = m.add_global(t, None, &g.name);
            glob.set_initializer(&t.const_int(0, false));
            glob.set_linkage(inkwell::module::Linkage::Internal);
            glob
        }
        Type::I32 => {
            let t = ctx.i32_type();
            let glob = m.add_global(t, None, &g.name);
            glob.set_initializer(&t.const_int(0, false));
            glob.set_linkage(inkwell::module::Linkage::Internal);
            glob
        }
        Type::F64 => {
            let t = ctx.f64_type();
            let glob = m.add_global(t, None, &g.name);
            glob.set_initializer(&t.const_float(0.0));
            glob.set_linkage(inkwell::module::Linkage::Internal);
            glob
        }
        Type::Bool => {
            let t = ctx.bool_type();
            let glob = m.add_global(t, None, &g.name);
            glob.set_initializer(&t.const_int(0, false));
            glob.set_linkage(inkwell::module::Linkage::Internal);
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
