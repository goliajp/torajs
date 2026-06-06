//! SD-4c-prereq+e7a — vtable-global (`__vtable_<C>`) link probe
//! (ABI / layout validation only — runtime exec is e7b territory).
//!
//! Codegen's `GlobalRef("__vtable_<C>")` resolves to the vtable's
//! base vaddr; runtime `__dispatch_<M>` indexes by method offset to
//! load the fn ptr and BLR's it. e7a emits each
//! `LinkConfig.vtable_globals` entry as an `[N x ptr]` rodata block
//! sharing `__TEXT,__cstring` with `strings`, registers the sym
//! override, and resolves each slot through the effective sym table.
//!
//! Why no exec test: each rodata slot is an absolute pointer, which
//! a PIE binary needs `LC_DYLD_CHAINED_FIXUPS` rebase entries to
//! correct after the kernel's ASLR slide. Rebase chain emission is
//! the e7b sub-step; this probe verifies layout / sym registration
//! / payload byte layout only.
//!
//! Acceptance: `OK: link produced N bytes`, nm lists `__vtable_A`,
//! and the bytes at `__vtable_A` decode as a u64 LE = vaddr of
//! `__torajs_fn_1` (= `_helper`).
//!
//! Run: `cargo run --release -p torajs-link --example probe_vtable_link`

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

/// _main returns 0 directly — exec validation is e7b. The ADRP+ADD
/// against `__vtable_A` is left in to exercise the e7a sym vaddr
/// override path.
fn build_main() -> CompiledFunction {
    let adrp_x1: [u8; 4] = [0x01, 0x00, 0x00, 0x90];
    let add_x1_pageoff: [u8; 4] = [0x21, 0x00, 0x00, 0x91];
    let movz_x0_0: [u8; 4] = [0x00, 0x00, 0x80, 0xD2];
    let ret: [u8; 4] = [0xC0, 0x03, 0x5F, 0xD6];

    let mut bytes: Vec<u8> = Vec::with_capacity(16);
    bytes.extend_from_slice(&adrp_x1); // @0   ← Page21
    bytes.extend_from_slice(&add_x1_pageoff); // @4   ← PageOff12
    bytes.extend_from_slice(&movz_x0_0); // @8
    bytes.extend_from_slice(&ret); // @12

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
        archives: Vec::new(),
        strings: Vec::new(),
        data_globals: Vec::new(),
        vtable_globals: vec![UserVtableEntry {
            sym: "__vtable_A".into(),
            slot_syms: vec![Some("__torajs_fn_1".into())],
        }],
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
