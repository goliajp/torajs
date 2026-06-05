//! Diagnostic — surveys r_extern distribution + r_type histogram
//! across every member of every `libtorajs_*.a` under
//! `$CARGO_TARGET_DIR/release/`. Use before #9 production swap to
//! confirm whether section-keyed (r_extern=0) relocs are present
//! and need the S7-C5e patcher path.
//!
//! Run: `cargo run --release -p torajs-link --example probe_section_reloc`

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use torajs_link::archive::parse_archive;
use torajs_link::member_reloc::parse_member_text_relocs;

fn main() {
    let target_dir =
        PathBuf::from(std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".into()))
            .join("release");

    let mut archives: Vec<PathBuf> = fs::read_dir(&target_dir)
        .unwrap_or_else(|e| panic!("read_dir {}: {e}", target_dir.display()))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("libtorajs_") && n.ends_with(".a"))
                .unwrap_or(false)
        })
        .collect();
    archives.sort();

    let mut grand_extern1 = 0u64;
    let mut grand_extern0 = 0u64;
    let mut per_archive: BTreeMap<String, (u64, u64, u64)> = BTreeMap::new();
    let mut r_type_counts: BTreeMap<u8, u64> = BTreeMap::new();

    for ar_path in &archives {
        let name = ar_path.file_name().unwrap().to_str().unwrap().to_string();
        let bytes = fs::read(ar_path).expect("read archive");
        let members = match parse_archive(&bytes) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("[{name}] parse_archive ERR: {e:?}");
                continue;
            }
        };
        let mut n_members = 0u64;
        let mut e0 = 0u64;
        let mut e1 = 0u64;
        let mut n_with_relocs = 0u64;
        let mut n_no_text = 0u64;
        let mut n_other_err = 0u64;
        let mut first_err_sample: Option<String> = None;
        for (mi, m) in members.iter().enumerate() {
            n_members += 1;
            let relocs = match parse_member_text_relocs(m) {
                Ok(r) => r,
                Err(e) => {
                    if matches!(
                        e,
                        torajs_link::member_reloc::MemberRelocError::NoTextSection
                    ) {
                        n_no_text += 1;
                    } else {
                        n_other_err += 1;
                        if first_err_sample.is_none() {
                            first_err_sample = Some(format!("member {mi} ({}): {e:?}", m.name));
                        }
                    }
                    continue;
                }
            };
            if !relocs.is_empty() {
                n_with_relocs += 1;
            }
            for r in &relocs {
                if r.r_extern == 1 {
                    e1 += 1;
                } else {
                    e0 += 1;
                }
                *r_type_counts.entry(r.r_type).or_insert(0) += 1;
            }
        }
        if n_other_err > 0 || n_no_text > 0 {
            eprintln!(
                "  [{name}] members={n_members} with_relocs={n_with_relocs} no_text={n_no_text} other_err={n_other_err} first_err={first_err_sample:?}"
            );
        }
        grand_extern1 += e1;
        grand_extern0 += e0;
        per_archive.insert(name.clone(), (n_members, e1, e0));
    }

    println!("=== probe: r_extern distribution across libtorajs_*.a ===");
    println!("archive                                       members  r_ext=1  r_ext=0");
    for (name, (n, e1, e0)) in &per_archive {
        println!("  {name:42}  {n:>6}  {e1:>7}  {e0:>7}");
    }
    println!("---");
    println!(
        "GRAND TOTAL                                            {grand_extern1:>7}  {grand_extern0:>7}"
    );
    println!();
    println!("r_type distribution (all members, all archives):");
    for (t, c) in &r_type_counts {
        let kind = match *t {
            0 => "UNSIGNED",
            2 => "BRANCH26",
            3 => "PAGE21",
            4 => "PAGEOFF12",
            _ => "?",
        };
        println!("  r_type={t} ({kind:9}): {c}");
    }

    if grand_extern0 == 0 {
        println!();
        println!(
            "VERDICT (A): all relocs are extern-keyed — current S7-C5 patcher is sufficient for #9 swap on this archive set"
        );
    } else {
        println!();
        println!(
            "VERDICT (B): {grand_extern0} section-keyed reloc(s) present — S7-C5e (section-table walk + addend) needed before #9 swap"
        );
    }
}
