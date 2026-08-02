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
