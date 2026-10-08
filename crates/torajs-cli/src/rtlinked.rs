//! The runtime, linked once when tr itself is compiled, as one
//! fixed-layout image (`torajs_link::rtlinked_format`) for `tr run` to
//! map in process instead of linking the runtime on every run.
//!
//! Preferred base 0x100_0000_0000 (1 TiB). The base lies above every
//! range the kernel, dyld and libmalloc hand out in a tr process (the
//! executable, the shared region, malloc regions and stacks all sit far
//! lower) with a wide gap below it, so those ranges never grow into
//! it. The program's own code goes right after the image, within
//! branch range of it. A base that is taken anyway is handled by the
//! image's rebase table.

/// Kept by the linker although nothing reads it yet: the image must be
/// in tr's file for `tr run` to map its text from there. Only an
/// arm64 Mach-O runtime links into one.
#[cfg(all(target_arch = "aarch64", target_vendor = "apple"))]
#[used]
pub static RTLINKED_IMAGE: &[u8] = torajs_core::runtime_staticlib_envs!(
    torajs_rtlinked_macro::runtime_linked_image,
    0x100_0000_0000
);

#[cfg(all(test, target_arch = "aarch64", target_vendor = "apple"))]
mod tests {
    use super::RTLINKED_IMAGE;
    use torajs_link::rtlinked_format::{RtLinkedImage, SEGMENT_ALIGN};

    #[test]
    fn embedded_image_parses_at_the_chosen_base() {
        let img = RtLinkedImage::parse(RTLINKED_IMAGE).expect("parse");
        assert_eq!(img.header.preferred_base, 0x100_0000_0000);
        assert!(img.header.text.file_size > 0);
        assert!(img.export("___torajs_rc_inc").expect("exports").is_some());
    }

    #[test]
    fn templates_start_on_a_segment_boundary_in_memory() {
        let img = RtLinkedImage::parse(RTLINKED_IMAGE).expect("parse");
        let align = SEGMENT_ALIGN as usize;
        assert_eq!(RTLINKED_IMAGE.as_ptr() as usize % align, 0);
        assert_eq!(img.text().as_ptr() as usize % align, 0);
        assert_eq!(img.data_template().as_ptr() as usize % align, 0);
    }
}
