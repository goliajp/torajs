//! The pre-stripped runtime image: on the runtime this build embeds,
//! every name a program can reference still resolves to the member it
//! did; on synthetic archives, what it may cut in a member and what it
//! must leave whole.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use torajs_codegen::CompiledFunction;
use torajs_codegen::frame::FrameLayout;
use torajs_link::runtime_prestrip::{prestrip_image, prestripped_runtime_image};
use torajs_obj::MH_SUBSECTIONS_VIA_SYMBOLS;
use torajs_rtimage::archive::{ArMember, parse_member_defined_externs};
use torajs_rtimage::image::Image;
use torajs_rtimage::prune::nameable_by_program;

fn original() -> &'static [u8] {
    torajs_core::staticlibs::TORAJS_RUNTIME_IMAGE
}

fn stripped() -> &'static [u8] {
    static STRIPPED: OnceLock<Vec<u8>> = OnceLock::new();
    STRIPPED.get_or_init(|| prestrip_image(original()).expect("prestrip the embedded runtime"))
}

/// `(archive, member name)` of every member of `img`, by index.
fn member_names<'a>(img: &Image<'a>) -> Vec<Vec<&'a str>> {
    (0..img.archive_count())
        .map(|a| img.members(a).map(|m| m.name).collect())
        .collect()
}

/// A program's link looks each name it references up in the merged
/// index and pulls the member that wins it: the strip must leave every
/// such name in the index, won by the same member.
#[test]
fn every_name_a_program_can_reference_resolves_to_the_same_member() {
    let (before, after) = (
        Image::parse(original()).unwrap(),
        Image::parse(stripped()).unwrap(),
    );
    let (names_before, names_after) = (member_names(&before), member_names(&after));
    let index_after: BTreeMap<&str, (usize, &str)> = after
        .symbols()
        .map(|s| {
            (
                s.name,
                (s.archive_idx, names_after[s.archive_idx][s.member_idx]),
            )
        })
        .collect();
    let mut checked = 0;
    for s in before.symbols() {
        let member = names_before[s.archive_idx][s.member_idx];
        if !nameable_by_program(s.name, member) {
            continue;
        }
        assert_eq!(
            index_after.get(s.name),
            Some(&(s.archive_idx, member)),
            "{}",
            s.name
        );
        checked += 1;
    }
    assert!(checked > 1000, "only {checked} names checked");
    eprintln!("{checked} names checked");
}

/// A member that does not declare `MH_SUBSECTIONS_VIA_SYMBOLS` (the
/// compiler-builtins assembly objects some archives carry) reaches the
/// stripped image unchanged.
#[test]
fn every_member_without_subsections_is_kept_as_it_is() {
    let (before, after) = (
        Image::parse(original()).unwrap(),
        Image::parse(stripped()).unwrap(),
    );
    let mut checked = 0;
    for a in 0..before.archive_count() {
        let kept: BTreeMap<&str, &[u8]> = after.members(a).map(|m| (m.name, m.data)).collect();
        for m in before.members(a) {
            let flags = u32::from_le_bytes(m.data[24..28].try_into().unwrap());
            if flags & MH_SUBSECTIONS_VIA_SYMBOLS == 0
                && let Some(data) = kept.get(m.name)
            {
                assert!(*data == m.data, "{} changed", m.name);
                checked += 1;
            }
        }
    }
    eprintln!("{checked} members without subsections checked");
}

/// A Rust-mangled global outside a torajs member: no program names it.
const TAIL: &str = "__ZN4test4tail17h0123456789abcdefE";

fn leaf(name: &str) -> CompiledFunction {
    CompiledFunction {
        name: name.into(),
        bytes: vec![0xC0, 0x03, 0x5F, 0xD6],
        relocs: Vec::new(),
        frame: FrameLayout::leaf_no_spill(),
    }
}

/// `_head` (a root) followed in the same `__text` by [`TAIL`] (not
/// one), with or without `MH_SUBSECTIONS_VIA_SYMBOLS`.
fn head_and_tail(subsections: bool) -> Vec<u8> {
    let mut obj = torajs_obj::write_object(&[leaf("_head"), leaf(TAIL)]);
    let flags = u32::from_le_bytes(obj[24..28].try_into().unwrap());
    let flags = if subsections {
        flags | MH_SUBSECTIONS_VIA_SYMBOLS
    } else {
        flags & !MH_SUBSECTIONS_VIA_SYMBOLS
    };
    obj[24..28].copy_from_slice(&flags.to_le_bytes());
    obj
}

fn archive(obj: &[u8]) -> Vec<u8> {
    let mut ar = b"!<arch>\n".to_vec();
    let mut header = [b' '; 60];
    header[..3].copy_from_slice(b"t.o");
    header[16] = b'0';
    let size = obj.len().to_string();
    header[48..48 + size.len()].copy_from_slice(size.as_bytes());
    header[58..60].copy_from_slice(b"`\n");
    ar.extend_from_slice(&header);
    ar.extend_from_slice(obj);
    if obj.len() % 2 == 1 {
        ar.push(b'\n');
    }
    ar
}

/// The bytes the pre-stripped image holds for the one member.
fn prestripped_member(obj: &[u8]) -> Vec<u8> {
    let img = prestripped_runtime_image(&[&archive(obj)]).expect("prestrip");
    let view = Image::parse(&img).expect("parse");
    let members: Vec<_> = view.members(0).collect();
    assert_eq!(members.len(), 1);
    members[0].data.to_vec()
}

/// Without the flag `_head` may fall through into the tail, so the
/// member is kept as it is.
#[test]
fn an_object_without_subsections_is_kept_whole() {
    let obj = head_and_tail(false);
    assert!(prestripped_member(&obj) == obj);
}

/// The control: with the flag the tail is an atom of its own, and no
/// program reaches it.
#[test]
fn a_subsections_object_loses_its_unreachable_atom() {
    let obj = head_and_tail(true);
    let stripped = prestripped_member(&obj);
    assert_eq!(defined(&obj), ["_head", TAIL]);
    assert_eq!(defined(&stripped), ["_head"]);
}

fn defined(obj: &[u8]) -> Vec<&str> {
    let member = ArMember {
        name: "t.o",
        data: obj,
    };
    let syms = parse_member_defined_externs(&member).expect("symtab");
    syms.into_iter().map(|(n, _, _)| n).collect()
}
