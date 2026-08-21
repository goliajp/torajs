//! Ctor-static dispatch arms (RFC 20260720-ctor-static-reflection
//! 刀 1) — the Date / String / Object.hasOwn family appended behind
//! the RFC-20260719 namespaces. Split from `ns_static.rs` under the
//! 500-line file rule; the parent's `dispatch` match delegates here.

use core::ffi::c_void;

use crate::nanbox::{VALUE_UNDEFINED, box_double, box_void_ptr, is_null, is_undefined};

use super::ns_static::{arg_at, arg_num};
use super::ns_static_table::{
    __torajs_bigint_as_int_n, __torajs_bigint_as_uint_n, __torajs_bigint_drop_rc,
    __torajs_date_parse_iso, __torajs_date_utc_components, __torajs_map_group_by,
    __torajs_object_group_by, __torajs_str_from_char_code_f64, __torajs_str_from_code_point_f64,
    __torajs_throw_check, __torajs_throw_range_error, __torajs_throw_type_error,
};

/// §21.4.3.2 — ToString(arg0) into the ISO parse kernel (NaN on
/// failure). The coercion temp stays ours to release.
pub(super) unsafe fn date_parse(argv: *const u64, argc: i64) -> u64 {
    unsafe {
        let s = crate::nanbox_ffi::__torajs_anyv_to_str(arg_at(argv, argc, 0));
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        let n = __torajs_date_parse_iso(s as *const c_void);
        crate::__torajs_str_drop(s as *mut c_void);
        box_double(n)
    }
}

/// §21.4.3.4 — ToNumber every PRESENT component in source order;
/// absent trailing args take the spec defaults (year NaN — MakeTime
/// propagates it to a NaN time value — month 0, day 1, rest 0).
pub(super) unsafe fn date_utc(argv: *const u64, argc: i64) -> u64 {
    let mut c: [f64; 7] = [f64::NAN, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0];
    for (i, slot) in c.iter_mut().enumerate() {
        if (i as i64) < argc {
            let Ok(x) = (unsafe { arg_num(argv, argc, i as i64) }) else {
                return VALUE_UNDEFINED;
            };
            *slot = x;
        }
    }
    box_double(unsafe { __torajs_date_utc_components(c[0], c[1], c[2], c[3], c[4], c[5], c[6]) })
}

/// §22.1.2.1/.2 — per-code mint + pairwise concat, the same chain
/// the typed lowering builds (`ssa_lower_call_string_from_char_code`).
/// fromCharCode's kernel truncates ToUint16 itself (never throws);
/// fromCodePoint rejects non-integral codes here (the kernel's i64
/// ABI can't see the fraction) and range-errors inside the kernel —
/// checked per arg before the concat consumes the piece.
pub(super) unsafe fn str_from_codes(code_point: bool, argv: *const u64, argc: i64) -> u64 {
    unsafe {
        let release = |p: *mut u8| {
            if !p.is_null() {
                crate::__torajs_str_drop(p as *mut c_void);
            }
        };
        let mut acc: *mut u8 = core::ptr::null_mut();
        for i in 0..argc {
            let Ok(x) = arg_num(argv, argc, i) else {
                release(acc);
                return VALUE_UNDEFINED;
            };
            let piece = if code_point {
                // The f64 kernel makes §22.1.2.2 step 2.b's
                // non-integral test itself, so the `-1` poison value
                // this used to pass in its place is gone — one answer
                // to "what is a valid code point", shared with the
                // direct-call lowering.
                let p = __torajs_str_from_code_point_f64(x);
                if __torajs_throw_check() != 0 {
                    release(p);
                    release(acc);
                    return VALUE_UNDEFINED;
                }
                p
            } else {
                __torajs_str_from_char_code_f64(x)
            };
            acc = if acc.is_null() {
                piece
            } else {
                let joined =
                    crate::__torajs_str_concat(acc as *const u8, piece as *const u8) as *mut u8;
                release(acc);
                release(piece);
                joined
            };
        }
        if acc.is_null() {
            acc = crate::__torajs_str_alloc_pooled(0);
        }
        acc as u64
    }
}

/// The ns-static namespace name a builtin proto tag keys —
/// `torajs-rc builtin_proto` order; families with no table statics
/// answer through `ns_static_id`'s natural miss.
fn ctor_ns_name(proto_tag: i64) -> &'static str {
    match proto_tag {
        0 => "Number",
        1 => "Object",
        2 => "Array",
        3 => "String",
        4 => "Boolean",
        5 => "Symbol",
        6 => "BigInt",
        7 => "RegExp",
        8 => "Date",
        9 => "Error",
        10 => "Promise",
        11 => "Map",
        12 => "Set",
        13 => "Function",
        _ => "",
    }
}

/// gOPD's ctor-static probe (RFC 20260720 刀 2) — when `cell` is an
/// interned builtin ctor cell and `key` names one of that ctor's
/// table statics, answers the interned ns-static value cell (the
/// SAME identity a value read mints, so
/// `gOPD(Date, "parse").value === Date.parse` holds). NULL on every
/// miss; torajs-meta's Closure descriptor arm consumes it.
///
/// # Safety
/// `cell` is null or a live `Tag::Closure` cell; `key` is null or a
/// live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_ctor_static_value_cell(
    cell: *const c_void,
    key: *const c_void,
) -> *mut u8 {
    unsafe { ctor_static_cell(cell, key) }.unwrap_or(core::ptr::null_mut())
}

/// Rust-side twin of [`__torajs_ctor_static_value_cell`] — the
/// interned ns-static cell a builtin ctor cell owns under `key`
/// (immortal; callers hand it out without a ledger). `None` on
/// every miss.
///
/// # Safety
/// Same contract as the extern face.
pub(crate) unsafe fn ctor_static_cell(cell: *const c_void, key: *const c_void) -> Option<*mut u8> {
    let id = unsafe { ctor_static_table_id(cell, key) }?;
    Some(super::ns_static::ns_static_cell(id))
}

/// The ns-static table id a builtin ctor cell owns under `key` —
/// `None` on a non-ctor cell, a non-table key, or a delete-
/// tombstoned id (readers treat a tombstoned static as absent; a
/// defineProperty restore lands in the expando, which every reader
/// probes first).
///
/// # Safety
/// `cell` is null or a live heap cell; `key` is null or a live Str
/// cell.
pub(crate) unsafe fn ctor_static_table_id(cell: *const c_void, key: *const c_void) -> Option<i64> {
    let tag = super::ctor::ctor_tag_of_cell(cell)?;
    let name = unsafe { key_utf8(key) }?;
    let id = torajs_rc::ns_static_id(ctor_ns_name(tag), name);
    if id == torajs_rc::NS_STATIC_UNKNOWN || torajs_rc::ns_static_is_deleted(id) {
        return None;
    }
    Some(id)
}

/// The key Str's UTF-8 view — `None` on NULL / non-UTF-8 payload.
///
/// # Safety
/// `key` is NULL or a live Str cell; the view borrows its payload.
pub(super) unsafe fn key_utf8<'a>(key: *const c_void) -> Option<&'a str> {
    if key.is_null() {
        return None;
    }
    unsafe {
        let len = (key.cast::<u8>().add(super::STR_LEN_OFF) as *const u32).read() as usize;
        let bytes = core::slice::from_raw_parts(key.cast::<u8>().add(super::STR_DATA_OFF), len);
        core::str::from_utf8(bytes).ok()
    }
}

/// gOPD's ctor `prototype` probe (RFC 20260721 刀 1) — when `cell`
/// is an interned builtin ctor cell and `key` is `"prototype"`,
/// answers the builtin `<Ctor>.prototype` singleton (owned +1; the
/// descriptor takes the reference). §20.x.2.x: the property itself
/// is `{ writable: false, enumerable: false, configurable: false }`
/// — torajs-meta's arm carries the attrs. NULL on every miss.
///
/// # Safety
/// `cell` is null or a live `Tag::Closure` cell; `key` is null or a
/// live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_ctor_prototype_cell(
    cell: *const c_void,
    key: *const c_void,
) -> *mut u8 {
    if key.is_null() || !unsafe { crate::prop_has::key_is(key, b"prototype") } {
        return core::ptr::null_mut();
    }
    let Some(tag) = super::ctor::ctor_tag_of_cell(cell) else {
        return core::ptr::null_mut();
    };
    let proto = unsafe { torajs_rc::builtin_proto::__torajs_get_builtin_prototype(tag) };
    if proto.is_null() {
        return core::ptr::null_mut();
    }
    // Registry borrow → owned answer.
    crate::payload_rc_inc(4, proto as i64);
    proto as *mut u8
}

/// gOPD's Number data-constant probe (RFC 20260721 刀 1) — §21.1.2
/// value properties on the `Number` ctor, all `{ writable: false,
/// enumerable: false, configurable: false }`. Values mirror the SSA
/// const-fold table (`ssa_lower_member_builtin_namespace.rs
/// lower_number`) — MAX_SAFE_INTEGER / MIN_SAFE_INTEGER are I64
/// there, the rest F64. Writes the (slot-tag, payload) pair and
/// answers 1 on hit, 0 on every miss.
///
/// # Safety
/// `cell` is null or a live `Tag::Closure` cell; `key` is null or a
/// live Str cell; `out_tag` / `out_val` are valid writes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_ctor_number_constant(
    cell: *const c_void,
    key: *const c_void,
    out_tag: *mut u64,
    out_val: *mut u64,
) -> i64 {
    if super::ctor::ctor_tag_of_cell(cell) != Some(0) {
        return 0;
    }
    let Some(name) = (unsafe { key_utf8(key) }) else {
        return 0;
    };
    let Some((tag, val)) = number_constant(name) else {
        return 0;
    };
    unsafe {
        out_tag.write(tag);
        out_val.write(val);
    }
    1
}

/// §21.1.2 Number data constants as `(slot-tag, payload)` immediates
/// — values mirror the SSA const-fold table (see
/// [`__torajs_ctor_number_constant`]).
fn number_constant(name: &str) -> Option<(u64, u64)> {
    Some(match name {
        "NaN" => (3u64, f64::NAN.to_bits()),
        "POSITIVE_INFINITY" => (3, f64::INFINITY.to_bits()),
        "NEGATIVE_INFINITY" => (3, f64::NEG_INFINITY.to_bits()),
        "EPSILON" => (3, f64::EPSILON.to_bits()),
        "MAX_VALUE" => (3, f64::MAX.to_bits()),
        "MIN_VALUE" => (3, 5e-324f64.to_bits()),
        "MAX_SAFE_INTEGER" => (2, 9007199254740991u64),
        "MIN_SAFE_INTEGER" => (2, (-9007199254740991i64) as u64),
        _ => return None,
    })
}

/// Own-property read probe over a builtin ctor cell (RFC 20260721
/// 刀 3) — the read/HasProperty twin of the gOPD arms: table statics
/// answer `(4, interned ns-static cell)`, `prototype` answers
/// `(4, builtin prototype singleton)`, Number's data constants
/// answer their immediate pair. BORROW-shaped: every cell answer is
/// immortal / registry-held, so the member-get pair protocol boxes
/// without an inc. `None` when `cell` is not an interned ctor cell
/// or `key` misses its surface.
///
/// # Safety
/// `cell` is null or a live heap cell; `key` is null or a live Str
/// cell.
pub(crate) unsafe fn ctor_own_read_cell(
    cell: *const c_void,
    key: *const c_void,
) -> Option<(u64, u64)> {
    let tag = super::ctor::ctor_tag_of_cell(cell)?;
    let name = unsafe { key_utf8(key) }?;
    if name == "prototype" {
        let proto = unsafe { torajs_rc::builtin_proto::__torajs_get_builtin_prototype(tag) };
        if proto.is_null() {
            return None;
        }
        return Some((4, proto as u64));
    }
    if tag == 0
        && let Some(pair) = number_constant(name)
    {
        return Some(pair);
    }
    // §6.1.5.1 — the Symbol ctor's well-known data statics (RFC
    // 20260722 刀 2; immortal singleton, ledger-free hand-out).
    if tag == 5
        && let Some(sym) = unsafe { super::symbol_static::ctor_wellknown_symbol(cell, key) }
    {
        return Some((4, sym as u64));
    }
    let id = torajs_rc::ns_static_id(ctor_ns_name(tag), name);
    if id != torajs_rc::NS_STATIC_UNKNOWN && !torajs_rc::ns_static_is_deleted(id) {
        return Some((4, super::ns_static::ns_static_cell(id) as u64));
    }
    None
}

/// §21.2.2.1/.2 BigInt.asIntN / asUintN — ToIndex(bits) per §7.1.22
/// (NaN → 0, negative / past 2^53-1 → RangeError, messages match
/// bun/JSC), ToBigInt(value) per §7.1.13, then the 刀-5a fixed-width
/// kernel. Every temp is owned here and released before boxing.
pub(super) unsafe fn bigint_as_n(signed: bool, argv: *const u64, argc: i64) -> u64 {
    unsafe {
        let Ok(x) = arg_num(argv, argc, 0) else {
            return VALUE_UNDEFINED;
        };
        let x = if x.is_nan() { 0.0 } else { x.trunc() };
        if x < 0.0 {
            __torajs_throw_range_error(c"number of bits cannot be negative".as_ptr());
            return VALUE_UNDEFINED;
        }
        if x > 9007199254740991.0 {
            __torajs_throw_range_error(c"number of bits larger than (2 ** 53) - 1".as_ptr());
            return VALUE_UNDEFINED;
        }
        let bits = x as i64;
        let Some(b) = crate::to_bigint::any_to_bigint(arg_at(argv, argc, 1)) else {
            return VALUE_UNDEFINED;
        };
        let r = if signed {
            __torajs_bigint_as_int_n(bits, b as *const c_void)
        } else {
            __torajs_bigint_as_uint_n(bits, b as *const c_void)
        };
        __torajs_bigint_drop_rc(b as *mut c_void);
        if __torajs_throw_check() != 0 {
            __torajs_bigint_drop_rc(r as *mut c_void);
            return VALUE_UNDEFINED;
        }
        box_void_ptr(r as *mut c_void)
    }
}

/// §20.1.2.10 Object.groupBy / §24.2.2.4 Map.groupBy — neither
/// algorithm has a |this| step, so the detached call IS the real
/// semantics: (items, cb) borrowed into the torajs-meta kernels,
/// fresh owned result (null-proto dynobj / Map cell) out, throw
/// paths on the pending-throw channel.
pub(super) unsafe fn group_by(map: bool, argv: *const u64, argc: i64) -> u64 {
    unsafe {
        let items = arg_at(argv, argc, 0);
        let cb = arg_at(argv, argc, 1);
        if map {
            __torajs_map_group_by(items, cb)
        } else {
            __torajs_object_group_by(items, cb)
        }
    }
}

unsafe extern "C" {
    /// torajs-meta — the §20.1.2.8 descriptor kernel (owned fresh
    /// descriptor dynobj / undefined; nullish receiver throws inside).
    fn __torajs_anyv_get_property_descriptor(obj_any: u64, key: *const c_void) -> u64;
}

/// §20.1.2.8 Object.getOwnPropertyDescriptor as a detached call —
/// ToString(P) into the meta descriptor kernel (the same face every
/// gOPD lowering consumes). The coerced key temp stays ours to
/// release.
pub(super) unsafe fn gopd_static(argv: *const u64, argc: i64) -> u64 {
    unsafe {
        let o = arg_at(argv, argc, 0);
        let key = crate::nanbox_ffi::__torajs_anyv_to_str(arg_at(argv, argc, 1));
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        let d = __torajs_anyv_get_property_descriptor(o, key as *const c_void);
        crate::__torajs_str_drop(key as *mut c_void);
        d
    }
}
/// §20.1.2.{9,2,4,3} getOwnPropertyDescriptors / create /
/// defineProperty / defineProperties as detached calls — the define
/// family's kernels are dynobj-slot shaped (a resize relocates the
/// receiver and a receiver-less cell call has no writeback target),
/// so the arm stays a loud reject until the any-tier define kernel
/// lands (RFC 20260721 records the face). The cells exist for the
/// reflection surface.
pub(super) unsafe fn define_face_reject() -> u64 {
    unsafe {
        __torajs_throw_type_error(
            c"builtin namespace static called through a detached value is not supported for the define family"
                .as_ptr(),
        )
    };
    VALUE_UNDEFINED
}

/// §23.1.2.1 Array.from through the value cell (RFC
/// 20260808-construct-channel B6 刀 1+2) — the cell is recv-first
/// (`this_aware_id`), so argv[0] is the call-site thisArg (undefined
/// on a bare detached call). A constructor `this` takes the step-4
/// Construct split inside the kernel; anything else takes
/// ArrayCreate. items / mapfn / thisArg follow in argv[1..].
pub(super) unsafe fn array_from_value(argv: *const u64, argc: i64) -> u64 {
    unsafe {
        crate::array_from::array_from_this(
            arg_at(argv, argc, 0),
            arg_at(argv, argc, 1),
            arg_at(argv, argc, 2),
            arg_at(argv, argc, 3),
        )
    }
}

unsafe extern "C" {
    /// torajs-promise — the proposal-array-from-async §2.1.1
    /// sync-source kernels the direct-call lowering bakes (fresh
    /// rc-1 REPR_HEAP promise cell out).
    fn __torajs_array_from_async_dyn(v: u64) -> *mut core::ffi::c_void;
    fn __torajs_array_from_async_map_dyn(v: u64, cb: u64) -> *mut core::ffi::c_void;
    /// torajs-promise — the constructor-`this` face (B6 刀 4): a
    /// non-constructor thisArg falls through to the plain kernel
    /// inside.
    fn __torajs_array_from_async_this_dyn(this_c: u64, v: u64) -> *mut core::ffi::c_void;
}

/// proposal-array-from-async §2.1.1 Array.fromAsync through the
/// value cell — the cell is recv-first (`this_aware_id`, B6 刀 2),
/// so argv[0] is the thisArg and items / mapfn follow. A
/// constructor thisArg takes the §3.j / §3.k.iv Construct face
/// (rotation 346, B6 刀 4); the mapped form still reads the thisArg
/// past (recorded MVP boundary — the this-constructor t262 family
/// carries no mapfn).
pub(super) unsafe fn from_async_dyn(argv: *const u64, argc: i64) -> u64 {
    unsafe {
        let this_c = arg_at(argv, argc, 0);
        let items = arg_at(argv, argc, 1);
        let mapfn = arg_at(argv, argc, 2);
        let p = if is_undefined(mapfn) {
            __torajs_array_from_async_this_dyn(this_c, items)
        } else {
            __torajs_array_from_async_map_dyn(items, mapfn)
        };
        box_void_ptr(p)
    }
}

/// §20.1.2.11 Object.hasOwn — step 1 ToObject throws on a nullish
/// target; every other receiver routes through the same prop_has
/// probe the `hasOwnProperty` dispatcher arm uses (single source).
pub(super) unsafe fn object_has_own(argv: *const u64, argc: i64) -> u64 {
    unsafe {
        let o = arg_at(argv, argc, 0);
        if is_undefined(o) || is_null(o) {
            __torajs_throw_type_error(c"Object.hasOwn called on null or undefined".as_ptr());
            return VALUE_UNDEFINED;
        }
        let (key_argv, key_argc) = if argc >= 2 {
            (argv.add(1), argc - 1)
        } else {
            (core::ptr::null(), 0)
        };
        crate::method_call_object_proto::own_prop_probe(
            o,
            torajs_rc::ANY_METHOD_HAS_OWN_PROPERTY,
            key_argv,
            key_argc,
        )
    }
}
