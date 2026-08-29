//! Cell-layout mirrors + tag/flag probes shared by the member-get
//! family (`member_get` / `member_get_own` / the special-prop
//! getters). Split out of `member_get.rs` (file-size HARD RULE —
//! the L3b ⑧ class-prototype-chain arm pushed it past 500); every
//! item keeps its `crate::member_get::` face through the re-export
//! there.

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::nanbox::{AnyValue, as_void_ptr, is_cell};

/// Closure-cell lazy props slot — mirror of torajs-core
/// `ssa_lower.rs::CLOSURE_PROPS_OFF`.
pub(crate) const CLOSURE_PROPS_OFF: usize = 24;

/// Struct-cell (`Tag::Obj`) lazy props slot — mirror of torajs-core
/// `ssa_lower.rs::OBJ_PROPS_OFF` (RFC 20260714-struct-dynamic-props
/// blade 1/2). NULL until the first expando write through the `any`
/// lane.
pub(crate) const OBJ_PROPS_OFF: usize = 24;

/// The struct cell's `props_dynobj` pointer, NULL when no expando
/// was ever written. Same read shape as [`wrapper_props`].
pub(crate) unsafe fn struct_props(ptr: *const c_void) -> *const c_void {
    unsafe { *(ptr.cast::<u8>().add(OBJ_PROPS_OFF) as *const u64) as *const c_void }
}

/// Wrapper-cell lazy props slot — mirror of
/// `torajs-wrapper::WRAPPER_PROPS_OFF` (RFC 20260716 刀 5, rotation
/// 121). Every wrapper cell layout is `[header:8][value:8][props:8]`.
const WRAPPER_PROPS_OFF: usize = 16;

/// The wrapper's `props_dynobj` pointer, NULL when no expando was
/// ever written. Same read shape as `closure_props`.
pub(crate) unsafe fn wrapper_props(ptr: *mut c_void) -> *const c_void {
    unsafe { *(ptr.cast::<u8>().add(WRAPPER_PROPS_OFF) as *const u64) as *const c_void }
}

#[inline]
pub(crate) fn is_wrapper_tag(t: u16) -> bool {
    t == Tag::NumberWrapper as u16
        || t == Tag::StringWrapper as u16
        || t == Tag::BooleanWrapper as u16
        || t == Tag::SymbolWrapper as u16
}

/// The builtin-prototype registry tag a wrapper cell's [[Prototype]]
/// resolves to (`torajs_rc::builtin_proto` index space) — the chain
/// parent for own-miss reads on a wrapper receiver. `None` for a
/// non-wrapper tag (and for SymbolWrapper, whose prototype carries
/// no user-installable expando face the chain reads yet).
#[inline]
pub(crate) fn wrapper_proto_tag(t: u16) -> Option<i64> {
    use torajs_rc::builtin_proto::{BOOLEAN_PROTO_TAG, NUMBER_PROTO_TAG, STRING_PROTO_TAG};
    if t == Tag::NumberWrapper as u16 {
        Some(NUMBER_PROTO_TAG as i64)
    } else if t == Tag::BooleanWrapper as u16 {
        Some(BOOLEAN_PROTO_TAG as i64)
    } else if t == Tag::StringWrapper as u16 {
        Some(STRING_PROTO_TAG as i64)
    } else {
        None
    }
}

pub(crate) const STR_LEN_OFF: usize = 8;
pub(crate) const STR_DATA_OFF: usize = 16;

/// Symbol-cell description Str slot — mirror of
/// `torajs-str::symbol::SYMBOL_DESC_OFF`.
const SYMBOL_DESC_OFF: usize = 8;

/// The symbol's description Str pointer, NULL for `Symbol()`.
/// Borrow-shaped like every member-get probe answer — the desc's
/// stake lives on the symbol cell.
pub(crate) unsafe fn symbol_desc(ptr: *const c_void) -> *mut c_void {
    unsafe { *(ptr.cast::<u8>().add(SYMBOL_DESC_OFF) as *const *mut c_void) }
}

/// The closure's `props_dynobj` pointer, NULL when no expando was
/// ever written.
pub(crate) unsafe fn closure_props(ptr: *mut c_void) -> *const c_void {
    unsafe { *(ptr.cast::<u8>().add(CLOSURE_PROPS_OFF) as *const u64) as *const c_void }
}

/// Promise-cell lazy expando slot — mirror of `torajs-promise`'s
/// props slot @ +32 (rotation 352 `478088d4`; +24 is the callback
/// list). NULL until the first defineProperty against the promise.
const PROMISE_PROPS_OFF: usize = 32;

/// The promise's `props_dynobj` pointer, NULL when no expando was
/// ever written (see [`PROMISE_PROPS_OFF`]).
pub(crate) unsafe fn promise_props(ptr: *mut c_void) -> *const c_void {
    unsafe { expando_props(ptr, Tag::Promise as u16) }
}

/// Buffer-family lazy expando slots — mirrors of torajs-buffer
/// `arraybuffer.rs::PROPS_OFF` / `typedarray.rs::PROPS_OFF`. Both
/// cells are ordinary objects on their non-index face (§25.1 /
/// §23.2), so a define / plain assign needs somewhere to land — the
/// test262 species cases install a throwing `constructor` getter ON
/// THE INSTANCE.
pub(crate) const ARRAYBUFFER_PROPS_OFF: usize = 32;
pub(crate) const TYPEDARRAY_PROPS_OFF: usize = 40;

/// The buffer-family cell's `props_dynobj` pointer, NULL when no
/// own property was ever written. `tag` picks the slot offset.
pub(crate) unsafe fn buffer_props(ptr: *mut c_void, tag: u16) -> *const c_void {
    unsafe { expando_props(ptr, tag) }
}

/// Map / Set cell lazy expando slot — mirror of torajs-collections
/// `layout::MAP_PROPS_OFF`. Set is layout-identical to Map, so both
/// tags read the same offset.
pub(crate) const MAP_PROPS_OFF: usize = 48;

/// Date cell lazy expando slot — mirror of torajs-date's
/// `DATE_PROPS_OFF`.
pub(crate) const DATE_PROPS_OFF: usize = 16;

/// RegExp cell lazy expando slot — mirror of torajs-regex's
/// `regex::REGEX_PROPS_OFF`. It sits directly after the header so
/// the offset survives any reshuffle of the compiled program below.
pub(crate) const REGEX_PROPS_OFF: usize = 8;

/// ArrIter / MapIter cell lazy expando slot — mirrors of
/// `torajs_arr::iter::ARR_ITER_PROPS_OFF` and
/// `torajs_collections::iter::MAP_ITER_PROPS_OFF`. The two layouts
/// are the same shape (header, source, cursor, two words), so both
/// bags sit at the same offset.
pub(crate) const ITER_PROPS_OFF: usize = 32;

/// IterHelper cell lazy expando slot — mirror of
/// `crate::iter_helper::PROPS_OFF`.
pub(crate) const ITER_HELPER_PROPS_OFF: usize = 56;

/// Where a cell shape keeps its lazy own-property bag — `None` when
/// the shape carries none, and every caller then knows the answer is
/// "this receiver has no ordinary own face".
///
/// One table, read by both get channels, the assign ladder and the
/// reflection surfaces, so a shape that grows a bag is a single line
/// here rather than a new `cell_tag ==` arm in each of them. `Tag::Arr`
/// is deliberately absent: an array's expando lives behind the
/// `arrprops_*` kernels (index keys share the bucket), not in a bag
/// the plain `dynobj_*` probes may read.
pub(crate) fn expando_props_off(tag: u16) -> Option<usize> {
    if tag == Tag::Closure as u16 || tag == Tag::Obj as u16 {
        Some(CLOSURE_PROPS_OFF)
    } else if is_wrapper_tag(tag) {
        Some(WRAPPER_PROPS_OFF)
    } else if tag == Tag::Promise as u16 {
        Some(PROMISE_PROPS_OFF)
    } else if tag == Tag::TypedArray as u16 {
        Some(TYPEDARRAY_PROPS_OFF)
    } else if tag == Tag::ArrayBuffer as u16 || tag == Tag::DataView as u16 {
        // ArrayBuffer and DataView deliberately share +32
        // (torajs-buffer keeps the two cells' props slots aligned).
        Some(ARRAYBUFFER_PROPS_OFF)
    } else if tag == Tag::Map as u16 || tag == Tag::Set as u16 {
        Some(MAP_PROPS_OFF)
    } else if tag == Tag::Date as u16 {
        Some(DATE_PROPS_OFF)
    } else if tag == Tag::RegExp as u16 {
        Some(REGEX_PROPS_OFF)
    } else if tag == Tag::ArrIter as u16 || tag == Tag::MapIter as u16 {
        Some(ITER_PROPS_OFF)
    } else if tag == Tag::IterHelper as u16 {
        Some(ITER_HELPER_PROPS_OFF)
    } else {
        None
    }
}

/// The shapes whose ENTIRE ordinary own face is the bag: Map / Set
/// (§24.1.6 / §24.2.6), Date (§21.4.4), RegExp (§22.2.6) and the
/// three iterator cells ([`is_iter_cell_tag`]). Their entry table,
/// [[DateValue]], compiled program and iteration cursor are internal
/// state carried in the cell, never properties — so apart from
/// RegExp's own `lastIndex` every name key they answer, and every
/// name key they accept, is the bag's.
#[inline]
pub(crate) fn is_stateful_bag_tag(t: u16) -> bool {
    t == Tag::Map as u16
        || t == Tag::Set as u16
        || t == Tag::Date as u16
        || t == Tag::RegExp as u16
        || is_iter_cell_tag(t)
}

/// The three iterator cell shapes. §23.1.5.1 / §22.1.5.1 / §24.1.5.1
/// / §24.2.5.1 / §27.1.4.x all mint ORDINARY objects: the source, the
/// cursor, the captured callback and the alive flag are internal
/// state, so like the four above their whole property face is the
/// bag. They were the last receivers in the language that answered
/// every assign with the boundary TypeError.
#[inline]
pub(crate) fn is_iter_cell_tag(t: u16) -> bool {
    t == Tag::ArrIter as u16 || t == Tag::MapIter as u16 || t == Tag::IterHelper as u16
}

/// The cell's own-property bag pointer, NULL both when the shape has
/// no bag at all and when nothing was ever written into it — the two
/// answer the same way to every probe below.
///
/// # Safety
/// `ptr` is a live heap cell whose header tag is `tag`.
pub(crate) unsafe fn expando_props(ptr: *const c_void, tag: u16) -> *const c_void {
    match expando_props_off(tag) {
        Some(off) => unsafe { *(ptr.cast::<u8>().add(off) as *const u64) as *const c_void },
        None => core::ptr::null(),
    }
}

/// Universal heap-header flags probe — u16 at +6 (RFC 20260711
/// chunk C consumers test the `FLAG_FN_*_DELETED` tombstones).
///
/// # Safety
/// `ptr` is a live heap cell.
pub(crate) unsafe fn header_flag(ptr: *const c_void, bit: u16) -> bool {
    unsafe { (ptr.cast::<u8>().add(6) as *const u16).read() & bit != 0 }
}

/// Set a heap-header flag bit (read-or-write, u16 at +6).
///
/// # Safety
/// `ptr` is a live heap cell.
pub(crate) unsafe fn header_flag_set(ptr: *mut c_void, bit: u16) {
    unsafe {
        let p = ptr.cast::<u8>().add(6) as *mut u16;
        p.write(p.read() | bit);
    }
}

/// Cell tag of a dispatchable receiver, `None` for everything the
/// gate answers `(ANY_UNDEF, 0)` for.
pub(crate) fn recv_cell(recv: AnyValue) -> Option<(*mut c_void, u16)> {
    if !is_cell(recv) {
        return None;
    }
    let ptr = as_void_ptr(recv);
    // SAFETY: is_cell guarantees a non-null encoded pointer; the
    // caller invariant says it points to a live heap object.
    let tag = unsafe { (ptr.cast::<u8>().add(4) as *const u16).read() };
    Some((ptr, tag))
}
