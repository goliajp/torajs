//! Serialise the parts of a prelinked runtime image into the byte
//! format [`crate::rtlinked_format`] reads.

use std::collections::BTreeMap;

use crate::rtlinked_format::{HEADER_SIZE, Header, ImportKind, Region, SEGMENT_ALIGN, Table};

/// Everything the image holds, in origin offsets.
#[derive(Debug, Clone, Default)]
pub struct ImageParts {
    pub preferred_base: u64,
    pub text: Vec<u8>,
    pub text_vm_off: u32,
    /// `(vm_off, size)` of the `__TEXT` constant sections inside `text`.
    pub text_const: (u32, u32),
    pub data: Vec<u8>,
    pub data_vm_off: u32,
    /// Data template plus zero-fill.
    pub data_vm_size: u32,
    /// `(vm_off, size)` of the whole per-thread block.
    pub tlv_span: (u32, u32),
    /// `(vm_off, size)` of its initialised part.
    pub tlv_init: (u32, u32),
    pub rebases: Vec<u32>,
    pub imports: Vec<(ImportKind, String, u32)>,
    pub exports: BTreeMap<String, u32>,
    pub tlv: Vec<u32>,
}

fn align_up(v: usize, a: u32) -> usize {
    v.next_multiple_of(a as usize)
}

fn table(out: &mut Vec<u8>, count: usize, bytes: impl IntoIterator<Item = u8>) -> Table {
    let off = out.len() as u32;
    out.extend(bytes);
    Table {
        off,
        count: count as u32,
    }
}

fn u32s(vals: &[u32]) -> Vec<u8> {
    vals.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// Lay the parts out as header, tables, names, then the text and data
/// templates on [`SEGMENT_ALIGN`] boundaries.
pub fn encode_image(p: &ImageParts) -> Vec<u8> {
    let mut names: Vec<u8> = Vec::new();
    let mut name_ref = |n: &str| {
        let off = names.len() as u32;
        names.extend_from_slice(n.as_bytes());
        [off, n.len() as u32]
    };
    let imports: Vec<u32> = p
        .imports
        .iter()
        .flat_map(|(k, n, slot)| {
            let [off, len] = name_ref(n);
            [*k as u32, off, len, *slot]
        })
        .collect();
    let exports: Vec<u32> = p
        .exports
        .iter()
        .flat_map(|(n, off)| {
            let [noff, len] = name_ref(n);
            [noff, len, *off]
        })
        .collect();

    let mut out = vec![0u8; HEADER_SIZE];
    let mut h = Header {
        preferred_base: p.preferred_base,
        ..Header::default()
    };
    h.rebase = table(&mut out, p.rebases.len(), u32s(&p.rebases));
    h.imports = table(&mut out, p.imports.len(), u32s(&imports));
    h.exports = table(&mut out, p.exports.len(), u32s(&exports));
    h.tlv = table(&mut out, p.tlv.len(), u32s(&p.tlv));
    h.names = table(&mut out, names.len(), names);

    out.resize(align_up(out.len(), SEGMENT_ALIGN), 0);
    h.text = Region {
        vm_off: p.text_vm_off,
        file_off: out.len() as u32,
        file_size: p.text.len() as u32,
        vm_size: align_up(p.text.len(), SEGMENT_ALIGN) as u32,
    };
    out.extend_from_slice(&p.text);
    out.resize(align_up(out.len(), SEGMENT_ALIGN), 0);
    h.data = Region {
        vm_off: p.data_vm_off,
        file_off: out.len() as u32,
        file_size: p.data.len() as u32,
        vm_size: p.data_vm_size,
    };
    out.extend_from_slice(&p.data);
    h.text_const = Region {
        vm_off: p.text_const.0,
        vm_size: p.text_const.1,
        ..Region::default()
    };
    h.tlv_template = Region {
        vm_off: p.tlv_span.0,
        vm_size: p.tlv_span.1,
        file_off: p.tlv_init.0,
        file_size: p.tlv_init.1,
    };
    h.vm_size = p.data_vm_off + p.data_vm_size;
    out[..HEADER_SIZE].copy_from_slice(&h.encode());
    out
}
