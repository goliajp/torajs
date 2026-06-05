//! Real-archive link probe — hand-builds a `_main` that BL's
//! `___torajs_syscall_abort` (lives in `libtorajs_syscall.a`), feeds
//! it through `link_to_exec_with_archives` against the full
//! production `libtorajs_*.a` set, and reports what
//! `ArchiveLayoutError` (if any) blocks #9 swap.
//!
//! Run: `cargo run --release -p torajs-link --example probe_real_link`

use std::fs;
use std::path::PathBuf;

use torajs_codegen::CompiledFunction;
use torajs_codegen::frame::FrameLayout;
use torajs_codegen::reloc::{CallTarget, Reloc, RelocKind};

use torajs_link::archive_emit::link_to_exec_with_archives;
use torajs_link::exec::LinkConfig;
use torajs_link::resolve::SymTable;

fn fn_calls_extern_then_returns(name: &str, extern_name: &str) -> CompiledFunction {
    let stp_pre: [u8; 4] = [0xFD, 0x7B, 0xBF, 0xA9];
    let bl_placeholder: [u8; 4] = [0x00, 0x00, 0x00, 0x94];
    let ldp_post: [u8; 4] = [0xFD, 0x7B, 0xC1, 0xA8];
    let ret: [u8; 4] = [0xC0, 0x03, 0x5F, 0xD6];
    let mut bytes = Vec::with_capacity(16);
    bytes.extend_from_slice(&stp_pre);
    bytes.extend_from_slice(&bl_placeholder);
    bytes.extend_from_slice(&ldp_post);
    bytes.extend_from_slice(&ret);
    CompiledFunction {
        name: name.into(),
        bytes,
        relocs: vec![Reloc {
            byte_offset: 4,
            kind: RelocKind::CallSite {
                target: CallTarget::Extern(extern_name.into()),
            },
        }],
        frame: FrameLayout::leaf_no_spill(),
    }
}

fn main() {
    let target_dir =
        PathBuf::from(std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".into()))
            .join("release");

    // Just one archive — the syscall surface. Real production link
    // would fan out to the entire libtorajs_*.a set, but we start
    // narrow to surface the smallest possible failure.
    let archive_paths = vec![target_dir.join("libtorajs_syscall.a")];

    let archives: Vec<Vec<u8>> = archive_paths
        .iter()
        .map(|p| fs::read(p).expect("read archive"))
        .collect();
    eprintln!("probing with {} archive(s)", archives.len());

    let main_cf = fn_calls_extern_then_returns("_main", "___torajs_syscall_abort");
    let cfg = LinkConfig {
        funcs: vec![main_cf],
        entry: "_main".into(),
        sym_table: SymTable::new(),
        codesign_ident: "tora".into(),
        archives,
    };

    // Pre-link: query compute_archive_layout directly so we can see
    // how many members got pulled in by the worklist closure.
    {
        match torajs_link::archive_link::compute_archive_layout(&cfg) {
            Ok(layout) => {
                eprintln!("worklist pulled {} member(s):", layout.member_layouts.len());
                for m in &layout.member_layouts {
                    let merged = torajs_link::archives_merge::merge_archive_indexes(&cfg.archives)
                        .expect("merge");
                    let member = &merged.per_archive_members[m.key.0][m.key.1];
                    eprintln!(
                        "  archive={} member_idx={} member_name={} text_size={}",
                        m.key.0, m.key.1, member.name, m.text_size
                    );
                }
            }
            Err(e) => {
                eprintln!("layout ERR: {e:?}");
                return;
            }
        }
    }

    match link_to_exec_with_archives(&cfg) {
        Ok(bytes) => {
            println!(
                "OK: link produced {} bytes — write to /tmp/torajs_real_link and exec to verify",
                bytes.len()
            );
            let path = "/tmp/torajs_real_link";
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
