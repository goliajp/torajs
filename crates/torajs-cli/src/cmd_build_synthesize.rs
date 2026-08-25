//! Hand-rolled entry-point + intrinsic stubs emitted by `tr build` when
//! the NEW pipeline (no LLVM IR backend) needs intrinsics that the
//! LLVM-era pipeline used to inline-emit from `ssa_inkwell` /
//! `obj_builders`. Extracted from `cmd_build.rs` to keep that file under
//! the 500-prod-LOC file-size hard limit (`rules/common/file-size.md`).
//!
//! Three intrinsics live here:
//!
//! - `synthesize_main_argv_wrapper` — `_main` wrapper that calls
//!   `___torajs_argv_init` before user main.
//! - `synthesize_obj_drop_sized` — `___torajs_obj_drop_sized` tail-call
//!   to `___torajs_libc_free`.
//! - `synthesize_obj_alloc` — `___torajs_obj_alloc` tail-call to
//!   `___torajs_libc_malloc`.
//!
//! The two entry symbol names (`_main` / `_main_user`) also live here
//! since they identify the entry pair this module emits. `cmd_build`
//! re-imports them via `pub(crate)` for `LinkConfig.entry` and the
//! user-main rename pass.

use torajs_codegen::CompiledFunction;
use torajs_codegen::enc::{add_imm, b_imm26, bl_imm26, ret as enc_ret, str_x_imm12};
use torajs_codegen::frame::FrameLayout;
use torajs_codegen::reg::Gpr;
use torajs_codegen::reloc::{CallTarget, Reloc, RelocKind};

/// Mach-O `MH_EXECUTE` entry-point symbol. ld64 / dyld both look up
/// `_main` (with the Apple Silicon underscore prefix); ssa_lower
/// emits the synthesized top-level wrapper as `"main"` so we rename
/// after compile to land at the right name in `LinkConfig.entry`.
pub(crate) const ENTRY_SYM: &str = "_main";

/// Renamed user-main sym — the `__torajs_main_entry` wrapper below
/// occupies the real `_main` symbol so it can run argv-init before
/// jumping to the user main body. Mirrors the LLVM-era
/// `ssa_inkwell::lower.rs` inserting a `__torajs_argv_init(argc,
/// argv, envp)` call at the top of `main`'s entry block.
pub(crate) const USER_MAIN_SYM: &str = "_main_user";

/// SD-4c gap2 — wrapper that occupies `_main` and forwards the
/// kernel-supplied `(argc=x0, argv=x1, envp=x2)` triple to
/// `__torajs_argv_init` before invoking the user main body.
///
/// r498: `init_argv = false` (no user reloc touches the
/// `__torajs_process_*` family — the only readers of the captured
/// argc/argv/envp globals, and `process` is a compiler-resolved
/// namespace with no runtime object to reach them dynamically)
/// emits the wrapper without the init call, so the whole
/// torajs-process member (argv registry + its Mutex machinery)
/// drops out of the artifact. Mirrors
/// `ssa_inkwell::lower.rs`'s entry-block call insertion, but at the
/// raw ARM64 level since the NEW pipeline has no LLVM IR backend.
///
/// The wrapper preserves x0..x2 across the init call by storing
/// them on stack and restoring before the user-main BL. Strictly
/// speaking only argv (x1) + envp (x2) need preservation post-init
/// since user main takes no args; we save all three for symmetry
/// + future-proofing. AAPCS64 §6.4 requires 16-byte sp alignment,
/// so the frame is 48 bytes (3 × 16 = 48, holding x29/x30 + the
/// saved triple).
///
/// ARM64 body (12 inst × 4 = 48 bytes):
///   stp  x29, x30, [sp, #-48]!   ; save fp/lr + alloca 48 bytes
///   mov  x29, sp
///   stp  x0,  x1,  [sp, #16]     ; preserve argc + argv (unused
///                                 ; post-init, kept for clarity)
///   str  x2,        [sp, #32]    ; preserve envp (unused post-init)
///   bl   ___torajs_argv_init     ; argc/argv/envp already in x0..2
///   bl   __main_user             ; user main(), returns i32 in x0
///   ldp  x29, x30, [sp], #48     ; restore fp/lr + free frame
///   ret
pub(crate) fn synthesize_main_argv_wrapper(init_argv: bool) -> CompiledFunction {
    if !init_argv {
        return synthesize_main_plain_wrapper();
    }
    let mut bytes = Vec::with_capacity(48);
    // stp x29, x30, [sp, #-48]!  — pre-index frame allocation
    //   encoding: 0xA9BD7BFD = stp x29, x30, [sp, #-48]!
    //   immediate field = (-48 / 8) as 7-bit signed = -6 = 0x7A
    //   full word: 1010 1001 1011 1010 0111 1011 1111 1101 = 0xA9BA7BFD
    // Easier to hand-pick the matching pre-index pattern. Use enc::
    // stp_pre_index when available; otherwise inline the constant.
    bytes.extend_from_slice(
        &torajs_codegen::enc::stp_pre_index(Gpr::X29, Gpr::X30, Gpr::SP, -48).to_le_bytes(),
    );
    // mov x29, sp  =  add x29, sp, #0
    bytes.extend_from_slice(&add_imm(Gpr::X29, Gpr::SP, 0).to_le_bytes());
    // str x0, [sp, #16]  / str x1, [sp, #24]
    bytes.extend_from_slice(&str_x_imm12(Gpr::X0, Gpr::SP, 16).to_le_bytes());
    bytes.extend_from_slice(&str_x_imm12(Gpr::X1, Gpr::SP, 24).to_le_bytes());
    // str x2, [sp, #32]
    bytes.extend_from_slice(&str_x_imm12(Gpr::X2, Gpr::SP, 32).to_le_bytes());
    // bl __torajs_argv_init (args already in x0/x1/x2 from dyld)
    let argv_init_bl_off = bytes.len() as u32;
    bytes.extend_from_slice(&bl_imm26(0).to_le_bytes());
    // bl _main_user (no args; returns i32 in x0)
    let user_main_bl_off = bytes.len() as u32;
    bytes.extend_from_slice(&bl_imm26(0).to_le_bytes());
    // ldp x29, x30, [sp], #48  — post-index frame release
    bytes.extend_from_slice(
        &torajs_codegen::enc::ldp_post_index(Gpr::X29, Gpr::X30, Gpr::SP, 48).to_le_bytes(),
    );
    // ret x30
    bytes.extend_from_slice(&enc_ret(Gpr::X30).to_le_bytes());
    CompiledFunction {
        name: ENTRY_SYM.to_string(),
        bytes,
        relocs: vec![
            Reloc {
                byte_offset: argv_init_bl_off,
                kind: RelocKind::CallSite {
                    target: CallTarget::Extern("___torajs_argv_init".into()),
                },
            },
            Reloc {
                byte_offset: user_main_bl_off,
                kind: RelocKind::CallSite {
                    target: CallTarget::Extern(USER_MAIN_SYM.into()),
                },
            },
        ],
        // The wrapper saves its own fp/lr inline; the frame layout is
        // a hand-built stp/ldp pair, not the standard leaf prologue.
        // Mark as `leaf_no_spill` so codegen's epilogue emit doesn't
        // try to wrap this fn — it's already complete.
        frame: FrameLayout::leaf_no_spill(),
    }
}

/// The `init_argv = false` shape of the `_main` wrapper: no
/// argc/argv/envp preservation, no init call — just the AAPCS64
/// frame around the user-main BL (5 inst × 4 = 20 bytes):
///   stp  x29, x30, [sp, #-16]!
///   mov  x29, sp
///   bl   __main_user
///   ldp  x29, x30, [sp], #16
///   ret
fn synthesize_main_plain_wrapper() -> CompiledFunction {
    let mut bytes = Vec::with_capacity(20);
    bytes.extend_from_slice(
        &torajs_codegen::enc::stp_pre_index(Gpr::X29, Gpr::X30, Gpr::SP, -16).to_le_bytes(),
    );
    bytes.extend_from_slice(&add_imm(Gpr::X29, Gpr::SP, 0).to_le_bytes());
    let user_main_bl_off = bytes.len() as u32;
    bytes.extend_from_slice(&bl_imm26(0).to_le_bytes());
    bytes.extend_from_slice(
        &torajs_codegen::enc::ldp_post_index(Gpr::X29, Gpr::X30, Gpr::SP, 16).to_le_bytes(),
    );
    bytes.extend_from_slice(&enc_ret(Gpr::X30).to_le_bytes());
    CompiledFunction {
        name: ENTRY_SYM.to_string(),
        bytes,
        relocs: vec![Reloc {
            byte_offset: user_main_bl_off,
            kind: RelocKind::CallSite {
                target: CallTarget::Extern(USER_MAIN_SYM.into()),
            },
        }],
        // Same hand-built frame rationale as the argv shape above.
        frame: FrameLayout::leaf_no_spill(),
    }
}

/// SD-4c-prereq swap-2d — `__torajs_obj_drop_sized(user_ptr, size) -> void`.
/// The LLVM-era `obj_builders::define_obj_drop_sized` inlined a TLAB
/// fast path mirroring `define_obj_alloc`'s TLAB pop. The new pipeline
/// has no LLVM-IR emit backend; emit the intrinsic directly as a
/// hand-rolled CompiledFunction that tail-calls `___torajs_libc_free`
/// (the slow path). Loses the TLAB hot-loop optimization until a real
/// port lands (swap-3+ backlog: TLAB.push fast path in native ARM64);
/// gains correct drop semantics — every block makes it back to the
/// allocator instead of leaking.
///
/// ARM64 body:
///   `B ___torajs_libc_free`   ; tail call — x0 = user_ptr is already in
///                              ; the right register, x1 = size is
///                              ; discarded by libc_free's signature
///                              ; (which is `void(ptr)`).
pub(crate) fn synthesize_obj_drop_sized() -> CompiledFunction {
    let mut bytes = Vec::with_capacity(4);
    let reloc_offset = bytes.len() as u32;
    bytes.extend_from_slice(&b_imm26(0).to_le_bytes());
    CompiledFunction {
        name: "___torajs_obj_drop_sized".into(),
        bytes,
        relocs: vec![Reloc {
            byte_offset: reloc_offset,
            kind: RelocKind::CallSite {
                target: CallTarget::Extern("___torajs_libc_free".into()),
            },
        }],
        frame: FrameLayout::leaf_no_spill(),
    }
}

/// SD-4c-prereq swap-2h — `__torajs_obj_alloc(size) -> ptr`.
/// The LLVM-era `obj_builders::define_obj_alloc` inlined a TLAB
/// fast path (size-class bucket → TLAB.pop → return slot+16) with a
/// fallback to `___torajs_libc_malloc(size)`. The new pipeline has no
/// LLVM-IR emit backend; emit the intrinsic directly as a hand-rolled
/// CompiledFunction that tail-calls the fallback. Loses the TLAB
/// hot-loop optimization until a native ARM64 TLAB.pop port lands
/// (swap-3+ backlog, paired with `synthesize_obj_drop_sized`'s
/// TLAB.push); gains correct alloc semantics — `___torajs_libc_malloc`
/// already produces the 16-byte-header layout `obj_alloc` returns, so
/// the tail call is byte-for-byte the inline fallback path.
///
/// ARM64 body:
///   `B ___torajs_libc_malloc`  ; tail call — x0 = size is already in
///                              ; the right register, return ptr in x0
///                              ; flows straight back to the caller.
pub(crate) fn synthesize_obj_alloc() -> CompiledFunction {
    let mut bytes = Vec::with_capacity(4);
    let reloc_offset = bytes.len() as u32;
    bytes.extend_from_slice(&b_imm26(0).to_le_bytes());
    CompiledFunction {
        name: "___torajs_obj_alloc".into(),
        bytes,
        relocs: vec![Reloc {
            byte_offset: reloc_offset,
            kind: RelocKind::CallSite {
                target: CallTarget::Extern("___torajs_libc_malloc".into()),
            },
        }],
        frame: FrameLayout::leaf_no_spill(),
    }
}
