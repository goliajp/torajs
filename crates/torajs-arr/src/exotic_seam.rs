//! The typed kernels' exotic-array slow paths behind link seams
//! (RFC 20260824-s2-5 刀 4 Phase B, A2/A3).
//!
//! `arr_join_i64` and friends are the typed lane's kernels, yet each
//! carries one branch for an array that stopped being dense —
//! `FLAG_ARR_EXOTIC_INDEX` (an accessor / hole / shadowed index) —
//! and that branch calls the any-world join. The species guard is
//! the same shape: one NULL check on the props slot, and a slow path
//! that reads the `constructor` expando through the accessor
//! machinery. Both slow paths are the typed world's only references
//! into the generic any dispatcher, and either alone roots it whole
//! (Phase A: `[1,2,3].join(",")` links 348 KB, of which the join's
//! exotic branch accounts for 264 KB).
//!
//! The scalar-array drop (`__torajs_arr_drop_scalar`, r500 A4') is
//! the third member of the family: a `number[]` / `boolean[]` can
//! never hold a heap pointer (the any lane coerces or refuses a
//! kind-mismatched store, it never re-kinds), so its drop has no
//! element walk and no cycle-buffer hook; its only slow legs are a
//! props bag to release and a subclass envelope to unwind, each
//! behind a seam guarded on that state's sole writer.
//!
//! Two facts make the link the right judge:
//!
//! - an array only becomes exotic through [`__torajs_arr_flag_exotic`]
//!   and only grows a props bag through [`__torajs_arr_props_attach`]
//!   (or the regex exec attach, which builds the bag itself) — every
//!   writer in this crate goes through those `#[inline(never)]`
//!   entries, so "is that entry's text live" is exactly "can any
//!   array in this artifact reach that state";
//! - the slow paths sit behind `extern "C"` declarations whose
//!   default definitions live in `torajs-dispatch` (a separate
//!   archive member), so a user-`.o` stub can shadow them
//!   (`torajs-link::dead_strip_elide::GuardedStub`, guard =
//!   `Guard::Symbols` on the writer entries).
//!
//! A wrongly applied stub — a writer this crate grows later without
//! routing through the entries above — is LOUD: the stub records a
//! named TypeError (`torajs-dispatch`'s landing pad), never a wrong
//! answer.

use core::ffi::c_void;

use torajs_rc::{
    FLAG_ARR_ANY_HOLEY, FLAG_ARR_EXOTIC_INDEX, FLAG_ARR_LENGTH_RO, FLAG_ARR_SPARSE_TAIL,
};

unsafe extern "C" {
    fn __torajs_dynobj_alloc() -> *mut c_void;
    /// torajs-anyvalue — the kind-aware any-world join (holes consult
    /// the prototype, getters run).
    fn __torajs_arr_join_any(arr: *const u8, sep: *const u8) -> *mut u8;
    /// torajs-anyvalue — per-element Invoke("toLocaleString") walk.
    fn __torajs_arr_any_to_locale_string(arr: *mut c_void) -> *mut u8;
}

// The seams. Default definitions: `torajs-dispatch`. Unit-test
// binaries of this crate carry no dispatch member, so they bridge to
// the impls below.
#[cfg(not(test))]
unsafe extern "C" {
    /// Exotic-receiver join for a typed-kernel receiver. `kind` is the
    /// `ARR_KIND_*` the any kernel needs stamped; `locale != 0` picks
    /// the toLocaleString walk (no kind stamp — the locale kernel
    /// reads through the flags it finds).
    pub(crate) fn __torajs_arr_join_exotic(
        arr: *const u8,
        sep: *const u8,
        kind: u64,
        locale: i64,
    ) -> *mut u8;
    /// Species guard slow path — the receiver has a props bag.
    pub(crate) fn __torajs_arr_species_guard_slow(arr: *const u8) -> i64;
    /// Scalar-array drop, props leg — the dying array has a props
    /// bag to release (`__torajs_arrprops_drop_entry`).
    pub(crate) fn __torajs_arr_drop_props_slow(arr: *mut c_void);
    /// Scalar-array drop, subclass leg — the dying array wears
    /// `FLAG_SUBCLASSED` (`__torajs_subclass_drop_entry`).
    pub(crate) fn __torajs_arr_drop_subclass_slow(arr: *mut c_void);
    /// Typed read at a non-integral index, slow leg — the cell can
    /// carry a named property, so the keyed any-world read answers
    /// (in the F64 slot's shape).
    pub(crate) fn __torajs_arr_typed_nonint_f64_slow(arr: *mut c_void, key: f64) -> f64;
    /// `string[]` twin: an owned Str cell, or NULL for undefined.
    pub(crate) fn __torajs_arr_typed_nonint_str_slow(arr: *mut c_void, key: f64) -> *mut c_void;
    /// `boolean[]` / pointer-element twin: the AnyValue answer when
    /// `keep != 0`, otherwise `undefined` after the read has run.
    pub(crate) fn __torajs_arr_typed_nonint_any_slow(arr: *mut c_void, key: f64, keep: i64) -> u64;
    /// `shift` on a non-writable `length`, the move-and-delete leg
    /// (`__torajs_arr_shift_ro_length_impl`).
    pub(crate) fn __torajs_arr_shift_ro_length_slow(arr: *mut c_void) -> i64;
}

#[cfg(test)]
pub(crate) unsafe fn __torajs_arr_shift_ro_length_slow(arr: *mut c_void) -> i64 {
    unsafe { crate::sealed_refuse::__torajs_arr_shift_ro_length_impl(arr) }
}

#[cfg(test)]
pub(crate) unsafe fn __torajs_arr_join_exotic(
    arr: *const u8,
    sep: *const u8,
    kind: u64,
    locale: i64,
) -> *mut u8 {
    unsafe { __torajs_arr_join_exotic_impl(arr, sep, kind, locale) }
}

#[cfg(test)]
pub(crate) unsafe fn __torajs_arr_species_guard_slow(arr: *const u8) -> i64 {
    unsafe { crate::species::__torajs_arr_species_guard_props(arr) }
}

#[cfg(test)]
pub(crate) unsafe fn __torajs_arr_drop_props_slow(arr: *mut c_void) {
    unsafe { crate::props::__torajs_arrprops_drop_entry(arr) }
}

#[cfg(test)]
pub(crate) unsafe fn __torajs_arr_drop_subclass_slow(arr: *mut c_void) {
    unsafe extern "C" {
        fn __torajs_subclass_drop_entry(p: *mut c_void);
    }
    unsafe { __torajs_subclass_drop_entry(arr) }
}

#[cfg(test)]
pub(crate) unsafe fn __torajs_arr_typed_nonint_f64_slow(_arr: *mut c_void, _key: f64) -> f64 {
    panic!("torajs-arr unit-test stub: the keyed read lives in torajs-anyvalue");
}

#[cfg(test)]
pub(crate) unsafe fn __torajs_arr_typed_nonint_str_slow(
    _arr: *mut c_void,
    _key: f64,
) -> *mut c_void {
    panic!("torajs-arr unit-test stub: the keyed read lives in torajs-anyvalue");
}

#[cfg(test)]
pub(crate) unsafe fn __torajs_arr_typed_nonint_any_slow(
    _arr: *mut c_void,
    _key: f64,
    _keep: i64,
) -> u64 {
    panic!("torajs-arr unit-test stub: the keyed read lives in torajs-anyvalue");
}

/// Raise the header's exotic-index bit. The ONE writer of that bit —
/// `#[inline(never)]` so its text atom is the link-time evidence
/// that some live code can make an array exotic.
///
/// # Safety
/// `arr` is a live `Tag::Arr` heap block.
#[inline(never)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_flag_exotic(arr: *mut c_void) {
    let p = unsafe { (arr as *mut u8).add(6) as *mut u16 };
    unsafe { p.write(p.read() | FLAG_ARR_EXOTIC_INDEX) };
}

/// Raise the header's slot-hole bit on an Any cell. The ONE writer of
/// `FLAG_ARR_ANY_HOLEY` — the `new Array(n)` mint, the last-index
/// hole mark, and the slice / splice products that carry a source
/// hole all come through here (a header-bits copy such as
/// `layout::copy_elem_desc_bits` only propagates a bit this entry
/// raised). Together with [`__torajs_arr_flag_exotic`] it is the
/// whole enabler set of `layout::needs_index_mop`, so the two
/// entries' text liveness is the link-time evidence for the typed
/// sort kernel's exotic divert (`__torajs_arr_sort_exotic`).
///
/// # Safety
/// `arr` is a live `FLAG_ARR_ANY` `Tag::Arr` heap block.
#[inline(never)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_flag_any_holey(arr: *mut c_void) {
    let p = unsafe { (arr as *mut u8).add(6) as *mut u16 };
    unsafe { p.write(p.read() | FLAG_ARR_ANY_HOLEY) };
}

/// Raise the header's sparse-tail bit. The ONE writer of
/// `FLAG_ARR_SPARSE_TAIL` — the giant `length` grow on an Any cell
/// (`set_length`) — so its text liveness is the third enabler of the
/// callback methods' absent-index gate: that gate reads the exotic,
/// slot-hole and sparse-tail bits in the preheader, and `map`'s hole
/// arm behind it (`__torajs_arr_map_hole_slow`) is judged on the three
/// writers together.
///
/// # Safety
/// `arr` is a live `FLAG_ARR_ANY` `Tag::Arr` heap block.
#[inline(never)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_flag_sparse_tail(arr: *mut c_void) {
    let p = unsafe { (arr as *mut u8).add(6) as *mut u16 };
    unsafe { p.write(p.read() | FLAG_ARR_SPARSE_TAIL) };
}

/// Make the header's `length` non-writable. The ONE writer of
/// `FLAG_ARR_LENGTH_RO` on a receiver that is not frozen (a freeze
/// raises it next to FLAG_FROZEN, and every reader that matters here
/// answers a frozen receiver first), so its text liveness is the
/// link-time evidence for `shift`'s RO-length leg
/// (`__torajs_arr_shift_ro_length_slow`).
///
/// # Safety
/// `arr` is a live `Tag::Arr` heap block.
#[inline(never)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_flag_length_ro(arr: *mut c_void) {
    let p = unsafe { (arr as *mut u8).add(6) as *mut u16 };
    unsafe { p.write(p.read() | FLAG_ARR_LENGTH_RO) };
}

/// The array's props slot, with the bag allocated on first use. The
/// ONE writer that turns a NULL props slot into a bag that can hold a
/// named property (the regex exec attach builds its bag through the
/// dynobj side and is listed next to this entry in the link guard;
/// hole entries take [`__torajs_arr_props_attach_index`]) —
/// `#[inline(never)]` for the same reason as
/// [`__torajs_arr_flag_exotic`].
///
/// # Safety
/// `arr` is a live array heap block.
#[inline(never)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_props_attach(arr: *mut c_void) -> *mut *mut c_void {
    let slot = unsafe { (arr as *mut u8).add(crate::layout::ARR_PROPS_OFF) as *mut *mut c_void };
    if unsafe { (*slot).is_null() } {
        unsafe { *slot = __torajs_dynobj_alloc() };
    }
    slot
}

/// The props slot for a hole entry at an array index (a delete, the
/// gap of a grow), with the exotic bit raised. A bag this entry
/// creates holds index keys only until another writer adds to it, so
/// it is not evidence for a named property: the non-integral reads'
/// slow seams are judged on [`__torajs_arr_props_attach`] alone, and
/// the readers of any bag on both. The flag call keeps the body
/// distinct from the plain attach, which MergeFunctions would
/// otherwise fold onto one address and so make one writer's liveness
/// the other's.
///
/// # Safety
/// `arr` is a live array heap block.
#[inline(never)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_props_attach_index(arr: *mut c_void) -> *mut *mut c_void {
    let slot = unsafe { (arr as *mut u8).add(crate::layout::ARR_PROPS_OFF) as *mut *mut c_void };
    if unsafe { (*slot).is_null() } {
        unsafe { *slot = __torajs_dynobj_alloc() };
    }
    unsafe { __torajs_arr_flag_exotic(arr) };
    slot
}

/// The default body behind the `__torajs_arr_join_exotic` seam:
/// stamp the typed kernel's element kind and hand the receiver to
/// the any-world join (or the locale walk).
///
/// # Safety
/// `arr` is a live array heap block; `sep` a live Str (ignored by
/// the locale walk).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_join_exotic_impl(
    arr: *const u8,
    sep: *const u8,
    kind: u64,
    locale: i64,
) -> *mut u8 {
    unsafe {
        if locale != 0 {
            return __torajs_arr_any_to_locale_string(arr as *mut u8 as *mut c_void);
        }
        crate::mark_kind::__torajs_arr_mark_kind(arr as *mut u8 as *mut c_void, kind);
        __torajs_arr_join_any(arr, sep)
    }
}
