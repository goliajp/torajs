//! SD-4c-prereq+e7b — vtable-global (`__vtable_<C>`) runtime-exec probe.
//!
//! Codegen's `GlobalRef("__vtable_<C>")` resolves to the vtable's
//! base vaddr; runtime `__dispatch_<M>` indexes by method offset to
//! load the fn ptr and BLR's it. e7a wired emission of each
//! `LinkConfig.vtable_globals` entry as an `[N x ptr]` rodata block
//! sharing `__TEXT,__cstring` with `strings`, registered the sym
//! override, and resolved each slot through the effective sym table.
//!
//! e7b adds the missing piece: each rodata slot is an absolute
//! pointer that the PIE binary needs `LC_DYLD_CHAINED_FIXUPS` rebase
//! entries to correct after the kernel's ASLR slide. With the rebase
//! chain emission in place (`chained_fixups_starts` + `TextRebaseScope`
//! + multi-segment page-start tables), dyld walks the `__TEXT` chain
//! at load time, adds the slide, and stamps the runtime `_helper`
//! address into slot 0 — so the indirect `BR x2` lands inside
//! `_helper` regardless of ASLR.
//!
//! Acceptance: `OK: link produced N bytes`; the resulting binary at
//! `/tmp/torajs_vtable_link` exits with **42** because `_main` does
//! `ADRP+ADD __vtable_A` → `LDR x2,[x1,#0]` → `BR x2` → `_helper`
//! sets x0 = 42 and `RET`s (no LR pushed by `BR`, so `_helper`'s
//! return lands in dyld's main wrapper, which treats x0 as the
//! exit code).
//!
//! Run: `cargo run --release -p torajs-link --example probe_vtable_link`
//! then `/tmp/torajs_vtable_link; echo "EXIT=$?"`.

use std::fs;

use torajs_codegen::CompiledFunction;
use torajs_codegen::frame::FrameLayout;
use torajs_codegen::reloc::{Reloc, RelocKind};

use torajs_link::archive_emit::link_to_exec_with_archives;
use torajs_link::exec::{LinkConfig, UserVtableEntry};
use torajs_link::resolve::SymTable;

fn build_helper() -> CompiledFunction {
    let movz_x0_42: [u8; 4] = [0x40, 0x05, 0x80, 0xD2];
    let ret: [u8; 4] = [0xC0, 0x03, 0x5F, 0xD6];
    let mut bytes = Vec::with_capacity(8);
    bytes.extend_from_slice(&movz_x0_42);
    bytes.extend_from_slice(&ret);
    CompiledFunction {
        name: "_helper".into(),
        bytes,
        relocs: Vec::new(),
        frame: FrameLayout::leaf_no_spill(),
    }
}

/// _main loads `__vtable_A` base into x1, loads the first slot into
/// x2, then tail-branches there. dyld rebases the slot at load time
/// (chained fixup), so BR lands in `_helper` regardless of ASLR
/// slide; `_helper` then exits 42.
fn build_main() -> CompiledFunction {
    let adrp_x1: [u8; 4] = [0x01, 0x00, 0x00, 0x90];
    let add_x1_pageoff: [u8; 4] = [0x21, 0x00, 0x00, 0x91];
    // LDR x2, [x1, #0] unsigned-offset 64-bit.
    let ldr_x2_x1: [u8; 4] = [0x22, 0x00, 0x40, 0xF9];
    // BR x2 (unconditional indirect tail branch).
    let br_x2: [u8; 4] = [0x40, 0x00, 0x1F, 0xD6];

    let mut bytes: Vec<u8> = Vec::with_capacity(16);
    bytes.extend_from_slice(&adrp_x1); // @0   ← Page21
    bytes.extend_from_slice(&add_x1_pageoff); // @4   ← PageOff12
    bytes.extend_from_slice(&ldr_x2_x1); // @8   x2 = *(x1 + 0)
    bytes.extend_from_slice(&br_x2); // @12  jump through x2

    CompiledFunction {
        name: "_main".into(),
        bytes,
        relocs: vec![
            Reloc {
                byte_offset: 0,
                kind: RelocKind::Page21 {
                    target_sym: "__vtable_A".into(),
                },
            },
            Reloc {
                byte_offset: 4,
                kind: RelocKind::PageOff12 {
                    target_sym: "__vtable_A".into(),
                },
            },
        ],
        frame: FrameLayout::leaf_no_spill(),
    }
}

fn main() {
    let cfg = LinkConfig {
        funcs: vec![build_main(), build_helper()],
        entry: "_main".into(),
        sym_table: SymTable::new(),
        codesign_ident: "tora".into(),
        dead_strip: false,
        strip_member_symbols: false,
        archives: Vec::new(),
        strings: Vec::new(),
        data_globals: Vec::new(),
        vtable_globals: vec![UserVtableEntry {
            sym: "__vtable_A".into(),
            slot_syms: vec![Some("__torajs_fn_1".into())],
        }],
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
                "OK: link produced {} bytes — write to /tmp/torajs_vtable_link",
                bytes.len()
            );
            let path = "/tmp/torajs_vtable_link";
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
