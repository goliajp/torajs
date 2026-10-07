//! Prelinked runtime image: export lookup, rebase, imports and GOT
//! slots on the runtime this build embeds, plus synthetic archives for
//! the GOT encoding and the two refusal paths.

use std::sync::OnceLock;

use torajs_codegen::CompiledFunction;
use torajs_codegen::frame::FrameLayout;
use torajs_codegen::reloc::{CallTarget, Reloc, RelocKind};
use torajs_link::archive_link::ArchiveLayoutError;
use torajs_link::archives_merge::ArchiveLinkError;
use torajs_link::member_apply::MemberRelocApplyError;
use torajs_link::rtlinked_build::{RtLinkedBuildError, USER_TABLE_NAMES, build_rtlinked_image};
use torajs_link::rtlinked_format::{ImportKind, RtLinkedImage};
use torajs_obj::{
    ARM64_RELOC_GOT_LOAD_PAGE21, ARM64_RELOC_GOT_LOAD_PAGEOFF12, ARM64_RELOC_PAGE21,
    ARM64_RELOC_PAGEOFF12,
};

const BASE: u64 = 0x2_0000_0000;
const ADRP_X0: u32 = 0x9000_0000;
const LDR_X0_X0: u32 = 0xF940_0000;
const ADD_X0_X0: u32 = 0x9100_0000;
const BL: u32 = 0x9400_0000;
const RET: u32 = 0xD65F_03C0;

fn runtime_blob() -> &'static [u8] {
    static BLOB: OnceLock<Vec<u8>> = OnceLock::new();
    BLOB.get_or_init(|| {
        build_rtlinked_image(torajs_core::staticlibs::TORAJS_RUNTIME_IMAGE, BASE)
            .expect("build the embedded runtime")
    })
}

fn u32_at(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(b[off..off + 4].try_into().unwrap())
}

fn u64_at(b: &[u8], off: usize) -> u64 {
    u64::from_le_bytes(b[off..off + 8].try_into().unwrap())
}

fn sign_extend(v: u64, bits: u32) -> i64 {
    ((v << (64 - bits)) as i64) >> (64 - bits)
}

/// Page an `ADRP` at `pc` names.
fn adrp_page(insn: u32, pc: u64) -> u64 {
    let imm = u64::from((insn >> 29) & 3) | (u64::from((insn >> 5) & 0x7_FFFF) << 2);
    ((pc & !0xFFF) as i64 + (sign_extend(imm, 21) << 12)) as u64
}

/// Every address an `ADRP xN` followed within four words by an `LDR` or
/// `ADD` using xN as base computes: `(is_ldr, address)`.
fn adrp_pairs(text: &[u8], text_base: u64) -> Vec<(bool, u64)> {
    let mut out = Vec::new();
    for i in (0..text.len()).step_by(4) {
        let adrp = u32_at(text, i);
        if adrp & 0x9F00_0000 != 0x9000_0000 {
            continue;
        }
        let page = adrp_page(adrp, text_base + i as u64);
        for j in (i + 4..(i + 20).min(text.len())).step_by(4) {
            let insn = u32_at(text, j);
            if (insn >> 5) & 0x1F != adrp & 0x1F {
                continue;
            }
            let imm12 = u64::from((insn >> 10) & 0xFFF);
            if insn & 0xFFC0_0000 == LDR_X0_X0 {
                out.push((true, page + imm12 * 8));
            } else if insn & 0xFFC0_0000 == ADD_X0_X0 {
                out.push((false, page + imm12));
            }
        }
    }
    out
}

#[test]
fn export_binary_search_finds_every_entry() {
    let img = RtLinkedImage::parse(runtime_blob()).unwrap();
    let n = img.header.exports.count as usize;
    assert!(n > 1000, "exports: {n}");
    let mut prev: Option<&str> = None;
    for i in 0..n {
        let (name, off) = img.export_at(i).unwrap();
        assert!(
            prev.is_none_or(|p| p.as_bytes() < name.as_bytes()),
            "unsorted at {name}"
        );
        assert_eq!(img.export(name).unwrap(), Some(off), "{name}");
        assert!(off < img.header.vm_size, "{name} at {off:#x}");
        prev = Some(name);
    }
    assert_eq!(img.export("_no_such_symbol_anywhere").unwrap(), None);
    assert_eq!(img.export("_main").unwrap(), None);
    for name in USER_TABLE_NAMES {
        assert_eq!(img.export(name).unwrap(), None, "{name} must be an import");
    }
}

#[test]
fn rebase_slots_hold_absolute_pointers_into_the_image() {
    let img = RtLinkedImage::parse(runtime_blob()).unwrap();
    let h = img.header;
    let data = img.data_template();
    let rebases: Vec<u32> = img.rebases().collect();
    assert!(!rebases.is_empty());
    assert!(
        rebases.windows(2).all(|w| w[0] < w[1]),
        "sorted and distinct"
    );
    for off in rebases {
        assert!(off >= h.data.vm_off && off + 8 <= h.data.vm_off + h.data.file_size);
        let v = u64_at(data, (off - h.data.vm_off) as usize);
        assert!(
            v >= BASE && v - BASE < u64::from(h.vm_size),
            "{off:#x} -> {v:#x}"
        );
    }
}

#[test]
fn imports_cover_libsystem_and_user_tables_with_distinct_slots() {
    let img = RtLinkedImage::parse(runtime_blob()).unwrap();
    let h = img.header;
    let imports = img.imports().unwrap();
    let mut slots: Vec<u32> = imports.iter().map(|i| i.slot_off).collect();
    slots.sort_unstable();
    slots.dedup();
    assert_eq!(slots.len(), imports.len(), "slots are distinct");
    for i in &imports {
        assert_eq!(i.slot_off % 8, 0, "{}", i.name);
        assert!(i.slot_off >= h.data.vm_off && i.slot_off + 8 <= h.data.vm_off + h.data.vm_size);
    }
    let users: Vec<&str> = imports
        .iter()
        .filter(|i| i.kind == ImportKind::UserTable)
        .map(|i| i.name)
        .collect();
    let mut want = USER_TABLE_NAMES.to_vec();
    want.sort_unstable();
    assert_eq!(users, want);
    assert!(
        imports
            .iter()
            .any(|i| i.kind == ImportKind::LibSystem && i.name == "_write")
    );
}

#[test]
fn user_tables_are_read_through_their_got_slots() {
    let img = RtLinkedImage::parse(runtime_blob()).unwrap();
    let pairs = adrp_pairs(img.text(), BASE + u64::from(img.header.text.vm_off));
    for i in img.imports().unwrap() {
        if i.kind != ImportKind::UserTable {
            continue;
        }
        let slot = BASE + u64::from(i.slot_off);
        assert!(pairs.contains(&(true, slot)), "no LDR through {}", i.name);
        assert!(
            !pairs.contains(&(false, slot)),
            "{} slot used as data",
            i.name
        );
    }
}

#[test]
fn runtime_build_is_deterministic() {
    let again = build_rtlinked_image(torajs_core::staticlibs::TORAJS_RUNTIME_IMAGE, BASE).unwrap();
    assert!(again == runtime_blob(), "two builds differ");
}

/// A one-member `torajs_t.o` archive turned into a runtime image.
fn synthetic_runtime(entry: CompiledFunction, got: bool) -> Vec<u8> {
    let mut obj = torajs_obj::write_object(&[entry]);
    if got {
        retype_page_relocs_to_got(&mut obj);
    }
    let mut ar = b"!<arch>\n".to_vec();
    let mut header = [b' '; 60];
    header[..10].copy_from_slice(b"torajs_t.o");
    header[16] = b'0';
    let size = obj.len().to_string();
    header[48..48 + size.len()].copy_from_slice(size.as_bytes());
    header[58..60].copy_from_slice(b"`\n");
    ar.extend_from_slice(&header);
    ar.extend_from_slice(&obj);
    if obj.len() % 2 == 1 {
        ar.push(b'\n');
    }
    torajs_rtimage::image::build(&[&ar]).expect("runtime image")
}

/// codegen never emits GOT relocs; rewrite the `__text` PAGE21 /
/// PAGEOFF12 entries into their GOT_LOAD forms.
fn retype_page_relocs_to_got(obj: &mut [u8]) {
    let ncmds = u32_at(obj, 16) as usize;
    let mut lc = 32;
    for _ in 0..ncmds {
        if u32_at(obj, lc) == 0x19 {
            let sect = lc + 72;
            let (reloff, nreloc) = (u32_at(obj, sect + 56), u32_at(obj, sect + 60));
            for k in 0..nreloc as usize {
                let at = reloff as usize + k * 8 + 4;
                let info = u32_at(obj, at);
                let ty = match (info >> 28) as u8 {
                    ARM64_RELOC_PAGE21 => ARM64_RELOC_GOT_LOAD_PAGE21,
                    ARM64_RELOC_PAGEOFF12 => ARM64_RELOC_GOT_LOAD_PAGEOFF12,
                    other => other,
                };
                let info = (info & 0x0FFF_FFFF) | (u32::from(ty) << 28);
                obj[at..at + 4].copy_from_slice(&info.to_le_bytes());
            }
        }
        lc += u32_at(obj, lc + 4) as usize;
    }
}

/// `ADRP x0, table; <second>; BL callee; RET`.
fn entry(table: &str, second: u32, callee: &str) -> CompiledFunction {
    let words = [ADRP_X0, second, BL, RET];
    let sym = |s: &str| s.to_string();
    CompiledFunction {
        name: "_torajs_t_entry".into(),
        bytes: words.iter().flat_map(|w| w.to_le_bytes()).collect(),
        relocs: vec![
            Reloc {
                byte_offset: 0,
                kind: RelocKind::Page21 {
                    target_sym: sym(table),
                },
            },
            Reloc {
                byte_offset: 4,
                kind: RelocKind::PageOff12 {
                    target_sym: sym(table),
                },
            },
            Reloc {
                byte_offset: 8,
                kind: RelocKind::CallSite {
                    target: CallTarget::Extern(sym(callee)),
                },
            },
        ],
        frame: FrameLayout::leaf_no_spill(),
    }
}

#[test]
fn got_load_targets_the_slot_and_keeps_the_ldr() {
    let rt = synthetic_runtime(entry(USER_TABLE_NAMES[0], LDR_X0_X0, "_write"), true);
    let blob = build_rtlinked_image(&rt, BASE).expect("build");
    assert!(
        build_rtlinked_image(&rt, BASE).unwrap() == blob,
        "deterministic"
    );
    let img = RtLinkedImage::parse(&blob).unwrap();
    let at = img
        .export("_torajs_t_entry")
        .unwrap()
        .expect("entry exported");
    let text = img.text();
    let t = (at - img.header.text.vm_off) as usize;
    let pc = BASE + u64::from(at);
    let ldr = u32_at(text, t + 4);
    assert_eq!(ldr & 0xFFC0_0000, LDR_X0_X0, "LDR not relaxed: {ldr:#x}");
    let slot = img
        .imports()
        .unwrap()
        .into_iter()
        .find(|i| i.name == USER_TABLE_NAMES[0])
        .unwrap()
        .slot_off;
    let addr = adrp_page(u32_at(text, t), pc) + u64::from((ldr >> 10) & 0xFFF) * 8;
    assert_eq!(addr, BASE + u64::from(slot));
    let bl = u32_at(text, t + 8);
    let stub = img.export("_write").unwrap().expect("stub exported");
    let disp = sign_extend(u64::from(bl & 0x03FF_FFFF), 26) * 4;
    assert_eq!((pc + 8) as i64 + disp, (BASE + u64::from(stub)) as i64);
}

#[test]
fn direct_reference_to_a_user_table_is_refused() {
    let rt = synthetic_runtime(entry(USER_TABLE_NAMES[0], ADD_X0_X0, "_write"), false);
    match build_rtlinked_image(&rt, BASE) {
        Err(RtLinkedBuildError::Layout(ArchiveLayoutError::MemberReloc {
            err: MemberRelocApplyError::GotSlotNotGotLoad { name, .. },
            ..
        })) => assert_eq!(name, USER_TABLE_NAMES[0]),
        other => panic!("expected GotSlotNotGotLoad, got {other:?}"),
    }
}

#[test]
fn unresolved_name_outside_the_import_set_fails_the_build() {
    let rt = synthetic_runtime(
        entry(USER_TABLE_NAMES[0], LDR_X0_X0, "_torajs_nowhere"),
        true,
    );
    match build_rtlinked_image(&rt, BASE) {
        Err(RtLinkedBuildError::Layout(ArchiveLayoutError::Link(
            ArchiveLinkError::UnresolvedExterns { names },
        ))) => assert_eq!(names, ["_torajs_nowhere"]),
        other => panic!("expected UnresolvedExterns, got {other:?}"),
    }
}

#[test]
fn misaligned_base_is_refused() {
    let rt = synthetic_runtime(entry(USER_TABLE_NAMES[0], LDR_X0_X0, "_write"), true);
    assert!(matches!(
        build_rtlinked_image(&rt, BASE + 0x1000),
        Err(RtLinkedBuildError::Unsupported(_))
    ));
}
