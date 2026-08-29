//! The bag-only cyclic shapes — the tag/offset table for cells whose
//! single walkable child is a lazy expando props-dynobj, split out of
//! `layout.rs` (file-size HARD RULE) when rotation 528 taught the
//! collector about them.

use core::ffi::c_void;

use crate::layout::{FLAG_STATIC_LITERAL, HeapHeader};

/// Tags whose ONLY walkable child is a lazy expando props-dynobj —
/// the shapes rotation 527 gave a property face plus the ones that
/// already carried one. Mirrors torajs-anyvalue
/// `member_get_layout::expando_props_off`; kept as a table here
/// rather than a call so the collector takes no dependency on that
/// crate (the same reason `nan_box_is_cell_like` is duplicated).
///
/// What is deliberately NOT walked: a Map / Set entry table, a
/// TypedArray's buffer, a Promise's callback list, an iterator's
/// source. Those are owned by the shape's own destructor, which
/// [`crate::defer`] runs on the corpse — and a missed descent
/// under-collects a cycle, never corrupts (the `arr_elems_walkable`
/// posture). Reaching them takes each crate's layout mirror.
///
/// The four primitive wrappers stay on their own arm: a
/// StringWrapper's corpse needs its `[[StringData]]` released before
/// the block goes away, and that teardown predates this table.
pub fn bag_only_props_off(type_tag: u16) -> Option<usize> {
    Some(match type_tag {
        // torajs-regex `REGEX_PROPS_OFF` — directly after the header.
        TAG_REGEXP => 8,
        // torajs-date `DATE_PROPS_OFF`.
        TAG_DATE => 16,
        // torajs-promise props slot (+24 is the callback list);
        // torajs-buffer keeps ArrayBuffer and DataView aligned at
        // the same offset; the two iterator cells share a layout.
        TAG_PROMISE | TAG_ARRAYBUFFER | TAG_DATAVIEW | TAG_MAP_ITER | TAG_ARR_ITER => 32,
        TAG_TYPEDARRAY => 40,
        // torajs-collections `layout::MAP_PROPS_OFF` (Set is
        // layout-identical to Map).
        TAG_MAP | TAG_SET => 48,
        // torajs-anyvalue `iter_helper::PROPS_OFF`.
        TAG_ITER_HELPER => 56,
        _ => return None,
    })
}

/// `torajs_rc::Tag` mirrors for [`bag_only_props_off`]. Values are
/// the stable wire format in `torajs-rc/src/tag.rs`; do not renumber.
pub const TAG_REGEXP: u16 = 4;
/// See [`TAG_REGEXP`].
pub const TAG_DATE: u16 = 5;
/// See [`TAG_REGEXP`].
pub const TAG_PROMISE: u16 = 8;
/// See [`TAG_REGEXP`].
pub const TAG_MAP: u16 = 15;
/// See [`TAG_REGEXP`].
pub const TAG_MAP_ITER: u16 = 16;
/// See [`TAG_REGEXP`].
pub const TAG_ARR_ITER: u16 = 17;
/// See [`TAG_REGEXP`].
pub const TAG_SET: u16 = 19;
/// See [`TAG_REGEXP`].
pub const TAG_ITER_HELPER: u16 = 25;
/// See [`TAG_REGEXP`].
pub const TAG_ARRAYBUFFER: u16 = 27;
/// See [`TAG_REGEXP`].
pub const TAG_TYPEDARRAY: u16 = 28;
/// See [`TAG_REGEXP`].
pub const TAG_DATAVIEW: u16 = 29;

/// True when `p` is one of the [`bag_only_props_off`] shapes — its
/// lazy expando bag is its one walkable child. Tag-only, like
/// `layout::is_visitable_wrapper`: a NULL bag walks as zero children.
#[inline]
pub unsafe fn is_visitable_bag(p: *mut c_void) -> bool {
    if p.is_null() {
        return false;
    }
    let header = unsafe { &*(p as *const HeapHeader) };
    header.flags & FLAG_STATIC_LITERAL == 0 && bag_only_props_off(header.type_tag).is_some()
}
