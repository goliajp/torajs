//! Load-side layout checks on a prelinked runtime image. A loader
//! writes through the image's offsets into memory it has just mapped,
//! so [`RtLinkedImage::parse`] refuses an image whose regions, rebase
//! slots or TLV descriptors fall outside the range they name or off the
//! 8-byte grid, or whose TLV offsets point past the per-thread block;
//! import slots and export offsets are checked as they are read.

use crate::rtlinked_format::{FormatError, Region, RtLinkedImage, SEGMENT_ALIGN};

const PTR: u64 = 8;
const TLV_DESCRIPTOR: u64 = 24;
/// Byte offset of `offset` in a TLV descriptor (thunk, key, offset).
const TLV_OFFSET_FIELD: usize = 16;
/// End of the arm64 macOS user address space.
const USER_VA_END: u64 = 1 << 47;

fn bad(what: &'static str) -> FormatError {
    FormatError::BadLayout { what }
}

fn end(r: Region) -> u64 {
    u64::from(r.vm_off) + u64::from(r.vm_size)
}

/// `[off, off + len)` lies in `[start, end)`; sums in u64 cannot wrap.
fn within(off: u32, len: u64, start: u32, end: u64) -> bool {
    off >= start && u64::from(off) + len <= end
}

/// An 8-byte word (or a run of them) at `off` inside `[start, end)`.
fn word_within(off: u32, len: u64, start: u32, end: u64) -> bool {
    off.is_multiple_of(PTR as u32) && within(off, len, start, end)
}

impl RtLinkedImage<'_> {
    /// Origin-offset end of the file-backed part of the data region.
    fn data_file_end(&self) -> u64 {
        let d = self.header.data;
        u64::from(d.vm_off) + u64::from(d.file_size)
    }

    pub(crate) fn check_layout(&self) -> Result<(), FormatError> {
        let h = &self.header;
        let image_end = h.preferred_base.checked_add(h.vm_size.into());
        if !h.preferred_base.is_multiple_of(u64::from(SEGMENT_ALIGN))
            || image_end.is_none_or(|e| e > USER_VA_END)
        {
            return Err(bad("preferred base"));
        }
        for (what, r) in [("text", h.text), ("data", h.data)] {
            let aligned =
                r.vm_off.is_multiple_of(SEGMENT_ALIGN) && r.file_off.is_multiple_of(SEGMENT_ALIGN);
            if !aligned || r.file_size > r.vm_size {
                return Err(bad(what));
            }
        }
        if end(h.text) > u64::from(h.data.vm_off) || end(h.data) != u64::from(h.vm_size) {
            return Err(bad("region order"));
        }
        let tc = h.text_const;
        if !within(tc.vm_off, tc.vm_size.into(), h.text.vm_off, end(h.text)) {
            return Err(bad("text const"));
        }
        let (d0, file_end) = (h.data.vm_off, self.data_file_end());
        if h.tlv.count > 0 {
            let t = h.tlv_template;
            if !within(t.vm_off, t.vm_size.into(), d0, end(h.data))
                || !within(t.file_off, t.file_size.into(), t.vm_off, end(t))
            {
                return Err(bad("tlv template"));
            }
        }
        if !self.rebases().all(|r| word_within(r, PTR, d0, file_end)) {
            return Err(bad("rebase slot"));
        }
        if !self
            .tlv_descriptors()
            .all(|t| word_within(t, TLV_DESCRIPTOR, d0, file_end))
        {
            return Err(bad("tlv descriptor"));
        }
        let block_len = u64::from(h.tlv_template.vm_size);
        if !self
            .tlv_descriptors()
            .all(|t| self.tlv_offset(t) < block_len)
        {
            return Err(bad("tlv offset"));
        }
        Ok(())
    }

    /// The `offset` word of the TLV descriptor at origin offset `desc`,
    /// which must lie in the file-backed data.
    fn tlv_offset(&self, desc: u32) -> u64 {
        let at = (desc - self.header.data.vm_off) as usize + TLV_OFFSET_FIELD;
        u64::from_le_bytes(self.data_template()[at..at + 8].try_into().unwrap())
    }

    /// An import slot must be an 8-byte word of the data region.
    pub(crate) fn check_slot(&self, slot_off: u32) -> Result<u32, FormatError> {
        let d = self.header.data;
        if word_within(slot_off, PTR, d.vm_off, end(d)) {
            Ok(slot_off)
        } else {
            Err(bad("import slot"))
        }
    }

    /// An export names an address inside the image.
    pub(crate) fn check_export(&self, off: u32) -> Result<u32, FormatError> {
        if off <= self.header.vm_size {
            Ok(off)
        } else {
            Err(bad("export"))
        }
    }
}
