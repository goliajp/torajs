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
    // r_extern=0 breakdown: r_type → count
    let mut r_ext0_by_type: BTreeMap<u8, u64> = BTreeMap::new();
    // r_extern=0 by (r_type, r_length) — what addend slot size do
    // we need to support?
    let mut r_ext0_by_type_length: BTreeMap<(u8, u8), u64> = BTreeMap::new();
    // r_extern=0 r_symbolnum distribution (section ordinal hits).
    let mut r_ext0_section_idx_counts: BTreeMap<u32, u64> = BTreeMap::new();
    // Sample dumps of first N r_extern=0 relocs — includes the raw
    // instruction word at r_address so we can audit addend
    // convention against the Mach-O spec.
    let mut sample_dumps: Vec<String> = Vec::new();
    const SAMPLE_LIMIT: usize = 16;

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
            // Pull the member's __TEXT,__text payload so we can
            // print the instruction word at each r_extern=0 site —
            // required input for the addend-convention audit.
            let text_slice = match torajs_link::archive_link::parse_member_text_section(m) {
                Ok((text_off, text_size)) => {
                    let off = text_off as usize;
                    let end = off + text_size as usize;
                    Some(&m.data[off..end])
                }
                Err(_) => None,
            };
            for r in &relocs {
                if r.r_extern == 1 {
                    e1 += 1;
                } else {
                    e0 += 1;
                    *r_ext0_by_type.entry(r.r_type).or_insert(0) += 1;
                    *r_ext0_by_type_length
                        .entry((r.r_type, r.r_length))
                        .or_insert(0) += 1;
                    *r_ext0_section_idx_counts.entry(r.r_symbolnum).or_insert(0) += 1;
                    if sample_dumps.len() < SAMPLE_LIMIT {
                        let insn_word = text_slice.and_then(|ts| {
                            let off = r.r_address as usize;
                            if off + 4 <= ts.len() {
                                Some(u32::from_le_bytes(ts[off..off + 4].try_into().unwrap()))
                            } else {
                                None
                            }
                        });
                        let slot8 = text_slice.and_then(|ts| {
                            let off = r.r_address as usize;
                            if off + 8 <= ts.len() {
                                Some(u64::from_le_bytes(ts[off..off + 8].try_into().unwrap()))
                            } else {
                                None
                            }
                        });
                        sample_dumps.push(format!(
                            "  {name}::{mname} | r_addr=0x{ra:04x} r_type={rt} r_len={rl} r_pcrel={rp} sec_idx={si} insn=0x{iw:08x} slot8=0x{s8:016x}",
                            mname = m.name,
                            ra = r.r_address,
                            rt = r.r_type,
                            rl = r.r_length,
                            rp = r.r_pcrel,
                            si = r.r_symbolnum,
                            iw = insn_word.unwrap_or(0),
                            s8 = slot8.unwrap_or(0),
                        ));
                    }
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

    println!();
    println!("r_extern=0 (section-keyed) breakdown:");
    for (t, c) in &r_ext0_by_type {
        let kind = match *t {
            0 => "UNSIGNED",
            2 => "BRANCH26",
            3 => "PAGE21",
            4 => "PAGEOFF12",
            _ => "?",
        };
        println!("  r_type={t} ({kind:9}): {c}");
    }
    println!();
    println!("r_extern=0 (r_type, r_length) → count:");
    for ((t, l), c) in &r_ext0_by_type_length {
        println!("  (r_type={t}, r_length={l}): {c}");
    }
    println!();
    println!(
        "r_extern=0 section ordinal histogram (top 8): {} distinct",
        r_ext0_section_idx_counts.len()
    );
    let mut idx_vec: Vec<(u32, u64)> = r_ext0_section_idx_counts
        .iter()
        .map(|(k, v)| (*k, *v))
        .collect();
    idx_vec.sort_by(|a, b| b.1.cmp(&a.1));
    for (idx, c) in idx_vec.iter().take(8) {
        println!("  sec_ordinal={idx}: {c}");
    }
    println!();
    println!("r_extern=0 sample dumps (first {SAMPLE_LIMIT}):");
    for s in &sample_dumps {
        println!("{s}");
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
