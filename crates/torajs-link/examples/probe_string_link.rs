//! SD-4c-prereq+e1 — string-link probe.
//!
//! Hand-builds a `_main` that loads the rodata Str payload at
//! `__torajs_str_lit_0` (provided via `LinkConfig.strings`), then
//! calls `_write(1, payload, 5)` to print "hello" and exits 0.
//!
//! Exercises the e1 emit path end-to-end: `LinkConfig.strings`
//! materializes a `__TEXT,__cstring` rodata Str, the sym table
//! learns the entry's vaddr, and codegen-emitted ADRP+ADD pairs
//! resolve to the payload start.
//!
//! Run: `cargo run --release -p torajs-link --example probe_string_link`

use std::fs;

use torajs_codegen::CompiledFunction;
use torajs_codegen::frame::FrameLayout;
use torajs_codegen::reloc::{CallTarget, Reloc, RelocKind};

use torajs_link::archive_emit::link_to_exec_with_archives;
use torajs_link::exec::{LinkConfig, UserStringEntry, UserStringKind};
use torajs_link::resolve::SymTable;

/// Hand-encoded aarch64 `_main`:
///   stp x29, x30, [sp, #-16]!
///   adrp x1, __torajs_str_lit_0       ← Page21 reloc
///   add  x1, x1, :lo12:__torajs_str_lit_0   ← PageOff12 reloc
///   add  x1, x1, #16                   ← skip [u64 header, u32 length, u32 _pad]
///   movz x0, #1                        ← stdout fd
///   movz x2, #5                        ← byte count
///   bl   _write                        ← CallSite reloc (libSystem)
///   movz x0, #0                        ← exit code
///   ldp  x29, x30, [sp], #16
///   ret
fn build_main() -> CompiledFunction {
    let stp_pre: [u8; 4] = [0xFD, 0x7B, 0xBF, 0xA9];
    // ADRP X1, #0 placeholder = 0x90000001
    let adrp_x1: [u8; 4] = [0x01, 0x00, 0x00, 0x90];
    // ADD X1, X1, #0 placeholder = 0x91000021
    let add_x1_pageoff: [u8; 4] = [0x21, 0x00, 0x00, 0x91];
    // ADD X1, X1, #16 = 0x91004021 (imm12=0x10)
    let add_x1_16: [u8; 4] = [0x21, 0x40, 0x00, 0x91];
    // MOVZ X0, #1 = 0xD2800020
    let movz_x0_1: [u8; 4] = [0x20, 0x00, 0x80, 0xD2];
    // MOVZ X2, #5 = 0xD28000A2
    let movz_x2_5: [u8; 4] = [0xA2, 0x00, 0x80, 0xD2];
    // BL placeholder = 0x94000000
    let bl_placeholder: [u8; 4] = [0x00, 0x00, 0x00, 0x94];
    // MOVZ X0, #0 = 0xD2800000
    let movz_x0_0: [u8; 4] = [0x00, 0x00, 0x80, 0xD2];
    let ldp_post: [u8; 4] = [0xFD, 0x7B, 0xC1, 0xA8];
    let ret: [u8; 4] = [0xC0, 0x03, 0x5F, 0xD6];

    let mut bytes: Vec<u8> = Vec::with_capacity(40);
    bytes.extend_from_slice(&stp_pre); // @0
    bytes.extend_from_slice(&adrp_x1); // @4   ← Page21
    bytes.extend_from_slice(&add_x1_pageoff); // @8   ← PageOff12
    bytes.extend_from_slice(&add_x1_16); // @12
    bytes.extend_from_slice(&movz_x0_1); // @16
    bytes.extend_from_slice(&movz_x2_5); // @20
    bytes.extend_from_slice(&bl_placeholder); // @24  ← BL _write
    bytes.extend_from_slice(&movz_x0_0); // @28
    bytes.extend_from_slice(&ldp_post); // @32
    bytes.extend_from_slice(&ret); // @36

    CompiledFunction {
        name: "_main".into(),
        bytes,
        relocs: vec![
            Reloc {
                byte_offset: 4,
                kind: RelocKind::Page21 {
                    target_sym: "__torajs_str_lit_0".into(),
                },
            },
            Reloc {
                byte_offset: 8,
                kind: RelocKind::PageOff12 {
                    target_sym: "__torajs_str_lit_0".into(),
                },
            },
            Reloc {
                byte_offset: 24,
                kind: RelocKind::CallSite {
                    target: CallTarget::Extern("_write".into()),
                },
            },
        ],
        frame: FrameLayout::leaf_no_spill(),
    }
}

fn main() {
    let main_cf = build_main();
    let cfg = LinkConfig {
        funcs: vec![main_cf],
        entry: "_main".into(),
        sym_table: SymTable::new(),
        codesign_ident: "tora".into(),
        dead_strip: false,
        strip_member_symbols: false,
        elidable_sites: Vec::new(),
        guarded_stubs: Vec::new(),
        archives: Vec::new(),
        strings: vec![UserStringEntry {
            sym: "__torajs_str_lit_0".into(),
            bytes: b"hello".to_vec(),
            is_latin1: true,
            length: 5,
            kind: UserStringKind::StaticStr,
        }],
        data_globals: Vec::new(),
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
                "OK: link produced {} bytes — write to /tmp/torajs_string_link and exec to verify",
                bytes.len()
            );
            let path = "/tmp/torajs_string_link";
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
