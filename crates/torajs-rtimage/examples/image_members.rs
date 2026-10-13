//! Which archive members the runtime image keeps and which it leaves
//! out, read back from the image rather than from the builder.
//!
//!   cargo run -p torajs-rtimage --example image_members -- <lib.a>... [--kept]
//!
//! Prints one row per distinct `(name, bytes)` member that is in some
//! archive but not in the image (`--kept`: also the kept ones), then
//! the totals.

use std::collections::{BTreeMap, BTreeSet};

use torajs_rtimage::archive::parse_archive;
use torajs_rtimage::image::{Image, build_reachable};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let show_kept = args.iter().any(|a| a == "--kept");
    let paths: Vec<&String> = args.iter().filter(|a| *a != "--kept").collect();
    let files: Vec<Vec<u8>> = paths
        .iter()
        .map(|p| std::fs::read(p).unwrap_or_else(|e| panic!("read {p}: {e}")))
        .collect();
    let borrowed: Vec<&[u8]> = files.iter().map(Vec::as_slice).collect();
    let image = build_reachable(&borrowed).expect("build");
    let view = Image::parse(&image).expect("parse");

    let mut in_image: BTreeSet<(&str, &[u8])> = BTreeSet::new();
    for a in 0..view.archive_count() {
        for m in view.members(a) {
            in_image.insert((m.name, m.data));
        }
    }
    // distinct member -> the archives carrying it
    let mut all: BTreeMap<(&str, &[u8]), Vec<usize>> = BTreeMap::new();
    for (a, bytes) in borrowed.iter().enumerate() {
        for m in parse_archive(bytes).expect("archive") {
            all.entry((m.name, m.data)).or_default().push(a);
        }
    }
    let (mut kept, mut kept_bytes, mut dropped, mut dropped_bytes) = (0, 0, 0, 0);
    for ((name, data), archives) in &all {
        let keep = in_image.contains(&(*name, *data));
        if keep {
            kept += 1;
            kept_bytes += data.len();
        } else {
            dropped += 1;
            dropped_bytes += data.len();
        }
        if !keep || show_kept {
            let tag = if keep { "kept" } else { "dropped" };
            println!("{tag}\t{}\t{}\t{name}", data.len(), archives.len());
        }
    }
    println!(
        "image {} bytes; distinct members kept {kept} ({kept_bytes} bytes), dropped {dropped} ({dropped_bytes} bytes)",
        image.len()
    );
}
