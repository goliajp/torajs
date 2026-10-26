//! The baked regex literals' link-layer rows: their DFA tables and
//! their compiled images.

use torajs_core::ssa::Module;
use torajs_link::exec::{UserStringEntry, UserStringKind};

/// The DFA tables of the baked regex literals that have one, in the
/// link layer's `UserBakedRegexEntry` schema (empty Vec = link layer
/// skips emit). An entry keeps its literal's index, which names its
/// `___torajs_baked_regex_<index>` meta symbol.
pub(crate) fn build_baked_regex_entries(
    ssa_module: &Module,
) -> Vec<torajs_link::exec::UserBakedRegexEntry> {
    ssa_module
        .baked_regex_entries
        .iter()
        .filter_map(|e| {
            let d = e.dfa.as_ref()?;
            Some(torajs_link::exec::UserBakedRegexEntry {
                index: e.index,
                states_payload: d.states_payload.clone(),
                states_len: d.states_len,
                start: d.start,
                start_mid: d.start_mid,
                start_mid_word: d.start_mid_word,
                start_mid_nonword: d.start_mid_nonword,
                any_accept_before_byte: d.any_accept_before_byte,
            })
        })
        .collect()
}

/// Every baked regex literal's compiled image, as a raw-byte literal at
/// `__torajs_regex_image_<index>` (8-aligned, which the image's
/// in-place property tables rely on).
pub(crate) fn build_regex_images(
    ssa_module: &Module,
) -> impl Iterator<Item = UserStringEntry> + '_ {
    ssa_module
        .baked_regex_entries
        .iter()
        .map(|e| UserStringEntry {
            sym: torajs_core::ssa::BakedRegexEntry::image_sym(e.index),
            bytes: e.image.clone(),
            is_latin1: true,
            length: e.image.len() as u32,
            kind: UserStringKind::RawBytes,
            payload_alias: None,
        })
}
