//! Read-only census of the whole runtime laid out as one image: no
//! dead-strip, root set = every global defined by a `torajs*` member.
//! Reports the names the runtime expects the program side to define,
//! the reloc kinds that reach them, TLV descriptors, dyld imports, and
//! the image's text / const / data / bss sizes plus its rebase count.
//!
//! Run: `cargo run --release -p torajs-link --example probe_rtlinked_census
//! -- [staticlib_dir] [staticlibs.rs]` (defaults:
//! `target/aarch64-apple-darwin/release`, `crates/torajs-core/src/staticlibs.rs`).

use std::collections::{BTreeMap, BTreeSet};

use torajs_codegen::CompiledFunction;
use torajs_codegen::frame::FrameLayout;
use torajs_codegen::reloc::{CallTarget, Reloc, RelocKind};
use torajs_link::archive::ArMember;
use torajs_link::archive_link::{ArchiveLayout, compute_archive_layout_with_merged};
use torajs_link::archives_merge::{
    ArchiveLinkError, MergedArchives, compute_required_members, merge_archive_indexes,
};
use torajs_link::dyld_syms::dyld_lib_for;
use torajs_link::exec::LinkConfig;
use torajs_link::exec_user_entries::UserDataGlobalEntry;
use torajs_link::member_data_apply::parse_member_section_relocs;
use torajs_link::member_reloc::{MemberRelocEntry, parse_member_text_relocs};
use torajs_link::member_sections::is_tlv_descriptor;
use torajs_link::member_symtab::{parse_member_symtab, read_nlist_name};
use torajs_link::resolve::SymTable;

const RELOC_NAMES: [&str; 11] = [
    "UNSIGNED",
    "SUBTRACTOR",
    "BRANCH26",
    "PAGE21",
    "PAGEOFF12",
    "GOT_LOAD_PAGE21",
    "GOT_LOAD_PAGEOFF12",
    "POINTER_TO_GOT",
    "TLVP_LOAD_PAGE21",
    "TLVP_LOAD_PAGEOFF12",
    "ADDEND",
];

/// `(region, target class, reloc type) → count`
type Tally = BTreeMap<(&'static str, &'static str, &'static str), usize>;

/// The archive order `tr` embeds, read from the `runtime_image!` call.
fn archive_order(staticlibs_rs: &str) -> Vec<String> {
    staticlibs_rs
        .lines()
        .filter_map(|l| {
            let l = l.trim().trim_end_matches(',').trim_matches('"');
            let mid = l.strip_prefix("TORAJS_")?.strip_suffix("_STATICLIB_PATH")?;
            Some(format!("libtorajs_{}.a", mid.to_ascii_lowercase()))
        })
        .collect()
}

fn load_image(dir: &str, staticlibs_rs: &str) -> Vec<u8> {
    let src = std::fs::read_to_string(staticlibs_rs).expect("read staticlibs.rs");
    let names = archive_order(&src);
    assert!(
        !names.is_empty(),
        "no staticlib names found in {staticlibs_rs}"
    );
    let archives: Vec<Vec<u8>> = names
        .iter()
        .map(|n| std::fs::read(format!("{dir}/{n}")).unwrap_or_else(|e| panic!("read {n}: {e}")))
        .collect();
    println!("archives: {}", archives.len());
    let borrowed: Vec<&[u8]> = archives.iter().map(Vec::as_slice).collect();
    torajs_rtimage::image::build(&borrowed).expect("build image")
}

/// One `_main` whose relocs name every root, so the stock closure walk
/// pulls the whole runtime.
fn root_fn(roots: &[&str]) -> CompiledFunction {
    let relocs = roots
        .iter()
        .map(|r| Reloc {
            byte_offset: 0,
            kind: RelocKind::CallSite {
                target: CallTarget::Extern((*r).to_string()),
            },
        })
        .collect();
    CompiledFunction {
        name: "_main".into(),
        bytes: vec![0xC0, 0x03, 0x5F, 0xD6],
        relocs,
        frame: FrameLayout::leaf_no_spill(),
    }
}

/// Program-side names stand in as 8-byte data globals so the layout
/// closure treats them as defined.
fn link_cfg(image: Vec<u8>, main: CompiledFunction, user: &BTreeSet<String>) -> LinkConfig {
    let data_globals = user
        .iter()
        .map(|s| UserDataGlobalEntry {
            sym: s.clone(),
            size: 8,
            align_log2: 3,
        })
        .collect();
    LinkConfig {
        user_loops: Default::default(),
        funcs: vec![main],
        entry: "_main".into(),
        sym_table: SymTable::new(),
        codesign_ident: "census".into(),
        dead_strip: false,
        strip_member_symbols: true,
        elidable_sites: Vec::new(),
        guarded_stubs: Vec::new(),
        image: image.into(),
        strings: Vec::new(),
        data_globals,
        vtable_globals: Vec::new(),
        class_layouts: Vec::new(),
        force_emit_class_layouts_globals: false,
        fn_name_globals: Vec::new(),
        force_emit_fn_name_globals: false,
        class_names: Vec::new(),
        force_emit_class_names_globals: false,
        baked_regex_entries: Vec::new(),
    }
}

fn roots<'a>(merged: &MergedArchives<'a>) -> Vec<&'a str> {
    merged
        .symbols()
        .filter(|(_, s)| {
            merged
                .member(s.archive_idx, s.member_idx)
                .name
                .starts_with("torajs")
        })
        .map(|(n, _)| n)
        .collect()
}

fn sect_name(b: &[u8; 16]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(16);
    String::from_utf8_lossy(&b[..end]).into_owned()
}

fn classify(name: Option<&str>, user: &BTreeSet<String>) -> &'static str {
    match name {
        None => "local",
        Some(n) if user.contains(n) => "user",
        Some(n) if dyld_lib_for(n).is_some() => "import",
        Some(_) => "internal",
    }
}

/// Tally one member section's relocs; `user_hits` collects the
/// program-side names with the reloc kind that reaches them.
fn tally_relocs(
    member: &ArMember<'_>,
    region: &'static str,
    relocs: &[MemberRelocEntry],
    user: &BTreeSet<String>,
    tally: &mut Tally,
    user_hits: &mut BTreeMap<(String, &'static str, &'static str), usize>,
) {
    let symtab = parse_member_symtab(member).expect("member symtab");
    for r in relocs {
        let name = (r.r_extern == 1)
            .then(|| read_nlist_name(member, &symtab, r.r_symbolnum).expect("nlist name"));
        let class = classify(name, user);
        let ty = RELOC_NAMES.get(r.r_type as usize).copied().unwrap_or("?");
        *tally.entry((region, class, ty)).or_default() += 1;
        if class == "user" {
            let key = (name.unwrap().to_string(), region, ty);
            *user_hits.entry(key).or_default() += 1;
        }
    }
}

fn census_relocs(layout: &ArchiveLayout, merged: &MergedArchives<'_>, user: &BTreeSet<String>) {
    let mut tally = Tally::new();
    let mut user_hits = BTreeMap::new();
    let mut rebases = 0usize;
    for (mi, m) in layout.member_layouts.iter().enumerate() {
        let member = &merged.member(m.key.0, m.key.1);
        let text = parse_member_text_relocs(member).expect("text relocs");
        tally_relocs(member, "text", &text, user, &mut tally, &mut user_hits);
        for s in &m.non_text_sections {
            let rs = parse_member_section_relocs(member, s.section_index).expect("relocs");
            tally_relocs(member, "const", &rs, user, &mut tally, &mut user_hits);
        }
        for s in &layout.data_non_text_layouts[mi] {
            let rs = parse_member_section_relocs(member, s.section_index).expect("relocs");
            let region = if is_tlv_descriptor(s.flags) {
                "tlv"
            } else {
                "data"
            };
            if region == "data" && s.has_file_storage {
                rebases += rs.iter().filter(|r| r.r_type == 0).count();
            }
            tally_relocs(member, region, &rs, user, &mut tally, &mut user_hits);
        }
    }
    println!("\nrelocs reaching program-side names (name / region / kind / count):");
    for ((n, region, ty), c) in &user_hits {
        println!("  {n:<36} {region:<6} {ty:<20} {c}");
    }
    println!("\nall member relocs (region / target / kind / count):");
    for ((region, class, ty), c) in &tally {
        println!("  {region:<6} {class:<9} {ty:<20} {c}");
    }
    println!("\nrebase (UNSIGNED in file-backed __DATA*, non-TLV): {rebases}");
}

fn census_sizes(layout: &ArchiveLayout) {
    let text: u64 = layout
        .member_layouts
        .iter()
        .map(|m| u64::from(m.text_size))
        .sum();
    let first = layout.member_layouts.first().map_or(0, |m| m.vaddr);
    let last = layout
        .member_layouts
        .last()
        .map_or(0, |m| m.vaddr + u64::from(m.text_size));
    let mut tconst: BTreeMap<String, u64> = BTreeMap::new();
    for s in layout
        .member_layouts
        .iter()
        .flat_map(|m| &m.non_text_sections)
    {
        let k = format!("{},{}", sect_name(&s.segname), sect_name(&s.sectname));
        *tconst.entry(k).or_default() += u64::from(s.size);
    }
    println!("\nimage sizes (members only, bytes):");
    println!("  text   sum={text} span={}", last - first);
    println!("  const  region={}", layout.non_text_region_size);
    for (k, v) in &tconst {
        println!("         {k:<28} {v}");
    }
    println!("  data   file={}", layout.data_non_text_file_size);
    println!("  bss    zerofill={}", layout.data_non_text_zerofill_vmsize);
    for s in &layout.data_merged_sections {
        let k = format!("{},{}", sect_name(&s.segname), sect_name(&s.sectname));
        println!("         {k:<28} {} file={}", s.size, s.has_file_storage);
    }
    println!("  stubs  {}", layout.stubs_section_size);
}

fn census_imports(layout: &ArchiveLayout) {
    let libsystem = layout.dyld_imports.values().filter(|&&o| o == 1).count();
    let libcurl = layout.dyld_imports.values().filter(|&&o| o == 2).count();
    println!("\nTLV descriptors: {}", layout.tlv_descriptors.len());
    println!("dyld imports: libSystem={libsystem} libcurl={libcurl}");
}

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args
        .next()
        .unwrap_or_else(|| "target/aarch64-apple-darwin/release".into());
    let rs = args
        .next()
        .unwrap_or_else(|| "crates/torajs-core/src/staticlibs.rs".into());
    let image = load_image(&dir, &rs);
    let merged = merge_archive_indexes(&image).expect("merge");
    let root_names = roots(&merged);
    let main_fn = root_fn(&root_names);
    let user: BTreeSet<String> = match compute_required_members(
        std::slice::from_ref(&main_fn),
        &merged,
        &Default::default(),
    ) {
        Ok(_) => BTreeSet::new(),
        Err(ArchiveLinkError::UnresolvedExterns { names }) => names.into_iter().collect(),
        Err(e) => panic!("closure: {e:?}"),
    };
    println!("roots: {}", root_names.len());
    println!(
        "\nnames the runtime closure needs from the program side: {}",
        user.len()
    );
    for n in &user {
        println!("  {n}");
    }
    let cfg = link_cfg(image.clone(), main_fn, &user);
    let layout =
        compute_archive_layout_with_merged(&cfg, &merged, &Default::default()).expect("layout");
    println!("members: {}", layout.member_layouts.len());
    census_relocs(&layout, &merged, &user);
    census_imports(&layout);
    census_sizes(&layout);
}
