//! Layout / tag mirror constants for the enumeration choosers —
//! split out of [`crate::obj_own_keys`] (rotation 354: the promise
//! arm pushed the parent over the 500-line file cap; constants moved
//! verbatim, the parent re-exports the whole face).

/// `HeapHeader::type_tag` mirror of `torajs_rc::Tag::DynObj` (locked
/// there); header field lives at byte offset 4. Shared with the
/// values/entries chooser twin (`obj_own_values.rs`).
pub(crate) const TAG_DYNOBJ: u16 = 14;
pub(crate) const HDR_TYPE_TAG_OFF: usize = 4;

/// `torajs_rc::Tag` mirrors for the ToObject dispatch arms
/// (chunk B1 — for-in RFC): Str / Arr / Closure / Obj cells each get
/// their own own-keys shape instead of the former non-struct throw.
pub(crate) const TAG_STR_CELL: u16 = 0;
pub(crate) const TAG_OBJ_CELL: u16 = 1;
pub(crate) const TAG_ARR_CELL: u16 = 2;
pub(crate) const TAG_CLOSURE_CELL: u16 = 3;
/// `Tag::Promise` mirror — the enumeration arm walks the +32 expando
/// bag (rotation 354).
pub(crate) const TAG_PROMISE_CELL: u16 = 8;

/// ArrayBuffer cell (torajs-rc `Tag::ArrayBuffer`; expando bag at
/// +32, torajs-buffer `arraybuffer.rs::PROPS_OFF` mirror).
pub(crate) const TAG_ARRAYBUFFER_CELL: u16 = 27;
pub(crate) const ARRAYBUFFER_PROPS_OFF: usize = 32;

/// TypedArray cell (torajs-rc `Tag::TypedArray`; expando bag at
/// +40, torajs-buffer `typedarray.rs::PROPS_OFF` mirror).
pub(crate) const TAG_TYPEDARRAY_CELL: u16 = 28;
pub(crate) const TYPEDARRAY_PROPS_OFF: usize = 40;

/// DataView cell (torajs-rc `Tag::DataView`; expando bag at +32 —
/// deliberately the ArrayBuffer offset, so the two share every
/// off-32 bag consumer).
pub(crate) const TAG_DATAVIEW_CELL: u16 = 29;

/// Promise-cell lazy expando slot (`torajs_dynobj::layout::
/// PROMISE_PROPS_OFF` mirror — +24 is the callback list).
pub(crate) const PROMISE_PROPS_OFF: usize = 32;
/// RFC 20260716 刀 5 (rotation 121 chunk 5) — primitive-wrapper cell
/// tags (`torajs_rc::Tag::{NumberWrapper,StringWrapper,BooleanWrapper}`
/// mirrors). Every wrapper carries a lazy expando dynobj at
/// `WRAPPER_PROPS_OFF` (mirror of the closure `+24` slot).
pub(crate) const TAG_NUMBER_WRAPPER: u16 = 21;
pub(crate) const TAG_STRING_WRAPPER: u16 = 22;
pub(crate) const TAG_BOOLEAN_WRAPPER: u16 = 23;
pub(crate) const WRAPPER_PROPS_OFF: usize = 16;
/// Wrapper `[[StringData]]` inner Str-cell ptr slot
/// (`torajs_rc::Tag::StringWrapper` layout: `[header:8][str_cell:8]`).
pub(crate) const WRAPPER_INNER_OFF: usize = 8;

/// torajs-arr layout mirrors — `len` u64 at +8, inline props-dynobj
/// slot at +24 (`torajs_arr::layout::ARR_PROPS_OFF`).
pub(crate) const ARR_LEN_OFF: usize = 8;
pub(crate) const ARR_PROPS_OFF: usize = 24;
/// Element storage pointer (`torajs_arr::layout::ARR_DATA_PTR_OFF`).
pub(crate) const ARR_DATA_PTR_OFF: usize = 32;
/// Closure env-cell props-dynobj slot (T-27 Function-as-Object,
/// mirror `torajs_anyvalue::member_get::CLOSURE_PROPS_OFF`).
pub(crate) const CLOSURE_PROPS_OFF: usize = 24;
/// Str payload length u32 at +8 (`torajs-str` layout).
pub(crate) const STR_LEN_OFF: usize = 8;

/// ShortStr NaN-box marker (`top16 == 0x0001`) + len bits 47..40
/// (mirror `torajs_anyvalue::nanbox` SSO layout).
pub(crate) const SHORT_STR_TOP16: u64 = 0x0001;

/// Map / Set cells (`torajs_rc::Tag::{Map,Set}`; Set is layout-
/// identical to Map, so both read the same slot — torajs-collections
/// `layout::MAP_PROPS_OFF` mirror).
pub(crate) const TAG_MAP_CELL: u16 = 15;
pub(crate) const TAG_SET_CELL: u16 = 19;
pub(crate) const MAP_PROPS_OFF: usize = 48;

/// Date cell (`torajs_rc::Tag::Date`; torajs-date `DATE_PROPS_OFF`
/// mirror).
pub(crate) const TAG_DATE_CELL: u16 = 5;
pub(crate) const DATE_PROPS_OFF: usize = 16;

/// RegExp cell (`torajs_rc::Tag::RegExp`; torajs-regex
/// `regex::REGEX_PROPS_OFF` mirror — the bag sits directly after the
/// header).
pub(crate) const TAG_REGEXP_CELL: u16 = 4;
pub(crate) const REGEX_PROPS_OFF: usize = 8;

/// The three iterator cell tags (`torajs_rc::Tag::{MapIter, ArrIter,
/// IterHelper}`). ArrIter and MapIter share a layout shape, so their
/// bags sit at the same offset (`torajs_arr::iter::ARR_ITER_PROPS_OFF`
/// / `torajs_collections::iter::MAP_ITER_PROPS_OFF`); the helper cell
/// carries more state and keeps its bag past it
/// (`torajs_anyvalue::iter_helper::PROPS_OFF`).
pub(crate) const TAG_MAP_ITER_CELL: u16 = 16;
pub(crate) const TAG_ARR_ITER_CELL: u16 = 17;
pub(crate) const TAG_ITER_HELPER_CELL: u16 = 25;
pub(crate) const ITER_PROPS_OFF: usize = 32;
pub(crate) const ITER_HELPER_PROPS_OFF: usize = 56;

/// Where a cell shape keeps its lazy own-property bag, `None` when
/// the shape carries none. Twin of torajs-anyvalue's
/// `member_get_layout::expando_props_off` (the two tiers mirror
/// constants rather than share a crate, the same narrow-ABI pattern
/// every layout constant in this file follows) — the enumeration and
/// descriptor surfaces read it so they answer exactly the keys the
/// member-get channels do.
pub(crate) fn expando_props_off(tag: u16) -> Option<usize> {
    match tag {
        TAG_ARR_CELL | TAG_CLOSURE_CELL | TAG_OBJ_CELL => Some(CLOSURE_PROPS_OFF),
        TAG_NUMBER_WRAPPER | TAG_STRING_WRAPPER | TAG_BOOLEAN_WRAPPER => Some(WRAPPER_PROPS_OFF),
        TAG_PROMISE_CELL => Some(PROMISE_PROPS_OFF),
        TAG_TYPEDARRAY_CELL => Some(TYPEDARRAY_PROPS_OFF),
        TAG_ARRAYBUFFER_CELL | TAG_DATAVIEW_CELL => Some(ARRAYBUFFER_PROPS_OFF),
        TAG_MAP_CELL | TAG_SET_CELL => Some(MAP_PROPS_OFF),
        TAG_DATE_CELL => Some(DATE_PROPS_OFF),
        TAG_REGEXP_CELL => Some(REGEX_PROPS_OFF),
        TAG_MAP_ITER_CELL | TAG_ARR_ITER_CELL => Some(ITER_PROPS_OFF),
        TAG_ITER_HELPER_CELL => Some(ITER_HELPER_PROPS_OFF),
        _ => None,
    }
}

/// The shapes whose ENTIRE own face is the bag: a promise (§27.2 —
/// `then` / `catch` are prototype surface), a Map / Set (§24.1.6 /
/// §24.2.6 — the entry table is internal state), a Date (§21.4.4 —
/// so is [[DateValue]]) and the three iterator cells (§23.1.5.1 /
/// §24.1.5.1 / §27.1.4.x — so is the cursor). RegExp is deliberately
/// absent: it also owns `lastIndex` in the cell, so its arms lead
/// with that name.
#[inline]
pub(crate) fn is_bag_only_tag(tag: u16) -> bool {
    matches!(
        tag,
        TAG_PROMISE_CELL
            | TAG_MAP_CELL
            | TAG_SET_CELL
            | TAG_DATE_CELL
            | TAG_MAP_ITER_CELL
            | TAG_ARR_ITER_CELL
            | TAG_ITER_HELPER_CELL
    )
}

/// The cell's own-property bag pointer — NULL both when the shape
/// has no bag and when nothing was written into it yet.
///
/// # Safety
/// `cell` is a live heap cell whose header tag is `tag`.
pub(crate) unsafe fn expando_props(
    cell: *const core::ffi::c_void,
    tag: u16,
) -> *const core::ffi::c_void {
    match expando_props_off(tag) {
        Some(off) => {
            let raw = unsafe { (cell.cast::<u8>().add(off) as *const u64).read() };
            raw as *const core::ffi::c_void
        }
        None => core::ptr::null(),
    }
}

/// `torajs_dynobj::layout::BUCKET_FLAG_ENUMERABLE` mirror (bit 1).
pub(crate) const FLAG_ENUMERABLE: u64 = 1 << 1;

/// `AnySlotTag` heap tag (mirror torajs-anyvalue) — the tag
/// `unbox_tag` reports for any heap cell, including an AccessorPair.
pub(crate) const ANY_HEAP_TAG: i64 = 4;
/// `torajs_dynobj::accessor::TAG_ACCESSOR_PAIR` mirror.
pub(crate) const TAG_ACCESSOR_PAIR: u16 = 18;
/// Elem-kind chain stamping the entries outer array (`Arr<Arr<Any>>`:
/// heap elem = 4, inner FLAG_ARR_ANY blocks self-describe) so
/// kind-aware borrow readers (Object.fromEntries) decode the slots.
pub(crate) const KIND_CHAIN_HEAP: u64 = 4;
