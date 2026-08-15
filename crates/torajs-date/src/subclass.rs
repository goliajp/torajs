//! Date-subclass instance allocation (rotation 373 — extends RFC
//! 20260730-exotic-backed-class-instance blade 2 to Date, the
//! torajs-collections / torajs-weak `subclass_alloc` twin).
//!
//! `class C extends Date` mints a REAL Date cell whose [[DateValue]]
//! is the current wall clock — the no-argument `new Date()` answer —
//! so a bare `super()` is a no-op against the mint. The
//! one-argument `super(v)` (§21.4.2.1 step 4) overwrites the mint's
//! ms via `__torajs_date_set_ms_from` (the torajs-anyvalue super
//! kernel resolves `v` with the full ToPrimitive/parse ladder and
//! hands back a scratch Date). The whole getter/setter/format
//! surface rides the existing arms because the instance IS a Date.
//! Class identity rides blade 0 (`FLAG_SUBCLASSED` + torajs-meta
//! side table), scrubbed by `__torajs_date_drop`.

use core::ffi::c_void;

use crate::Date;
use crate::api::__torajs_date_now;

/// `torajs_rc::FLAG_SUBCLASSED` mirror (flags bit 0, RFC 20260730
/// blade 0 — same mirror the collections twin carries).
pub(crate) const FLAG_SUBCLASSED: u16 = 1;

/// `torajs_rc::AnySlotTag::Heap` mirror.
const ANY_HEAP: i64 = 4;

unsafe extern "C" {
    /// torajs-meta — record the fresh instance's class identity
    /// (blade 0). Takes no reference on the proto cell.
    fn __torajs_subclass_register(cell: *mut c_void, class_tag: i64, proto_cell: u64);
    /// torajs-meta classmeta — the class's registered `__proto_<C>`
    /// AnyValue immediate (0 when unregistered).
    fn __torajs_proto_cell_raw(tag: i64) -> u64;
    /// torajs-anyvalue — NaN-box encode.
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
}

/// Mint a Date-subclass instance ([[DateValue]] = current wall
/// clock) and answer it boxed — subclass instances live in the any
/// world.
///
/// # Safety
/// `class_tag` is the class's registered tag.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_subclass_alloc(class_tag: i64) -> u64 {
    unsafe {
        let p = __torajs_date_now();
        (*(p as *mut Date)).header.flags |= FLAG_SUBCLASSED;
        let proto_cell = __torajs_proto_cell_raw(class_tag);
        __torajs_subclass_register(p, class_tag, proto_cell);
        __torajs_anyv_box_from_pair(ANY_HEAP, p as i64)
    }
}

/// Copy `src`'s [[DateValue]] into `dst` — the invalid-date sentinel
/// rides verbatim, so a refused coercion lands as Invalid Date
/// exactly like the plain one-argument ctor.
///
/// # Safety
/// Both pointers are live Date cells.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_set_ms_from(dst: *mut c_void, src: *const c_void) {
    if dst.is_null() || src.is_null() {
        return;
    }
    unsafe {
        (*(dst as *mut Date)).ms = (*(src as *const Date)).ms;
    }
}

/// `torajs_arr` header mirror — the length slot every Arr cell
/// carries at +8 (the torajs-promise combinator carries the same
/// mirror; torajs-date takes no crate dep on torajs-arr).
const ARR_LEN_OFF: usize = 8;

unsafe extern "C" {
    /// torajs-anyvalue — NaN-box payload decode.
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    /// torajs-anyvalue — ToNumber over a boxed any (§7.1.4).
    fn __torajs_anyv_to_number(v: u64) -> f64;
    /// torajs-arr — borrow-read one Any slot boxed (OOB answers
    /// boxed undefined; not consulted past `len` here — a MISSING
    /// component takes its §21.4.2.1 step-6 default, while a present
    /// `undefined` must run ToNumber to NaN, so the argc question is
    /// answered by the length slot, never by the OOB posture).
    fn __torajs_arr_get_any_boxed(arr: *const c_void, i: u64) -> u64;
    /// torajs-rc — the fresh-owned-answer inc (see below).
    fn __torajs_rc_inc(p: *mut c_void);
}

/// `super(y, m, ...)` with 2+ arguments inside a Date-subclass ctor —
/// the `new Date(y, m, d, h, mi, s, ms)` components form
/// (§21.4.2.1 step 6): LOCAL-time interpretation, MakeFullYear on the
/// year, day defaulting to 1 and the time components to 0 when the
/// argument list stops short. The synthesized rest-param default ctor
/// hands its packed rest array; each present slot runs ToNumber
/// (§7.1.4 — a boxed string parses, an undefined answers NaN and the
/// clip lands Invalid Date, exactly the plain ctor's account).
///
/// # Safety
/// `this_av` is the factory's freshly minted subclass instance boxed
/// ANY_HEAP (or any non-cell box, answered back unchanged);
/// `comps_av` is a live borrowed boxed Arr<Any>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_subclass_super_components(
    this_av: u64,
    comps_av: u64,
) -> u64 {
    unsafe {
        let this_p = __torajs_anyv_unbox_value(this_av) as *mut c_void;
        let comps_p = __torajs_anyv_unbox_value(comps_av) as *const c_void;
        if this_p.is_null() || comps_p.is_null() {
            return this_av;
        }
        let len = *((comps_p as *const u8).add(ARR_LEN_OFF) as *const u64);
        let comp = |i: u64, default: f64| -> f64 {
            if i < len {
                __torajs_anyv_to_number(__torajs_arr_get_any_boxed(comps_p, i))
            } else {
                default
            }
        };
        let ms = crate::make_time::make_ms_local(
            crate::make_time::make_full_year(comp(0, f64::NAN)),
            comp(1, 0.0),
            comp(2, 1.0),
            comp(3, 0.0),
            comp(4, 0.0),
            comp(5, 0.0),
            comp(6, 0.0),
        );
        (*(this_p as *mut Date)).ms = ms;
        // Fresh owned answer — the `super(...)` statement position
        // releases the discarded any value (the arr elems kernel's
        // account).
        __torajs_rc_inc(this_p);
    }
    this_av
}
