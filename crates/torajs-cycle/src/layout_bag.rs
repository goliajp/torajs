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
/// Map and Set have outgrown the name: [`crate::map`] walks their
/// entry table too, so for those two the bag is one child among
/// many. The table stays here because the offset is still where
/// their bag lives, which is what `clear_child_slot` and
/// [`crate::defer`]'s teardown route ask it for.
///
/// The same has since happened to four more slots: an iterator's
/// source ([`crate::iter_src`]), a helper's four
/// ([`crate::iter_src`] again), a promise's settled value
/// ([`crate::promise`]) and a view's buffer
/// ([`crate::view_buffer`]). What is still NOT walked is a
/// Promise's callback list, whose `arg` word the runtime cannot
/// read — it stays the shape's own destructor's, which
/// [`crate::defer`] runs on the corpse, and a missed descent
/// under-collects a cycle, never corrupts (the
/// `arr_elems_walkable` posture).
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

/// True when a corpse of this shape must be torn down by the crate
/// that owns its layout rather than by a bare `free`. Two things
/// follow from it, and they are the same thing said twice:
/// [`crate::defer`]'s pass B routes the corpse through the universal
/// dispatcher so the destructor releases what the walk never reached,
/// and `collect_white`'s second sweep must therefore NOT release
/// those same children itself.
///
/// The bag shapes qualify because their entry table / compiled
/// program / byte store is the destructor's. A Proxy qualifies for a
/// different reason: it has no bag at all, but both of its slots are
/// owned AnyValues and its cell is a sized `std::alloc` block, so the
/// array-spill fallthrough would read it at the wrong offset and free
/// it with the wrong shape.
#[inline]
pub fn corpse_takes_own_destructor(type_tag: u16) -> bool {
    bag_only_props_off(type_tag).is_some() || type_tag == crate::proxy::TAG_PROXY
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

/// True when `p` is one of the [`bag_only_props_off`] shapes and has
/// something to walk: a live bag, or — for Map and Set — a non-empty
/// entry table.
///
/// The bag read is what `is_visitable_arr` and `is_visitable_closure`
/// already do for their own expando slots, and it is what lets a
/// drop kernel hand every rc-survivor to `cycle_buffer` unconditionally
/// (the closure arm's shape) without the buffer filling with bagless
/// Maps and Dates. Like those two, the answer can flip to false after
/// a corpse's slot is cleared; `collect_white`'s second sweep and
/// `defer`'s pass A both already carry the `rc > 0` gate that makes
/// that safe.
#[inline]
pub unsafe fn is_visitable_bag(p: *mut c_void) -> bool {
    if p.is_null() {
        return false;
    }
    let header = unsafe { &*(p as *const HeapHeader) };
    if header.flags & FLAG_STATIC_LITERAL != 0 {
        return false;
    }
    let Some(off) = bag_only_props_off(header.type_tag) else {
        return false;
    };
    let props = unsafe { *((p as *const u8).add(off) as *const *mut c_void) };
    if !props.is_null() {
        return true;
    }
    // A Map or Set with no bag still owns two references per entry.
    if matches!(header.type_tag, TAG_MAP | TAG_SET) {
        return unsafe { crate::map::map_child_count(p) } > 0;
    }
    // A bagless iterator cell still owns the thing it walks.
    if crate::iter_src::is_iter_cell(header.type_tag) {
        return !unsafe { crate::iter_src::iter_src(p) }.is_null();
    }
    // A bagless helper still owns its underlying / callback / inner /
    // next.
    if crate::iter_src::is_iter_helper(header.type_tag) {
        return (0..crate::iter_src::HELPER_CHILD_COUNT)
            .any(|i| !unsafe { crate::iter_src::helper_child_at(p, i) }.is_null());
    }
    // A bagless view still owns the buffer it reads through.
    if crate::view_buffer::is_view_cell(header.type_tag) {
        return !unsafe { crate::view_buffer::view_buffer(p) }.is_null();
    }
    // A bagless promise still owns whatever it settled with.
    header.type_tag == TAG_PROMISE && !unsafe { crate::promise::promise_value(p) }.is_null()
}
