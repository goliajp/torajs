//! SD-4c-prereq+e3 — function-pointer (FnAddr) link probe.
//!
//! Codegen's `FnAddr(FuncId(n))` emits ADRP+ADD against the parallel
//! naming scheme `__torajs_fn_<n>`. Without e3's
//! `register_fn_addr_syms`, that sym is unresolved at link time. e3
//! injects one alias per user fn (`__torajs_fn_<i>` → `fn_vaddrs[i]`)
//! so the ADRP+ADD pair lands on the right function entry.
//!
//! Layout:
//!   funcs[0] = _main:    builds a fn-ptr to _helper, indirect-calls it
//!   funcs[1] = _helper:  movz x0, #42; ret   (returns 42)
//!
//! exec → exit code 42 (x0 propagates as the process exit status).
//!
//! Run: `cargo run --release -p torajs-link --example probe_fn_addr_link`

use std::fs;

use torajs_codegen::CompiledFunction;
use torajs_codegen::frame::FrameLayout;
use torajs_codegen::reloc::{Reloc, RelocKind};

use torajs_link::archive_emit::link_to_exec_with_archives;
use torajs_link::exec::LinkConfig;
use torajs_link::resolve::SymTable;

/// _helper: `movz x0, #42; ret`. Leaf — no prologue/epilogue.
fn build_helper() -> CompiledFunction {
    // MOVZ X0, #42 = 0xD2800540 → [0x40, 0x05, 0x80, 0xD2]
    let movz_x0_42: [u8; 4] = [0x40, 0x05, 0x80, 0xD2];
    let ret: [u8; 4] = [0xC0, 0x03, 0x5F, 0xD6];

    let mut bytes: Vec<u8> = Vec::with_capacity(8);
    bytes.extend_from_slice(&movz_x0_42);
    bytes.extend_from_slice(&ret);

    CompiledFunction {
        name: "_helper".into(),
        bytes,
        relocs: Vec::new(),
        frame: FrameLayout::leaf_no_spill(),
    }
}

/// _main: non-leaf wrapper that loads `__torajs_fn_1` into X1 and
/// calls it via BLR, then returns whatever X0 holds.
///
///   stp x29, x30, [sp, #-16]!
///   adrp x1, __torajs_fn_1     ← Page21 reloc
///   add  x1, x1, :lo12:__torajs_fn_1  ← PageOff12 reloc
///   blr  x1                    ← indirect call, X0 = _helper result
///   ldp  x29, x30, [sp], #16
///   ret
fn build_main() -> CompiledFunction {
    let stp_pre: [u8; 4] = [0xFD, 0x7B, 0xBF, 0xA9];
    let adrp_x1: [u8; 4] = [0x01, 0x00, 0x00, 0x90];
    let add_x1_pageoff: [u8; 4] = [0x21, 0x00, 0x00, 0x91];
    // BLR X1 = 0xD63F0020
    let blr_x1: [u8; 4] = [0x20, 0x00, 0x3F, 0xD6];
    let ldp_post: [u8; 4] = [0xFD, 0x7B, 0xC1, 0xA8];
    let ret: [u8; 4] = [0xC0, 0x03, 0x5F, 0xD6];

    let mut bytes: Vec<u8> = Vec::with_capacity(24);
    bytes.extend_from_slice(&stp_pre); // @0
    bytes.extend_from_slice(&adrp_x1); // @4   ← Page21
    bytes.extend_from_slice(&add_x1_pageoff); // @8   ← PageOff12
    bytes.extend_from_slice(&blr_x1); // @12  ← BLR X1
    bytes.extend_from_slice(&ldp_post); // @16
    bytes.extend_from_slice(&ret); // @20

    CompiledFunction {
        name: "_main".into(),
        bytes,
        relocs: vec![
            Reloc {
                byte_offset: 4,
                kind: RelocKind::Page21 {
                    target_sym: "__torajs_fn_1".into(),
                },
            },
            Reloc {
                byte_offset: 8,
                kind: RelocKind::PageOff12 {
                    target_sym: "__torajs_fn_1".into(),
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
                "OK: link produced {} bytes — write to /tmp/torajs_fn_addr_link and exec to verify",
                bytes.len()
            );
            let path = "/tmp/torajs_fn_addr_link";
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
