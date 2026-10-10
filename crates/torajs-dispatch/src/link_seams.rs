//! Default resolutions of the link-judged slow-path seams (RFC
//! 20260824-s2-5 刀 4).
//!
//! The typed kernels and the synthesized drops reach their
//! exotic-receiver slow paths through these seams; the link judgment
//! (`torajs-link::dead_strip_elide`, guard = each state's writer
//! entries) shadows one with a loud-reject stub when nothing in the
//! artifact can put a value into the state it serves. Same
//! archive-member argument as the arm seams in the crate root.

unsafe extern "C" {
    fn __torajs_arr_join_exotic_impl(
        arr: *const u8,
        sep: *const u8,
        kind: u64,
        locale: i64,
    ) -> *mut u8;
    fn __torajs_arr_species_guard_props(arr: *const u8) -> i64;
    fn __torajs_arrprops_drop_entry(arr: *mut core::ffi::c_void);
    fn __torajs_subclass_drop_entry(p: *mut core::ffi::c_void);
    fn __torajs_value_drop_heap(p: *mut core::ffi::c_void);
    fn __torajs_cycle_unbuffer(p: *mut core::ffi::c_void);
    fn __torajs_cycle_buffer(p: *mut core::ffi::c_void);
    fn __torajs_uncaught_error_render_impl(p: *const u8);
    fn __torajs_anyv_rc_dec(v: u64);
    fn __torajs_obj_check_field_writable_impl(p: *const u8, name: *const u8);
    fn __torajs_arr_sort_exotic_impl(arr: *mut core::ffi::c_void, cmp: u64) -> u64;
    fn __torajs_arr_mark_last_hole(arr: *mut core::ffi::c_void);
    fn __torajs_arr_typed_nonint_f64_impl(arr: *mut core::ffi::c_void, key: f64) -> f64;
    fn __torajs_arr_typed_nonint_str_impl(
        arr: *mut core::ffi::c_void,
        key: f64,
    ) -> *mut core::ffi::c_void;
    fn __torajs_arr_typed_nonint_any_impl(arr: *mut core::ffi::c_void, key: f64, keep: i64) -> u64;
}

/// Default resolution of a typed `number[]` read at a non-integral
/// index, slow leg (§7.1.19 names the property "1.5"): the keyed
/// any-world read in torajs-anyvalue. torajs-arr calls it only for a
/// cell that can carry a named property; the link judgment stubs it
/// (fam 52) when no array can grow a props bag, no builtin prototype
/// can be minted and no Array subclass is allocated.
///
/// # Safety
/// `arr` is a live `Tag::Arr` cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_typed_nonint_f64_slow(
    arr: *mut core::ffi::c_void,
    key: f64,
) -> f64 {
    unsafe { __torajs_arr_typed_nonint_f64_impl(arr, key) }
}

/// The `string[]` twin of [`__torajs_arr_typed_nonint_f64_slow`]
/// (fam 53).
///
/// # Safety
/// `arr` is a live `Tag::Arr` cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_typed_nonint_str_slow(
    arr: *mut core::ffi::c_void,
    key: f64,
) -> *mut core::ffi::c_void {
    unsafe { __torajs_arr_typed_nonint_str_impl(arr, key) }
}

/// The `boolean[]` / pointer-element twin of
/// [`__torajs_arr_typed_nonint_f64_slow`] (fam 56).
///
/// # Safety
/// `arr` is a live `Tag::Arr` cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_typed_nonint_any_slow(
    arr: *mut core::ffi::c_void,
    key: f64,
    keep: i64,
) -> u64 {
    unsafe { __torajs_arr_typed_nonint_any_impl(arr, key, keep) }
}

/// Default resolution of the static `map` lane's hole arm: an absent
/// source index arrives in the product as a hole of its own
/// (§23.1.3.21 step 6.c), marked by torajs-arr's elision writer. The
/// loop copy that calls it runs only after a header probe found the
/// source exotic, slot-holey or sparse-tailed, so the link judgment
/// stubs it (fam 54) when none of `__torajs_arr_flag_exotic`,
/// `__torajs_arr_flag_any_holey` and `__torajs_arr_flag_sparse_tail`
/// has live text. The dense copy reaches the writer directly, not
/// through this seam.
///
/// # Safety
/// `arr` is a live `Tag::Arr` product cell whose last slot was just
/// pushed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_map_hole_slow(arr: *mut core::ffi::c_void) {
    unsafe { __torajs_arr_mark_last_hole(arr) }
}

/// Default resolution of the typed sort lane's exotic divert: the
/// §23.1.3.30 SortIndexedProperties walk in torajs-anyvalue for a
/// receiver carrying a hole or an accessor index. The link judgment
/// stubs it (fam 51) when neither `__torajs_arr_flag_exotic` nor
/// `__torajs_arr_flag_any_holey` has live text — no array in the
/// artifact can take the divert.
///
/// The kernel answers the receiver +1 (the any lane's method result);
/// the typed caller still holds its own reference, so that stake is
/// released here rather than in the user `.o`, whose generic any
/// drop would root the cycle and weak worlds in every sorting
/// program.
///
/// # Safety
/// `arr` is a live `Tag::Arr` cell; `cmp` is a borrowed AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_sort_exotic(arr: *mut core::ffi::c_void, cmp: u64) {
    unsafe { __torajs_anyv_rc_dec(__torajs_arr_sort_exotic_impl(arr, cmp)) }
}

/// Default resolution of the typed field-write guard (r503): the
/// frozen / redefined-field refusal behind every `instance.field =
/// v` store, called only when the header bits hit. The link judgment
/// stubs it (fam 50) when neither bit has a live writer
/// (`__torajs_obj_freeze` / `__torajs_obj_flag_exotic_field`), and
/// the refusal's throw world leaves with it.
///
/// # Safety
/// `p` is NULL or a live heap pointer; `name` is a live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_check_field_writable(p: *const u8, name: *const u8) {
    unsafe { __torajs_obj_check_field_writable_impl(p, name) }
}

/// Default resolution of a class prologue cell's exit release (r503):
/// the `__class_<C>` / `__proto_<C>` bindings' end-of-main drop is
/// the generic any release. The link judgment NOPs the site under the
/// register call's guard — a cell the registry never held and no
/// reader can reach is not observable after exit — so a program whose
/// classes stay private stops rooting the any-world drop from here.
///
/// # Safety
/// `v` is a live AnyValue the caller owns one reference of.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_class_cell_release(v: u64) {
    unsafe { __torajs_anyv_rc_dec(v) }
}

/// Default resolution of the exotic-join seam — the any-world join
/// walk in torajs-arr / torajs-anyvalue.
///
/// # Safety
/// `arr` is a live array heap block; `sep` a live Str (unused by
/// the locale walk).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_join_exotic(
    arr: *const u8,
    sep: *const u8,
    kind: u64,
    locale: i64,
) -> *mut u8 {
    unsafe { __torajs_arr_join_exotic_impl(arr, sep, kind, locale) }
}

/// Default resolution of the species-guard slow seam — the props-bag
/// `constructor` classification in torajs-arr.
///
/// # Safety
/// `arr` is a live array heap block whose props slot is non-NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_species_guard_slow(arr: *const u8) -> i64 {
    unsafe { __torajs_arr_species_guard_props(arr) }
}

/// Default resolution of the scalar-array drop's props leg.
///
/// # Safety
/// `arr` is a live array heap block reaching rc=0 in the caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_drop_props_slow(arr: *mut core::ffi::c_void) {
    unsafe { __torajs_arrprops_drop_entry(arr) }
}

/// Default resolution of the scalar-array drop's subclass leg.
///
/// # Safety
/// `arr` is a live `FLAG_SUBCLASSED` array heap block reaching rc=0.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_drop_subclass_slow(arr: *mut core::ffi::c_void) {
    unsafe { __torajs_subclass_drop_entry(arr) }
}

/// Default resolution of the closure env-drop's props leg (A5): the
/// bag at +24 is a dynobj, released through the universal drop.
///
/// # Safety
/// `cell` is a live closure cell reaching rc=0 whose props slot is
/// non-NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_closure_drop_props_slow(cell: *mut core::ffi::c_void) {
    unsafe {
        let props = *(cell.cast::<u8>().add(24) as *const *mut core::ffi::c_void);
        __torajs_value_drop_heap(props)
    }
}

/// Default resolution of a struct instance's expando-bag release
/// (r502, the A5 shape on `Tag::Obj`): the bag at +24 is a dynobj,
/// released through the universal drop.
///
/// # Safety
/// `cell` is a live struct cell reaching rc=0 whose props slot is
/// non-NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_props_drop_slow(cell: *mut core::ffi::c_void) {
    unsafe {
        let props = *(cell.cast::<u8>().add(24) as *const *mut core::ffi::c_void);
        __torajs_value_drop_heap(props)
    }
}

/// Default resolution of a fields-all-scalar struct instance's
/// cycle-root buffering (r502): its rc went nonzero while it carries
/// a bag, the only place a cycle through it can run.
///
/// # Safety
/// `cell` is a live struct cell whose props slot is non-NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_buffer_slow(cell: *mut core::ffi::c_void) {
    unsafe { __torajs_cycle_buffer(cell) }
}

/// Default resolution of the uncaught reporter's Error-instance
/// rendering (A6): `name: message` through the prototype-chain name
/// resolver in torajs-throw / torajs-anyvalue.
///
/// # Safety
/// `p` is a live `Tag::Obj` cell carrying FLAG_ERROR.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_uncaught_error_render_slow(p: *const u8) {
    unsafe { __torajs_uncaught_error_render_impl(p) }
}
