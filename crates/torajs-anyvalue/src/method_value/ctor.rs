//! Constructor-value family — the per-builtin-family interned
//! `constructor` cells (rotation 131) plus the L3b ④ value faces:
//! the bare namespace ident read (`Object` as a VALUE) and the
//! `.constructor` reify probe. Split from `method_value.rs`
//! (file-size HARD RULE); parent-private layout consts and the
//! throwing entries resolve through `super::`.

use core::ffi::c_void;
use core::sync::atomic::{AtomicU64, Ordering};

use torajs_rc::{FLAG_STATIC_LITERAL, Tag};

use crate::nanbox::AnyValue;

use super::{
    CELL_SIZE, CLOSURE_BOXED_ENTRY_OFF, CLOSURE_CAP_BASE_OFF, CLOSURE_DROP_FN_OFF,
    CLOSURE_FN_ADDR_OFF, CLOSURE_PROPS_OFF, bare_entry, native_entry,
};

/// Per-proto-tag interned `constructor` cells (rotation 131 —
/// the gOPD 15.2.3.3-4 constructor family). `<Ctor>.prototype
/// .constructor` must answer ONE identity per builtin so
/// `desc.value === Date.prototype.constructor` holds; the cell is
/// the same immortal closure shape as [`builtin_method_cell`]
/// (calling it is the bare-receiver TypeError — constructing
/// through a first-class ctor value is a recorded follow-up).
static CTOR_CELLS: [AtomicU64; torajs_rc::builtin_proto::NUM_BUILTIN_PROTOS] =
    [const { AtomicU64::new(0) }; torajs_rc::builtin_proto::NUM_BUILTIN_PROTOS];

/// The interned `constructor` cell for a builtin proto tag —
/// lazily allocated, immortal.
pub(crate) fn builtin_ctor_cell(proto_tag: i64) -> *mut u8 {
    let slot = &CTOR_CELLS[proto_tag as usize];
    let p = slot.load(Ordering::Relaxed);
    if p != 0 {
        return p as *mut u8;
    }
    // SAFETY: fresh CELL_SIZE allocation, fully initialized below —
    // same layout as builtin_method_cell.
    unsafe {
        let layout = core::alloc::Layout::from_size_align(CELL_SIZE, 8).unwrap();
        let cell = std::alloc::alloc_zeroed(layout);
        *(cell as *mut u32) = 1;
        *(cell.add(4) as *mut u16) = Tag::Closure as u16;
        *(cell.add(6) as *mut u16) = FLAG_STATIC_LITERAL;
        *(cell.add(CLOSURE_FN_ADDR_OFF) as *mut u64) = native_entry as *const () as u64;
        *(cell.add(CLOSURE_DROP_FN_OFF) as *mut u64) = 0;
        *(cell.add(CLOSURE_PROPS_OFF) as *mut u64) = 0;
        *(cell.add(CLOSURE_BOXED_ENTRY_OFF) as *mut u64) = ctor_call_entry as *const () as u64;
        *(cell.add(CLOSURE_CAP_BASE_OFF) as *mut u64) = torajs_rc::ANY_METHOD_UNKNOWN as u64;
        slot.store(cell as u64, Ordering::Relaxed);
        cell
    }
}

/// The interned ctor cell WITHOUT minting — the promise patch-consult
/// probe asks "has anyone stored on it", and a cell nobody ever
/// minted cannot have been written through, so a null peek answers
/// "unpatched" without paying the allocation.
pub(crate) fn ctor_cell_peek(proto_tag: i64) -> *mut u8 {
    CTOR_CELLS[proto_tag as usize].load(Ordering::Relaxed) as *mut u8
}

/// The interned empty-string cell of the `String()` zero-arg call —
/// lazily minted, immortal (strings compare by value, so one
/// identity serves every call).
static EMPTY_STR_CELL: AtomicU64 = AtomicU64::new(0);

fn empty_str_cell() -> *mut u8 {
    let p = EMPTY_STR_CELL.load(Ordering::Relaxed);
    if p != 0 {
        return p as *mut u8;
    }
    let cell = super::mint_immortal_str(b"");
    EMPTY_STR_CELL.store(cell as u64, Ordering::Relaxed);
    cell
}

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_check() -> i64;
    /// torajs-arr — the §23.1.1.1 length-form mint (its own
    /// RangeError gate on a non-index length).
    fn __torajs_arr_alloc_any_filled_f64(len: f64) -> *mut u8;
    fn __torajs_arr_alloc_any(cap: u64) -> *mut u8;
    /// torajs-date — fresh now-valued Date cell (rc 1) + the
    /// §21.4.4.41 rendering (fresh owned Str), and the ms-form mint.
    fn __torajs_date_now() -> *mut c_void;
    fn __torajs_date_to_string(d_ptr: *const c_void) -> *mut u8;
    fn __torajs_date_from_ms(ms: f64) -> *mut c_void;
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// torajs-wrapper — the §21.1.1.2 / §22.1.1.2 / §20.3.1.2
    /// [[Construct]] mints (rc 1; String's transfers +1 on the cell).
    fn __torajs_number_wrapper_new(val: f64) -> *mut u8;
    fn __torajs_string_wrapper_new(cell: *mut u8) -> *mut u8;
    fn __torajs_boolean_wrapper_new(val: u8) -> *mut u8;
    /// torajs-collections — fresh empty Map / Set cells (rc 1).
    fn __torajs_map_create() -> *mut c_void;
    fn __torajs_set_create() -> *mut c_void;
}

/// [[Call]] of a builtin-constructor value — `const N = Number; N(42)`,
/// `Number.call(null, x)`, and calls through a `.bind` mint all land
/// here (the boxed entry of every interned ctor cell). Families whose
/// ctor-as-function semantics are pure conversions run them: Number =
/// ToNumber (§21.1.1.1), String = the display coercion (§22.1.1.1 —
/// symbols stringify here), Boolean = ToBoolean, Object = ToObject
/// with the fresh-object nullish arm, Array = the §23.1.1.1
/// length-form, Date = the current-time string (§21.4.2 ignores its
/// arguments). Everything else keeps the loud catchable TypeError —
/// including Function (dynamic code has no runtime channel in tr; the
/// compile-time inline lane owns the constant forms).
unsafe extern "C" fn ctor_call_entry(env: *mut c_void, argv: *const u64, argc: i64) -> u64 {
    use crate::nanbox::{VALUE_UNDEFINED, box_bool, box_double, box_void_ptr};
    let Some(tag) = ctor_tag_of_cell(env as *const c_void) else {
        return unsafe { bare_entry(env, argv, argc) };
    };
    let arg0 = |i: i64| -> u64 {
        if i < argc {
            unsafe { argv.add(i as usize).read() }
        } else {
            VALUE_UNDEFINED
        }
    };
    unsafe {
        match tag {
            // Number — absent argument answers +0 (§21.1.1.1 step 1).
            0 => {
                if argc == 0 {
                    return box_double(0.0);
                }
                let n = crate::nanbox_ffi::__torajs_anyv_to_number(arg0(0));
                if __torajs_throw_check() != 0 {
                    return VALUE_UNDEFINED;
                }
                box_double(n)
            }
            // String — absent argument answers the empty string
            // (§22.1.1.1 step 2.a; coercing the absent-arg undefined
            // would answer "undefined" instead).
            3 => {
                if argc == 0 {
                    return box_void_ptr(empty_str_cell() as *mut c_void);
                }
                let s = crate::nanbox_ffi::__torajs_anyv_to_display_str(arg0(0));
                if __torajs_throw_check() != 0 {
                    return VALUE_UNDEFINED;
                }
                box_void_ptr(s)
            }
            4 => box_bool(argc > 0 && crate::nanbox_ffi::__torajs_anyv_to_bool(arg0(0))),
            1 => crate::to_object::__torajs_any_to_object(arg0(0)),
            // Array — the length-form and the empty call; the
            // elements form (§23.1.1.3) keeps a loud reject until a
            // caller shape demands it (no silent half-array).
            2 => {
                if argc == 0 {
                    return box_void_ptr(__torajs_arr_alloc_any(0) as *mut c_void);
                }
                let n0 = arg0(0);
                if argc > 1 || !(crate::nanbox::is_int32(n0) || crate::nanbox::is_double(n0)) {
                    __torajs_throw_type_error(
                        c"Array constructor elements form is not supported through a first-class ctor value".as_ptr(),
                    );
                    return VALUE_UNDEFINED;
                }
                let len = crate::nanbox_ffi::__torajs_anyv_to_number(arg0(0));
                let arr = __torajs_arr_alloc_any_filled_f64(len);
                if __torajs_throw_check() != 0 {
                    return VALUE_UNDEFINED;
                }
                box_void_ptr(arr as *mut c_void)
            }
            // Date as a function — §21.4.2: arguments ignored, the
            // answer is the current time rendered as a string.
            8 => {
                let d = __torajs_date_now();
                let s = __torajs_date_to_string(d);
                __torajs_value_drop_heap(d);
                box_void_ptr(s as *mut c_void)
            }
            _ => {
                __torajs_throw_type_error(
                    c"this constructor cannot be called as a function through a first-class value"
                        .as_ptr(),
                );
                VALUE_UNDEFINED
            }
        }
    }
}

/// L3b ④ — the boxed builtin-constructor value for the bare
/// namespace ident read (`Object` / `Array` / `Number` … as a
/// VALUE: `o.constructor === Object`). Immortal interned cell —
/// the box is a pure bit-encode over the static-flagged cell, rc
/// traffic no-ops. The compiler gates `proto_tag` to the builtin
/// family range.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_builtin_ctor_value(proto_tag: i64) -> u64 {
    crate::nanbox::box_void_ptr(builtin_ctor_cell(proto_tag) as *mut core::ffi::c_void)
}

/// `.constructor` through the reify surface (L3b ④) — the
/// receiver's builtin constructor per its family tag, the same
/// interned identity the prototype's own `constructor` entry and
/// the bare ident read answer. `None` for non-constructor keys and
/// for receivers whose constructor rides another channel (a struct
/// instance walks its class prototype chain; the own-face shadow
/// probes already ran in every member-get arm).
pub(crate) unsafe fn ctor_cell_for_recv(recv: AnyValue, key: *const c_void) -> Option<*mut u8> {
    if !unsafe { crate::prop_has::key_is(key, b"constructor") } {
        return None;
    }
    let tag = ctor_family_tag(recv)?;
    Some(builtin_ctor_cell(tag))
}

/// Reverse lookup: the proto tag whose interned ctor cell is `cell`,
/// `None` for every other pointer. Linear over ≤16 slots — gOPD /
/// reflection cold path only (RFC 20260720-ctor-static-reflection
/// 刀 2).
pub(crate) fn ctor_tag_of_cell(cell: *const c_void) -> Option<i64> {
    CTOR_CELLS
        .iter()
        .position(|slot| slot.load(Ordering::Relaxed) == cell as u64 && !cell.is_null())
        .map(|i| i as i64)
}

/// ES `name` / `length` of a builtin constructor (刀 3) — the
/// torajs-rc single-source table (the lowering's ctor-namespace
/// member fold reads the same rows).
pub(super) fn ctor_meta(proto_tag: i64) -> Option<(&'static str, u32)> {
    torajs_rc::builtin_proto::builtin_ctor_meta(proto_tag)
}

/// Per-proto-tag interned `.name` Str cells (刀 3) — same immortal
/// shape as the method-name intern table.
static CTOR_NAME_CELLS: [AtomicU64; torajs_rc::builtin_proto::NUM_BUILTIN_PROTOS] =
    [const { AtomicU64::new(0) }; torajs_rc::builtin_proto::NUM_BUILTIN_PROTOS];

/// The interned `.name` Str cell of a ctor cell — lazily minted,
/// immortal.
pub(super) fn ctor_name_cell(proto_tag: i64) -> Option<*mut u8> {
    let (name, _) = ctor_meta(proto_tag)?;
    let slot = &CTOR_NAME_CELLS[proto_tag as usize];
    let p = slot.load(Ordering::Relaxed);
    if p != 0 {
        return Some(p as *mut u8);
    }
    let cell = super::mint_immortal_str(name.as_bytes());
    slot.store(cell as u64, Ordering::Relaxed);
    Some(cell)
}

/// The builtin-prototype family tag of a receiver (`torajs-rc
/// builtin_proto` order: Number 0 / Object 1 / Array 2 / String 3 /
/// Boolean 4 / Symbol 5 / BigInt 6 / RegExp 7 / Date 8 / Error 9 /
/// Promise 10 / Map 11 / Set 12 / Function 13). Wrapper cells
/// answer their inner primitive's family. `None` for struct
/// instances (class chain) and null/undefined (TypeError upstream).
/// [[Construct]] of a builtin-constructor value — `new N(5)` through
/// a `const N: any = Number` binding, `new (Array.bind(null))(3)`,
/// `new globalThis.Array(3)`. The wrapper trio mints real wrapper
/// cells (§21.1.1.2 / §22.1.1.2 / §20.3.1.2 — construct differs from
/// call: `new String(sym)` runs plain ToString and throws where the
/// call form stringifies); Object / Array share their call-form
/// semantics per spec; Date covers the zero-arg (now) and one-number
/// (ms) mints; Map / Set mint fresh empties (the iterable-seed form
/// keeps a loud reject). Everything else keeps the loud TypeError.
pub(crate) unsafe fn ctor_construct(tag: i64, argv: *const u64, argc: i64) -> u64 {
    use crate::nanbox::{VALUE_UNDEFINED, box_void_ptr};
    let arg0 = || -> u64 {
        if argc > 0 {
            unsafe { argv.read() }
        } else {
            VALUE_UNDEFINED
        }
    };
    unsafe {
        match tag {
            0 => {
                let n = if argc == 0 {
                    0.0
                } else {
                    let x = crate::nanbox_ffi::__torajs_anyv_to_number(arg0());
                    if __torajs_throw_check() != 0 {
                        return VALUE_UNDEFINED;
                    }
                    x
                };
                box_void_ptr(__torajs_number_wrapper_new(n) as *mut c_void)
            }
            3 => {
                let cell = if argc == 0 {
                    empty_str_cell()
                } else {
                    let s = crate::nanbox_ffi::__torajs_anyv_to_str(arg0());
                    if __torajs_throw_check() != 0 {
                        return VALUE_UNDEFINED;
                    }
                    s as *mut u8
                };
                box_void_ptr(__torajs_string_wrapper_new(cell) as *mut c_void)
            }
            4 => {
                let b = argc > 0 && crate::nanbox_ffi::__torajs_anyv_to_bool(arg0());
                box_void_ptr(__torajs_boolean_wrapper_new(b as u8) as *mut c_void)
            }
            1 => crate::to_object::__torajs_any_to_object(arg0()),
            2 => {
                if argc == 0 {
                    return box_void_ptr(__torajs_arr_alloc_any(0) as *mut c_void);
                }
                let n0 = arg0();
                if argc > 1 || !(crate::nanbox::is_int32(n0) || crate::nanbox::is_double(n0)) {
                    __torajs_throw_type_error(
                        c"Array constructor elements form is not supported through a first-class ctor value".as_ptr(),
                    );
                    return VALUE_UNDEFINED;
                }
                let len = crate::nanbox_ffi::__torajs_anyv_to_number(n0);
                let arr = __torajs_arr_alloc_any_filled_f64(len);
                if __torajs_throw_check() != 0 {
                    return VALUE_UNDEFINED;
                }
                box_void_ptr(arr as *mut c_void)
            }
            8 => {
                if argc == 0 {
                    return box_void_ptr(__torajs_date_now());
                }
                let n0 = arg0();
                if argc > 1 || !(crate::nanbox::is_int32(n0) || crate::nanbox::is_double(n0)) {
                    __torajs_throw_type_error(
                        c"Date constructor component forms are not supported through a first-class ctor value".as_ptr(),
                    );
                    return VALUE_UNDEFINED;
                }
                box_void_ptr(__torajs_date_from_ms(
                    crate::nanbox_ffi::__torajs_anyv_to_number(n0),
                ))
            }
            11 | 12 if argc == 0 => box_void_ptr(if tag == 11 {
                __torajs_map_create()
            } else {
                __torajs_set_create()
            }),
            _ => {
                __torajs_throw_type_error(
                    c"this constructor cannot be constructed through a first-class value yet"
                        .as_ptr(),
                );
                VALUE_UNDEFINED
            }
        }
    }
}

fn ctor_family_tag(recv: AnyValue) -> Option<i64> {
    use crate::nanbox::{is_bool, is_cell, is_double, is_int32, is_short_str};
    if is_short_str(recv) {
        return Some(3);
    }
    if is_int32(recv) || is_double(recv) {
        return Some(0);
    }
    if is_bool(recv) {
        return Some(4);
    }
    if !is_cell(recv) {
        return None;
    }
    let ptr = crate::nanbox::as_void_ptr(recv);
    // SAFETY: is_cell guarantees a live heap header.
    let tag = unsafe { (ptr.cast::<u8>().add(4) as *const u16).read() };
    match tag {
        t if t == Tag::Str as u16 => Some(3),
        t if t == Tag::Arr as u16 => Some(2),
        t if t == Tag::DynObj as u16 => Some(1),
        // RFC 20260721 刀 4 — an async-form cell (FLAG_FN_ASYNC,
        // bit 7 Closure-private) reflects %AsyncFunction% (§27.7.1);
        // every other closure keeps the Function ctor.
        t if t == Tag::Closure as u16 => {
            let flags = unsafe { (ptr.cast::<u8>().add(6) as *const u16).read() };
            if flags & torajs_rc::FLAG_FN_ASYNC != 0 {
                Some(14)
            } else {
                Some(13)
            }
        }
        t if t == Tag::BigInt as u16 => Some(6),
        t if t == Tag::Symbol as u16 => Some(5),
        t if t == Tag::RegExp as u16 => Some(7),
        t if t == Tag::Date as u16 => Some(8),
        t if t == Tag::Map as u16 => Some(11),
        t if t == Tag::Set as u16 => Some(12),
        t if t == Tag::WeakMap as u16 => Some(16),
        t if t == Tag::WeakSet as u16 => Some(17),
        t if t == Tag::WeakRef as u16 => Some(18),
        t if t == Tag::Promise as u16 => Some(10),
        t if t == Tag::NumberWrapper as u16 => Some(0),
        t if t == Tag::StringWrapper as u16 => Some(3),
        t if t == Tag::BooleanWrapper as u16 => Some(4),
        _ => None,
    }
}
