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

    // Full production archive set. The worklist closure decides which
    // members actually get pulled in based on the user's reloc graph,
    // so listing all archives doesn't necessarily integrate them all.
    let archive_names = [
        "libtorajs_abort.a",
        "libtorajs_anyvalue.a",
        "libtorajs_arr.a",
        "libtorajs_bigint.a",
        "libtorajs_capture_box.a",
        "libtorajs_collections.a",
        "libtorajs_cycle.a",
        "libtorajs_date.a",
        "libtorajs_dynobj.a",
        "libtorajs_fetch.a",
        "libtorajs_fmt.a",
        "libtorajs_fs.a",
        "libtorajs_io.a",
        "libtorajs_math.a",
        "libtorajs_mem.a",
        "libtorajs_meta.a",
        "libtorajs_microtask.a",
        "libtorajs_mmalloc.a",
        "libtorajs_mutex.a",
        "libtorajs_num.a",
        "libtorajs_panic.a",
        "libtorajs_panic_runtime.a",
        "libtorajs_print.a",
        "libtorajs_process.a",
        "libtorajs_promise.a",
        "libtorajs_rc.a",
        "libtorajs_regex.a",
        "libtorajs_str.a",
        "libtorajs_syscall.a",
        "libtorajs_throw.a",
        "libtorajs_value_drop.a",
        "libtorajs_weak.a",
    ];
    let archive_paths: Vec<PathBuf> = archive_names.iter().map(|n| target_dir.join(n)).collect();

    let archives: Vec<std::borrow::Cow<'static, [u8]>> = archive_paths
        .iter()
        .map(|p| fs::read(p).expect("read archive").into())
        .collect();
    eprintln!("probing with {} archive(s)", archives.len());

    // Pick the user-facing sym from CLI arg (defaults to the leaf
    // syscall surface so the original smoke path still runs).
    let extern_sym = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "___torajs_syscall_abort".to_string());
    eprintln!("user fn calls extern: {extern_sym}");

    let main_cf = fn_calls_extern_then_returns("_main", &extern_sym);
    let cfg = LinkConfig {
        funcs: vec![main_cf],
        entry: "_main".into(),
        sym_table: SymTable::new(),
        codesign_ident: "tora".into(),
        dead_strip: false,
        archives,
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
