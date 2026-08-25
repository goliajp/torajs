//! SD-4c-prereq+e4 — data-global (`__DATA,__bss`) link probe.
//!
//! Codegen's `GlobalRef(name)` emits ADRP+ADD against the user-supplied
//! `name` verbatim. e4 materializes each `LinkConfig.data_globals` entry
//! as one slot in the binary's `__DATA,__bss` zerofill section and
//! registers `sym → vaddr` in the effective sym table so the ADRP+ADD
//! pair lands on the right slot vaddr.
//!
//! _main: takes the I64 global's address (ADRP+ADD), stores 42 into
//! the slot, loads it back, and returns the loaded value as the exit
//! code (X0 propagates through `ret` to the kernel).
//!
//! Run: `cargo run --release -p torajs-link --example probe_data_global_link`

use std::fs;

use torajs_codegen::CompiledFunction;
use torajs_codegen::frame::FrameLayout;
use torajs_codegen::reloc::{CallTarget, Reloc, RelocKind};

use torajs_link::archive_emit::link_to_exec_with_archives;
use torajs_link::exec::{LinkConfig, UserDataGlobalEntry};
use torajs_link::resolve::SymTable;

/// _main:
///   stp x29, x30, [sp, #-16]!
///   adrp x1, global_x         ← Page21 reloc
///   add  x1, x1, :lo12:global_x  ← PageOff12 reloc
///   movz x0, #42                 ← value to store
///   str  x0, [x1]                ← STR X0, [X1] = 0xF9000020
///   ldr  x0, [x1]                ← LDR X0, [X1] = 0xF9400020
///   bl   _free                   ← force has_dyld true (e4 guards
///                                  data_globals against !has_dyld;
///                                  any libSystem ref satisfies it).
///                                  X0 isn't clobbered by _free
///                                  receiving anything — we instead
///                                  call after loading and store X0
///                                  on the stack. SIMPLER: just call
///                                  _getpid() and ignore its result.
///   ldp  x29, x30, [sp], #16
///   ret
///
/// We use a libSystem call to force the has_dyld path; we save X0 on
/// the stack across the call to keep our 42 in the return register.
fn build_main() -> CompiledFunction {
    // stp x29, x30, [sp, #-32]!  - reserve 32 B (16 for FP/LR, 16 for x0 spill)
    let stp_pre_32: [u8; 4] = [0xFD, 0x7B, 0xBE, 0xA9]; // 0xA9BE7BFD
    let adrp_x1: [u8; 4] = [0x01, 0x00, 0x00, 0x90];
    let add_x1_pageoff: [u8; 4] = [0x21, 0x00, 0x00, 0x91];
    // MOVZ X0, #42 = 0xD2800540
    let movz_x0_42: [u8; 4] = [0x40, 0x05, 0x80, 0xD2];
    // STR X0, [X1] = 0xF9000020
    let str_x0_x1: [u8; 4] = [0x20, 0x00, 0x00, 0xF9];
    // LDR X0, [X1] = 0xF9400020
    let ldr_x0_x1: [u8; 4] = [0x20, 0x00, 0x40, 0xF9];
    // STR X0, [sp, #16] = 0xF90008E0 (saves loaded value across libSystem call)
    let str_x0_spill: [u8; 4] = [0xE0, 0x0B, 0x00, 0xF9];
    let bl_placeholder: [u8; 4] = [0x00, 0x00, 0x00, 0x94];
    // LDR X0, [sp, #16] = 0xF94008E0 (restore loaded value into X0)
    let ldr_x0_spill: [u8; 4] = [0xE0, 0x0B, 0x40, 0xF9];
    // ldp x29, x30, [sp], #32 = 0xA8C27BFD
    let ldp_post_32: [u8; 4] = [0xFD, 0x7B, 0xC2, 0xA8];
    let ret: [u8; 4] = [0xC0, 0x03, 0x5F, 0xD6];

    let mut bytes: Vec<u8> = Vec::with_capacity(44);
    bytes.extend_from_slice(&stp_pre_32); // @0
    bytes.extend_from_slice(&adrp_x1); // @4   ← Page21
    bytes.extend_from_slice(&add_x1_pageoff); // @8   ← PageOff12
    bytes.extend_from_slice(&movz_x0_42); // @12
    bytes.extend_from_slice(&str_x0_x1); // @16
    bytes.extend_from_slice(&ldr_x0_x1); // @20
    bytes.extend_from_slice(&str_x0_spill); // @24
    bytes.extend_from_slice(&bl_placeholder); // @28  ← BL _getpid
    bytes.extend_from_slice(&ldr_x0_spill); // @32
    bytes.extend_from_slice(&ldp_post_32); // @36
    bytes.extend_from_slice(&ret); // @40

    CompiledFunction {
        name: "_main".into(),
        bytes,
        relocs: vec![
            Reloc {
                byte_offset: 4,
                kind: RelocKind::Page21 {
                    target_sym: "global_x".into(),
                },
            },
            Reloc {
                byte_offset: 8,
                kind: RelocKind::PageOff12 {
                    target_sym: "global_x".into(),
                },
            },
            Reloc {
                byte_offset: 28,
                kind: RelocKind::CallSite {
                    target: CallTarget::Extern("_getpid".into()),
                },
            },
        ],
        frame: FrameLayout::leaf_no_spill(),
    }
}

fn main() {
    let cfg = LinkConfig {
        funcs: vec![build_main()],
        entry: "_main".into(),
        sym_table: SymTable::new(),
        codesign_ident: "tora".into(),
        dead_strip: false,
        strip_member_symbols: false,
        archives: Vec::new(),
        strings: Vec::new(),
        data_globals: vec![UserDataGlobalEntry {
            sym: "global_x".into(),
            size: 8,
            align_log2: 3,
        }],
        vtable_globals: Vec::new(),
        class_layouts: Vec::new(),
        force_emit_class_layouts_globals: false,
        fn_name_globals: Vec::new(),
        force_emit_fn_name_globals: false,
        class_names: Vec::new(),
        force_emit_class_names_globals: false,
        baked_regex_entries: Vec::new(),
    };

    match link_to_exec_with_archives(&cfg) {
        Ok(bytes) => {
            println!(
                "OK: link produced {} bytes — write to /tmp/torajs_data_global_link and exec to verify",
                bytes.len()
            );
            let path = "/tmp/torajs_data_global_link";
            fs::write(path, &bytes).expect("write");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = fs::metadata(path).unwrap().permissions();
                perms.set_mode(0o755);
                fs::set_permissions(path, perms).unwrap();
            }
            eprintln!("binary at {path}");
        }
        Err(e) => {
            println!("ERR: {e:?}");
        }
    }
}
