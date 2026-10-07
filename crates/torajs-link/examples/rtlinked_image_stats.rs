//! Build the prelinked runtime image and print its statistics: blob
//! size, regions, table sizes, the user-table slots, build time, and
//! whether a second build is byte-identical.
//!
//! Run: `cargo run --release -p torajs-link --example rtlinked_image_stats
//! -- [staticlib_dir staticlibs.rs]`. With no arguments it links the
//! runtime image this `torajs-core` build embeds; with both it reads the
//! archives from `staticlib_dir` in the order `staticlibs.rs` names them
//! (e.g. `target/aarch64-apple-darwin/release`).

use std::time::Instant;

use torajs_link::rtlinked_build::build_rtlinked_image;
use torajs_link::rtlinked_format::{ImportKind, RtLinkedImage};

const PREFERRED_BASE: u64 = 0x2_0000_0000;

fn image_from_dir(dir: &str, staticlibs_rs: &str) -> Vec<u8> {
    let src = std::fs::read_to_string(staticlibs_rs).expect("read staticlibs.rs");
    let archives: Vec<Vec<u8>> = src
        .lines()
        .filter_map(|l| {
            let l = l.trim().trim_end_matches(',').trim_matches('"');
            let mid = l.strip_prefix("TORAJS_")?.strip_suffix("_STATICLIB_PATH")?;
            Some(format!("{dir}/libtorajs_{}.a", mid.to_ascii_lowercase()))
        })
        .map(|p| std::fs::read(&p).unwrap_or_else(|e| panic!("read {p}: {e}")))
        .collect();
    assert!(
        !archives.is_empty(),
        "no staticlib names in {staticlibs_rs}"
    );
    let borrowed: Vec<&[u8]> = archives.iter().map(Vec::as_slice).collect();
    torajs_rtimage::image::build(&borrowed).expect("build runtime image")
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let runtime = match args.as_slice() {
        [dir, rs] => image_from_dir(dir, rs),
        [] => torajs_core::staticlibs::TORAJS_RUNTIME_IMAGE.to_vec(),
        _ => panic!("usage: rtlinked_image_stats [staticlib_dir staticlibs.rs]"),
    };
    println!("runtime image (unlinked): {} bytes", runtime.len());
    let t = Instant::now();
    let blob = build_rtlinked_image(&runtime, PREFERRED_BASE).expect("build rtlinked image");
    let ms = t.elapsed().as_secs_f64() * 1e3;
    let again = build_rtlinked_image(&runtime, PREFERRED_BASE).expect("second build");
    println!(
        "build: {ms:.1} ms; second build identical: {}",
        blob == again
    );

    let img = RtLinkedImage::parse(&blob).expect("parse");
    let h = img.header;
    println!("blob: {} bytes", blob.len());
    println!("vm span: {} bytes", h.vm_size);
    println!(
        "text: vm_off {:#x} size {} (__TEXT consts {} at {:#x})",
        h.text.vm_off, h.text.file_size, h.text_const.vm_size, h.text_const.vm_off
    );
    println!(
        "data: vm_off {:#x} template {} zero-fill {}",
        h.data.vm_off,
        h.data.file_size,
        h.data.vm_size - h.data.file_size
    );
    println!(
        "tlv: {} descriptors, block {} bytes ({} initialised)",
        h.tlv.count, h.tlv_template.vm_size, h.tlv_template.file_size
    );
    println!("rebase: {}", h.rebase.count);
    println!(
        "exports: {} (names {} bytes)",
        h.exports.count, h.names.count
    );
    let imports = img.imports().expect("imports");
    for kind in [
        ImportKind::LibSystem,
        ImportKind::LazyDylib,
        ImportKind::UserTable,
    ] {
        let n = imports.iter().filter(|i| i.kind == kind).count();
        println!("imports {kind:?}: {n}");
    }
    for i in imports.iter().filter(|i| i.kind == ImportKind::UserTable) {
        println!("  {:<32} slot {:#x}", i.name, i.slot_off);
    }
}
