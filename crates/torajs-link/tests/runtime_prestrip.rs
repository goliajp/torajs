//! The pre-stripped runtime image on synthetic archives: what it may
//! cut in a member and what it must leave whole.

use torajs_codegen::CompiledFunction;
use torajs_codegen::frame::FrameLayout;
use torajs_link::runtime_prestrip::prestripped_runtime_image;
use torajs_obj::MH_SUBSECTIONS_VIA_SYMBOLS;
use torajs_rtimage::archive::{ArMember, parse_member_defined_externs};
use torajs_rtimage::image::Image;

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
