//! Compiler-emitted dispatch-family stubs (RFC 20260824-s2-5
//! Phase B blade 2b).
//!
//! Each stub is one `b __torajs_dispatch_stub_reject` (4 bytes + a
//! BRANCH26 reloc, argument registers pass through untouched) named
//! after a family's arm seam. Being a user-`.o` definition it wins
//! symbol resolution over the `torajs-dispatch` default arm
//! (`register_user_fn_syms` + the required-members walk are both
//! user-first), the default arm atom loses its in-edge, and the
//! family kernel dead-strips.
//!
//! Gate for this pre-blade: the `TORAJS_DISPATCH_STUB_ALL` env var
//! (pricing / diagnosis ONLY — it stubs every family uncondition-
//! ally, so any program that actually enters the any-method
//! dispatcher will throw the stub TypeError). The emitted-mid
//! judgment that turns this on automatically and soundly is blade
//! 2b proper; until then the default path appends nothing and the
//! artifact is byte-identical.

use torajs_codegen::CompiledFunction;
use torajs_codegen::frame::FrameLayout;
use torajs_codegen::reloc::{CallTarget, Reloc, RelocKind};

/// Mach-O names of the fifteen family arm seams (see
/// `torajs-anyvalue/src/dispatch_seam.rs`).
const FAMILY_ARMS: [&str; 15] = [
    "___torajs_dispatch_str_arm",
    "___torajs_dispatch_arr_arm",
    "___torajs_dispatch_dynobj_arm",
    "___torajs_dispatch_struct_arm",
    "___torajs_dispatch_mapset_arm",
    "___torajs_dispatch_iter_arm",
    "___torajs_dispatch_buffer_arm",
    "___torajs_dispatch_date_arm",
    "___torajs_dispatch_promise_arm",
    "___torajs_dispatch_regexp_arm",
    "___torajs_dispatch_bigint_arm",
    "___torajs_dispatch_symbol_arm",
    "___torajs_dispatch_closure_arm",
    "___torajs_dispatch_weak_arm",
    "___torajs_dispatch_num_arm",
];

/// Per-family printer / inspect kernels (defined in their family's
/// provider crate, referenced by anyvalue's inspect dispatch as
/// undef relocs). Stubbing one shadows the provider definition the
/// same way an arm stub shadows the torajs-dispatch default — the
/// provider atom loses its in-edges and the family's print world
/// strips. Reaching a stub at runtime is impossible in a correctly
/// judged program: a family that is stubbed has no values of that
/// tag to print.
const FAMILY_PRINTERS: [&str; 19] = [
    "___torajs_regex_print_inline",
    "___torajs_map_print",
    "___torajs_set_print",
    "___torajs_map_print_at",
    "___torajs_set_print_at",
    "___torajs_promise_print",
    "___torajs_date_to_iso_string",
    "___torajs_arraybuffer_print",
    "___torajs_typedarray_print",
    "___torajs_dataview_print",
    "___torajs_bigint_print_inline",
    "___torajs_symbol_print_inline",
    "___torajs_fn_print_inline",
    "___torajs_anyv_struct_print_inline",
    "___torajs_anyv_struct_print_inline_at",
    "___torajs_arr_print_any",
    "___torajs_arr_print_any_at",
    "___torajs_obj_print_any",
    "___torajs_obj_print_any_at",
];

/// One loud-reject stub per family in `arms`, appended to the user
/// fn list. `0x14000000` is the bare `b` opcode; the link pass
/// patches its imm26 like any BRANCH26 site (the opcode bits are
/// preserved), so the stub tail-branches into
/// `__torajs_dispatch_stub_reject` with x0-x5 untouched.
pub(crate) fn append_dispatch_stubs(funcs: &mut Vec<CompiledFunction>) {
    for name in FAMILY_ARMS.into_iter().chain(FAMILY_PRINTERS) {
        funcs.push(CompiledFunction {
            name: name.into(),
            bytes: vec![0x00, 0x00, 0x00, 0x14],
            relocs: vec![Reloc {
                byte_offset: 0,
                kind: RelocKind::CallSite {
                    target: CallTarget::Extern("___torajs_dispatch_stub_reject".into()),
                },
            }],
            frame: FrameLayout::leaf_no_spill(),
        });
    }
}

/// Pricing/diagnosis gate — see the module doc.
pub(crate) fn stub_all_enabled() -> bool {
    std::env::var_os("TORAJS_DISPATCH_STUB_ALL").is_some_and(|v| v != "0")
}
