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

/// The ns-static MINT seam (`torajs-anyvalue`'s minting sites call
/// it; `torajs-dispatch` owns the default). Stubbed for programs
/// that provably never put a namespace / ctor / globalThis object
/// into the any world — the whole ns-static dispatch universe
/// (boxed entry + DISPATCH table + every static's kernel)
/// dead-strips. fam_id 15 (the free slot between the arm roster
/// and the printers).
const NS_STATIC_SEAM: &str = "___torajs_ns_static_cell";

/// True when `ssa_name` (no Mach-O underscore) is one of the arm
/// seams — the judgment scan refuses to stub a family whose seam
/// the user `.o` somehow references directly.
pub(crate) fn is_arm_sym(ssa_name: &str) -> bool {
    FAMILY_ARMS.iter().any(|a| &a[1..] == ssa_name)
}

/// True when `ssa_name` is one of the printer kernels — a direct
/// typed-lane reference (e.g. `console.log(d)` emitting
/// `__torajs_date_to_iso_string`) keeps the whole print world.
pub(crate) fn is_printer_sym(ssa_name: &str) -> bool {
    FAMILY_PRINTERS.iter().any(|a| &a[1..] == ssa_name)
}

/// One loud-reject stub per selected family arm (bit i of
/// `arm_bits` = `FAMILY_ARMS[i]`, lockstep with
/// `torajs_rc::any_method_family`'s bit order) plus, when
/// `printers` is set, every per-family printer kernel — appended to
/// the user fn list. `0x14000000` is the bare `b` opcode; the link
/// pass patches its imm26 like any BRANCH26 site (the opcode bits
/// are preserved), so the stub tail-branches into
/// `__torajs_dispatch_stub_reject` with x0-x5 untouched.
pub(crate) fn append_dispatch_stubs(
    funcs: &mut Vec<CompiledFunction>,
    arm_bits: u16,
    ns_static: bool,
    printers: bool,
) {
    let arms = FAMILY_ARMS
        .into_iter()
        .enumerate()
        .filter(|(i, _)| arm_bits & (1 << i) != 0);
    let ns_tagged = ns_static.then_some((15, NS_STATIC_SEAM)).into_iter();
    // the ctor-call entry's Date arm strips with the date family
    // (bit 7): a Date ctor cell cannot be read off any receiver in
    // a program with no date values, so the arm shares the family's
    // judgment (and its landing-pad attribution).
    let ctor_date = (arm_bits & (1 << 7) != 0)
        .then_some((7, "___torajs_ctor_date_call"))
        .into_iter();
    let printer_syms = if printers { &FAMILY_PRINTERS[..] } else { &[] };
    let printers_tagged = printer_syms.iter().enumerate().map(|(i, n)| (16 + i, *n));
    for (fam_id, name) in arms
        .chain(ns_tagged)
        .chain(ctor_date)
        .chain(printers_tagged)
    {
        let (bytes, relocs) = reject_stub_body(fam_id as u32);
        funcs.push(CompiledFunction {
            name: name.into(),
            bytes,
            relocs,
            frame: FrameLayout::leaf_no_spill(),
        });
    }
}

/// The loud-reject stub body: `movz x7, #fam_id` (the unused 8th
/// C-ABI arg slot, so the landing pad can NAME the stripped family
/// in its TypeError — x0-x5 pass through untouched) + a bare `b`
/// whose BRANCH26 reloc the link pass patches into
/// `__torajs_dispatch_stub_reject`. Shared with the link-judged
/// stubs (`cmd_build_elide`), whose fam ids start at 40.
pub(crate) fn reject_stub_body(fam_id: u32) -> (Vec<u8>, Vec<Reloc>) {
    let movz: u32 = 0xD280_0000 | (fam_id << 5) | 7;
    let mut bytes = movz.to_le_bytes().to_vec();
    bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x14]);
    let relocs = vec![Reloc {
        byte_offset: 4,
        kind: RelocKind::CallSite {
            target: CallTarget::Extern("___torajs_dispatch_stub_reject".into()),
        },
    }];
    (bytes, relocs)
}

/// Pricing/diagnosis override — forces every family stubbed
/// regardless of the judgment (see the module doc).
pub(crate) fn stub_all_enabled() -> bool {
    std::env::var_os("TORAJS_DISPATCH_STUB_ALL").is_some_and(|v| v != "0")
}

/// Kill switch — disables the blade-2b automatic judgment (emits no
/// stubs), for isolating a suspected wrong judgment in the field.
pub(crate) fn stub_off() -> bool {
    std::env::var_os("TORAJS_DISPATCH_STUB_OFF").is_some_and(|v| v != "0")
}
