//! SD-4c-prereq+e8-2b — runtime-exec probe for the cycle collector's
//! per-class `child_offsets` tables.
//!
//! Builds a tiny PIE binary with one `LinkConfig.class_layouts` entry
//! whose `child_offsets = [42]`. `_main` traces the runtime read path
//! the cycle collector uses: load the entry's inner-ptr slot (rebased
//! by dyld at startup via the e7b chained-fixups path the e8-2b wiring
//! extends to class_layouts), then read `offsets[0]` and return it as
//! the exit code. exit 42 → end-to-end e8-2b acceptance:
//!
//! - rodata placement: outer table + inner `.__class_offsets_0` global
//!   land inside `__DATA_CONST` past the (empty) vtable region
//! - sym registration: `__torajs_class_layouts` resolves to the outer
//!   table base vaddr via `apply_user_class_layouts_overrides`
//! - chained-fixup rebase: dyld walks the `__DATA_CONST` chain, finds
//!   the outer entry's inner-ptr slot, adds the ASLR slide, and stamps
//!   the runtime `.__class_offsets_0` vaddr into the slot
//! - runtime read: LDR x2, [x1, #8] picks up the rebased pointer;
//!   LDR w0, [x2, #0] reads the i32 = 42 from `.__class_offsets_0[0]`
//!
//! Run: `cargo run --release -p torajs-link --example probe_class_layouts_link`
//! then `/tmp/torajs_class_layouts_link; echo "EXIT=$?"`.

use std::fs;

use torajs_codegen::CompiledFunction;
use torajs_codegen::frame::FrameLayout;
use torajs_codegen::reloc::{Reloc, RelocKind};

use torajs_link::archive_emit::link_to_exec_with_archives;
use torajs_link::exec::{LinkConfig, UserClassLayoutEntry};
use torajs_link::resolve::SymTable;

/// _main reads the first `child_offsets` value through the rebased
/// outer-table inner-ptr slot and returns it as the process exit code.
fn build_main() -> CompiledFunction {
    // ADRP x1, page_of(__torajs_class_layouts)
    let adrp_x1: [u8; 4] = [0x01, 0x00, 0x00, 0x90];
    // ADD x1, x1, #pageoff(__torajs_class_layouts)
    let add_x1: [u8; 4] = [0x21, 0x00, 0x00, 0x91];
    // LDR x2, [x1, #8] — load outer entry[0].inner_ptr (rebased at load
    // time so it points at `.__class_offsets_0`).
    let ldr_x2_x1_8: [u8; 4] = [0x22, 0x04, 0x40, 0xF9];
    // LDR w0, [x2, #0] — load `.__class_offsets_0[0]` (i32 = 42).
    let ldr_w0_x2: [u8; 4] = [0x40, 0x00, 0x40, 0xB9];
    // RET — _main return; w0 = exit code in kernel ABI.
    let ret: [u8; 4] = [0xC0, 0x03, 0x5F, 0xD6];

    let mut bytes: Vec<u8> = Vec::with_capacity(20);
    bytes.extend_from_slice(&adrp_x1); // @0   ← Page21
    bytes.extend_from_slice(&add_x1); // @4   ← PageOff12
    bytes.extend_from_slice(&ldr_x2_x1_8); // @8
    bytes.extend_from_slice(&ldr_w0_x2); // @12
    bytes.extend_from_slice(&ret); // @16

    CompiledFunction {
        name: "_main".into(),
        bytes,
        relocs: vec![
            Reloc {
                byte_offset: 0,
                kind: RelocKind::Page21 {
                    target_sym: "___torajs_class_layouts".into(),
                },
            },
            Reloc {
                byte_offset: 4,
                kind: RelocKind::PageOff12 {
                    target_sym: "___torajs_class_layouts".into(),
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
        data_globals: Vec::new(),
        vtable_globals: Vec::new(),
        class_layouts: vec![UserClassLayoutEntry {
            child_offsets: vec![42],
            // W-J A3b — probe exercises only the child_offsets ptr path
            // (LDR x2, [x1, #8]); empty fields = NULL field_metadata_ptr,
            // no inner field-meta global, no extra rebase target.
            fields: Vec::new(),
            is_named: true,
            is_generic: false,
            methods: Vec::new(),
        }],
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
                "OK: link produced {} bytes — write to /tmp/torajs_class_layouts_link",
                bytes.len()
            );
            let path = "/tmp/torajs_class_layouts_link";
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
