//! Load-side layout checks of `RtLinkedImage::parse`: the runtime image
//! this build embeds parses, and each offset a loader writes through or
//! reads from is refused once it leaves the range it must lie in.

use std::sync::OnceLock;

use torajs_link::rtlinked_build::build_rtlinked_image;
use torajs_link::rtlinked_format::{FormatError, RtLinkedImage};

const BASE: u64 = 0x2_0000_0000;
/// Header word offsets (after magic, version, flags and base).
const W_TEXT: usize = 24 + 4;
const W_TEXT_CONST: usize = 24 + 4 * 5;
const W_DATA: usize = 24 + 4 * 9;
const W_TLV_TEMPLATE: usize = 24 + 4 * 13;
const W_REBASE: usize = 24 + 4 * 17;
const W_IMPORTS: usize = 24 + 4 * 19;
const W_EXPORTS: usize = 24 + 4 * 21;
const W_TLV: usize = 24 + 4 * 23;

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

fn put(b: &mut [u8], off: usize, v: u32) {
    b[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

/// A copy of the runtime image with `edit` applied.
fn edited(edit: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
    let mut b = runtime_blob().to_vec();
    edit(&mut b);
    b
}

fn layout_error(b: &[u8]) -> &'static str {
    match RtLinkedImage::parse(b) {
        Err(FormatError::BadLayout { what }) => what,
        other => panic!("expected a layout error, got {other:?}"),
    }
}

/// Blob offset of entry `i` of the table whose (off, count) header
/// words start at `word`.
fn entry(b: &[u8], word: usize, size: usize, i: usize) -> usize {
    assert!(i < u32_at(b, word + 4) as usize);
    u32_at(b, word) as usize + i * size
}

#[test]
fn embedded_runtime_passes_every_check() {
    let img = RtLinkedImage::parse(runtime_blob()).expect("parse");
    assert!(img.imports().expect("imports").len() > 1);
    for i in 0..img.header.exports.count as usize {
        img.export_at(i).expect("export");
    }
}

#[test]
fn misaligned_base_is_refused() {
    let b = edited(|b| b[16] = 0x10);
    assert_eq!(layout_error(&b), "preferred base");
}

#[test]
fn image_past_the_user_address_space_is_refused() {
    for base in [1u64 << 47, !0x3fff] {
        let b = edited(|b| b[16..24].copy_from_slice(&base.to_le_bytes()));
        assert_eq!(layout_error(&b), "preferred base", "base {base:#x}");
    }
}

#[test]
fn misaligned_text_template_is_refused() {
    let b = edited(|b| {
        let off = u32_at(b, W_TEXT + 4);
        put(b, W_TEXT + 4, off + 16);
    });
    assert_eq!(layout_error(&b), "text");
}

#[test]
fn data_template_larger_than_its_region_is_refused() {
    let b = edited(|b| {
        let file_size = u32_at(b, W_DATA + 8);
        put(b, W_DATA + 12, file_size - 8);
    });
    assert_eq!(layout_error(&b), "data");
}

#[test]
fn text_running_into_data_is_refused() {
    let b = edited(|b| {
        let reach = u32_at(b, W_DATA) - u32_at(b, W_TEXT);
        put(b, W_TEXT + 12, reach + 0x4000);
    });
    assert_eq!(layout_error(&b), "region order");
}

#[test]
fn text_constants_outside_text_are_refused() {
    let b = edited(|b| {
        let data = u32_at(b, W_DATA);
        put(b, W_TEXT_CONST, data + 0x4000);
    });
    assert_eq!(layout_error(&b), "text const");
}

#[test]
fn tlv_block_outside_data_is_refused() {
    let b = edited(|b| {
        assert!(u32_at(b, W_TLV + 4) > 0, "the runtime has thread-locals");
        put(b, W_TLV_TEMPLATE, 0);
    });
    assert_eq!(layout_error(&b), "tlv template");
}

#[test]
fn rebase_slot_past_the_data_template_is_refused() {
    let b = edited(|b| {
        let at = entry(b, W_REBASE, 4, 0);
        let data_file_end = u32_at(b, W_DATA) + u32_at(b, W_DATA + 8);
        put(b, at, data_file_end.next_multiple_of(8));
    });
    assert_eq!(layout_error(&b), "rebase slot");
}

#[test]
fn tlv_descriptor_in_text_is_refused() {
    let b = edited(|b| {
        let at = entry(b, W_TLV, 4, 0);
        put(b, at, 0);
    });
    assert_eq!(layout_error(&b), "tlv descriptor");
}

#[test]
fn tlv_offset_past_the_block_is_refused() {
    let b = edited(|b| {
        let desc = u32_at(b, entry(b, W_TLV, 4, 0));
        let at = u32_at(b, W_DATA + 4) + desc - u32_at(b, W_DATA) + 16;
        let block_len = u64::from(u32_at(b, W_TLV_TEMPLATE + 12));
        let at = at as usize;
        b[at..at + 8].copy_from_slice(&block_len.to_le_bytes());
    });
    assert_eq!(layout_error(&b), "tlv offset");
}

#[test]
fn slots_off_the_word_grid_are_refused() {
    for (word, what) in [(W_REBASE, "rebase slot"), (W_TLV, "tlv descriptor")] {
        let b = edited(|b| {
            let at = entry(b, word, 4, 0);
            let off = u32_at(b, at);
            put(b, at, off + 4);
        });
        assert_eq!(layout_error(&b), what);
    }
    let b = edited(|b| {
        let at = entry(b, W_IMPORTS, 16, 0) + 12;
        let off = u32_at(b, at);
        put(b, at, off + 4);
    });
    let img = RtLinkedImage::parse(&b).expect("parse");
    let err = img.imports().expect_err("misaligned slot");
    assert_eq!(
        err,
        FormatError::BadLayout {
            what: "import slot"
        }
    );
}

#[test]
fn import_slot_outside_data_is_refused() {
    let b = edited(|b| {
        let at = entry(b, W_IMPORTS, 16, 0);
        put(b, at + 12, 0);
    });
    let img = RtLinkedImage::parse(&b).expect("parse");
    let err = img.imports().expect_err("slot in text");
    assert_eq!(
        err,
        FormatError::BadLayout {
            what: "import slot"
        }
    );
}

#[test]
fn export_past_the_image_is_refused() {
    let b = edited(|b| {
        let at = entry(b, W_EXPORTS, 12, 0);
        let vm_size = u32_at(b, 24);
        put(b, at + 8, vm_size + 1);
    });
    let img = RtLinkedImage::parse(&b).expect("parse");
    let err = img.export_at(0).expect_err("export past the end");
    assert_eq!(err, FormatError::BadLayout { what: "export" });
}

#[test]
fn name_offset_near_u32_max_is_out_of_bounds() {
    let b = edited(|b| {
        let at = entry(b, W_EXPORTS, 12, 0);
        put(b, at, u32::MAX);
    });
    let img = RtLinkedImage::parse(&b).expect("parse");
    let err = img.export_at(0).expect_err("name past the end");
    assert_eq!(err, FormatError::OutOfBounds { what: "name" });
}
