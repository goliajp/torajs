//! `__torajs_dynobj_define_from_desc(obj_slot, key, desc)` — the
//! runtime-descriptor path for `Object.defineProperty`, split out of
//! `define.rs` (file-size hard limit; RFC 20260712-arr-exotic-define
//! chunk B pushed the shared file over 500). Reads the descriptor
//! fields off the `desc` dynobj at runtime (§6.2.6.5
//! ToPropertyDescriptor, accessor pairs included) and applies via
//! [`crate::define::define_apply`] — which also carries the Arr
//! receiver dispatch, so this path inherits it.

use core::ffi::c_void;

use crate::define::define_apply;
use crate::layout::{
    CELL_PROPS_OFF, DEFINE_FLAG_CONFIGURABLE, DEFINE_FLAG_ENUMERABLE, DEFINE_FLAG_WRITABLE,
    DEFINE_PRESENT_CONFIGURABLE, DEFINE_PRESENT_ENUMERABLE, DEFINE_PRESENT_VALUE,
    DEFINE_PRESENT_WRITABLE, TAG_ARR_HDR, TAG_CLOSURE_HDR, TAG_DYNOBJ, TAG_OBJ,
};
use crate::probe::{entries, probe};

mod face;
use face::try_define_accessor;

/// Wrapper-cell lazy expando slot — mirror of
/// `torajs-wrapper::WRAPPER_PROPS_OFF` (layout
/// `[header:8][value:8][props:8]`).
const WRAPPER_PROPS_OFF: usize = 16;

unsafe extern "C" {
    /// torajs-rc builtin-proto registry — the `<Ctor>.prototype`
    /// singleton dynobj for a builtin tag (13 = Function); NULL
    /// until first materialized.
    fn __torajs_get_builtin_prototype(tag: i64) -> *mut c_void;
}

unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_throw_type_error(msg: *const u8);
    /// torajs-throw — non-zero iff a throw is in flight.
    fn __torajs_throw_check() -> i64;
    /// torajs-anyvalue — §10.5.6 [[DefineOwnProperty]] on a Proxy.
    fn __torajs_proxy_define_own(recv: u64, key: *mut c_void, desc: *const c_void) -> i64;
    fn __torajs_value_drop_heap(child: *mut c_void);
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    /// torajs-anyvalue — owned payload decode: cell +1, ShortStr
    /// materialized at rc=1 (the materialization IS the stake),
    /// immediates raw.
    fn __torajs_anyv_unbox_value_owned(v: u64) -> i64;
    /// torajs-anyvalue — borrow-shaped cell read: cell → pointer
    /// bits, every immediate (ShortStr included) → 0, zero
    /// materialization.
    fn __torajs_anyv_cell_ptr(v: u64) -> i64;
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_anyv_to_bool(v: u64) -> bool;
    /// torajs-anyvalue — class-layout struct field read as a
    /// borrowed NaN-box (the §6.2.6.5 struct-desc lane).
    fn __torajs_struct_field_read_anyv(
        obj: *mut c_void,
        name: *const u8,
        name_len: u32,
        out_anyv: *mut u64,
    ) -> i64;
}

/// The two own-property stores a runtime descriptor object can live
/// in: a dynobj entry table (DynObj itself, or a Closure / Arr
/// expando) vs a static-layout class instance (`Tag::Obj` — an
/// ObjectLit descriptor the checker typed structurally). Pre-fix the
/// dispatcher's `_ => null` fallback read every struct-shaped desc
/// as an EMPTY descriptor: `Object.defineProperties(o, props)` with
/// `props.p = { value: 42, enumerable: true }` defined `p` as
/// `{ value: undefined, all-false }`.
#[derive(Clone, Copy)]
enum DescStore {
    /// `(own entry table, proto entry table)` — either may be NULL.
    /// §6.2.6.5 reads every field through [[Get]], so a miss on the
    /// descriptor's own store falls to its prototype's expando
    /// (test262's `Function.prototype.writable = true; defineProperty
    /// (obj, k, funObj)` inherits writable through a fresh closure
    /// with no expando of its own).
    Dyn(*const c_void, *const c_void),
    Struct(*const c_void),
}

/// Stack-allocated Str-shaped probe key. [`probe`] / `hash_str` /
/// `str_eq` only read `len` (offset 8) and the inline payload (offset
/// 16) — never the heap header — so a non-heap buffer with those two
/// fields suffices to look a property name up in a dynobj without
/// allocating (or interning) a real Str. Field names are short; a
/// 16-byte inline payload covers every descriptor key.
#[repr(C, align(8))]
struct FakeStrKey {
    _header: u64,
    len: u64,
    data: [u8; 16],
}

impl FakeStrKey {
    #[inline]
    fn new(name: &str) -> FakeStrKey {
        let mut k = FakeStrKey {
            _header: 0,
            len: name.len() as u64,
            data: [0u8; 16],
        };
        k.data[..name.len()].copy_from_slice(name.as_bytes());
        k
    }
}

/// Look a property name up in `desc` and return its NaN-box
/// `value_anyv` if present (the property's stored AnyValue).
///
/// # Safety
/// `desc` points at a live dynobj heap block.
#[inline]
unsafe fn desc_field(desc: DescStore, name: &str) -> Option<u64> {
    match desc {
        DescStore::Dyn(d, proto) => {
            let probe_key = FakeStrKey::new(name);
            if !d.is_null() {
                let pr = unsafe { probe(d, &probe_key as *const FakeStrKey as *const c_void) };
                if pr.found {
                    let ent = unsafe { entries(d) };
                    return Some(unsafe { (*ent.add(pr.entry as usize)).value_anyv });
                }
            }
            // Own miss — [[Get]] continues up the prototype's
            // expando table (NULL = chain end).
            if proto.is_null() {
                return None;
            }
            let pr = unsafe { probe(proto, &probe_key as *const FakeStrKey as *const c_void) };
            if !pr.found {
                return None;
            }
            let ent = unsafe { entries(proto) };
            Some(unsafe { (*ent.add(pr.entry as usize)).value_anyv })
        }
        DescStore::Struct(s) => {
            let mut anyv: u64 = 0;
            let hit = unsafe {
                __torajs_struct_field_read_anyv(
                    s as *mut c_void,
                    name.as_ptr(),
                    name.len() as u32,
                    &mut anyv,
                )
            };
            if hit == 0 { None } else { Some(anyv) }
        }
    }
}

/// [`desc_field`] with §6.2.6.5 ToPropertyDescriptor [[Get]]
/// semantics: a descriptor field that is itself an accessor property
/// invokes its getter (test262 defines desc fields via accessors to
/// probe exactly this; a getter-less accessor answers `undefined`).
/// Answers `(anyv, owned)` — `owned` marks a fresh getter product
/// whose ref the caller must consume or drop; a plain data field is
/// a borrow. Caller must check for pending throw when `owned`.
///
/// # Safety
/// Same contract as [`desc_field`].
unsafe fn desc_field_get(desc: DescStore, name: &str) -> Option<(u64, bool)> {
    let raw = unsafe { desc_field(desc, name) }?;
    if unsafe { crate::accessor::value_is_accessor(raw) } {
        let pair = unsafe { __torajs_anyv_cell_ptr(raw) } as *const c_void;
        // §6.2.6.5 [[Get]] receiver = the descriptor object itself.
        let (DescStore::Dyn(d, _) | DescStore::Struct(d)) = desc;
        let recv = unsafe { __torajs_anyv_box_from_pair(4, d as i64) };
        let v = unsafe { crate::accessor::__torajs_accessor_invoke_getter(pair, recv) };
        return Some((v, true));
    }
    Some((raw, false))
}

/// Consume an `owned` flag from [`desc_field_get`] after the value
/// has been read — drops a fresh heap product's surplus ref
/// (immediates — ShortStr included — no-op through the cell gate).
unsafe fn release_desc_field(anyv: u64, owned: bool) {
    if owned {
        let val = unsafe { __torajs_anyv_cell_ptr(anyv) };
        if val != 0 {
            unsafe { __torajs_value_drop_heap(val as *mut c_void) };
        }
    }
}

/// `__torajs_dynobj_define_from_desc(obj_slot, key, desc)` — the
/// runtime-descriptor path for `Object.defineProperty`. Reads the
/// data-descriptor fields (`value` / `writable` / `enumerable` /
/// `configurable`) off the `desc` dynobj at runtime, builds the
/// `flags_byte` + `(tag, value)` the compile-time literal path
/// produces, and applies via [`define_apply`].
///
/// Accessor descriptors (RFC 20260712 chunk 2): when `get` / `set` is
/// present, an `AccessorPair` is built (mirroring the compile-time
/// `emit_accessor_define` shape — each closure ref is inc'd since the
/// desc keeps its own, the pair's +1 transfers into the entry) and
/// stored as the property value; a descriptor mixing accessor and
/// `value` / `writable` fields throws per §10.1.6.3.
///
/// # Safety
/// `obj_slot` points at a live `*mut c_void` (dynobj or NULL). `key`
/// is a live Str. `desc` is a heap cell pointer or NULL. Caller must
/// check for pending throw after return.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_define_from_desc(
    obj_slot: *mut *mut c_void,
    key: *mut c_void,
    desc: *const c_void,
) {
    unsafe { define_from_desc_impl(obj_slot, key, desc, true) };
}

/// §28.1.2 Reflect.defineProperty flavor — identical
/// ToPropertyDescriptor + validate/apply walk, but a §10.1.6.3
/// refusal answers 0 with NO pending throw. A throw raised while
/// READING the descriptor (a getter-backed desc field per §6.2.6.5
/// [[Get]], or an accessor/data mix per §10.1.6.3) still records for
/// either flavor — those belong to ToPropertyDescriptor, not to
/// [[DefineOwnProperty]] — so the caller must still run its
/// throw-check before consuming the answer.
///
/// # Safety
/// Same contract as [`__torajs_dynobj_define_from_desc`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_define_from_desc_soft(
    obj_slot: *mut *mut c_void,
    key: *mut c_void,
    desc: *const c_void,
) -> i64 {
    unsafe { define_from_desc_impl(obj_slot, key, desc, false) }
}

unsafe fn define_from_desc_impl(
    obj_slot: *mut *mut c_void,
    key: *mut c_void,
    desc: *const c_void,
    throw_on_refusal: bool,
) -> i64 {
    if desc.is_null() {
        return 1;
    }
    // §10.5.6 — a Proxy receiver answers the define itself (RFC
    // 20260823-proxy-substrate 刀 8). The slot holds a heap cell
    // pointer, which for a Proxy IS its AnyValue.
    let obj = unsafe { *obj_slot };
    if !obj.is_null()
        && unsafe { obj.cast::<u8>().add(4).cast::<u16>().read() } == crate::layout::TAG_PROXY
    {
        let r = unsafe { __torajs_proxy_define_own(obj as u64, key, desc) };
        if r == 0 && throw_on_refusal && unsafe { __torajs_throw_check() } == 0 {
            unsafe {
                __torajs_throw_type_error(
                    c"proxy 'defineProperty' trap returned falsish".as_ptr() as *const u8
                )
            };
        }
        return r;
    }
    // §6.2.6.5 ToPropertyDescriptor reads off ANY object — dispatch
    // per the desc cell's shape to its own-field store: dynobj entry
    // table (DynObj / Closure-Arr expando) or class-layout struct
    // fields (Tag::Obj — see [`DescStore`]). Pre-fix a Closure
    // descriptor (test262's `descObj = function(){};
    // descObj.enumerable = true` idiom) was probed as a dynobj —
    // SIGSEGV. A NULL store (fresh Closure/Arr, or a shape with no
    // expando domain) is an empty descriptor: all-absent flags still
    // create / validate the property.
    let desc = match unsafe { (desc.cast::<u8>().add(4) as *const u16).read() } {
        TAG_DYNOBJ => Some(DescStore::Dyn(desc, core::ptr::null())),
        TAG_CLOSURE_HDR => {
            // A closure descriptor inherits through
            // Function.prototype's expando dynobj (tag 13 in the
            // builtin-proto registry) — even with no expando of its
            // own. §20.2.3 makes that singleton a built-in FUNCTION
            // object, so its entries live in the same +24 expando any
            // other closure keeps them in; the Arr arm below reaches
            // `Array.prototype`'s the same way, for the same reason.
            let expando = unsafe {
                desc.cast::<u8>()
                    .add(CELL_PROPS_OFF)
                    .cast::<*const c_void>()
                    .read()
            };
            let fp_cell = unsafe { __torajs_get_builtin_prototype(13) };
            let fp = if fp_cell.is_null() {
                core::ptr::null()
            } else {
                unsafe {
                    fp_cell
                        .cast::<u8>()
                        .add(CELL_PROPS_OFF)
                        .cast::<*const c_void>()
                        .read()
                }
            };
            if expando.is_null() && fp.is_null() {
                None
            } else {
                Some(DescStore::Dyn(expando, fp))
            }
        }
        TAG_ARR_HDR => {
            // An array descriptor inherits through Array.prototype's
            // expando (tag 2 — an Arr cell whose monkey-patches land
            // in ITS props dynobj at the same +24 slot).
            let expando = unsafe {
                desc.cast::<u8>()
                    .add(CELL_PROPS_OFF)
                    .cast::<*const c_void>()
                    .read()
            };
            let ap = unsafe { __torajs_get_builtin_prototype(2) };
            let ap_props = if ap.is_null() {
                core::ptr::null()
            } else {
                unsafe {
                    ap.cast::<u8>()
                        .add(CELL_PROPS_OFF)
                        .cast::<*const c_void>()
                        .read()
                }
            };
            if expando.is_null() && ap_props.is_null() {
                None
            } else {
                Some(DescStore::Dyn(expando, ap_props))
            }
        }
        TAG_OBJ => Some(DescStore::Struct(desc)),
        // Rotation 131 — a primitive-wrapper descriptor
        // (`new String()` with expando desc fields) is a true
        // object per §6.2.6.5; its own fields live on the +16 lazy
        // expando (same shape as the Closure/Arr arm above; a NULL
        // expando is the all-absent empty descriptor).
        t if t == crate::define_wrapper::TAG_NUMBER_WRAPPER
            || t == crate::define_wrapper::TAG_STRING_WRAPPER
            || t == crate::define_wrapper::TAG_BOOLEAN_WRAPPER =>
        {
            // A wrapper descriptor inherits through ITS prototype's
            // expando (test262: `String.prototype.value = "String";
            // defineProperty(obj, k, new String("abc"))` — §6.2.6.5
            // [[Get]] climbs the chain). Registry tags: Number=0,
            // String=3, Boolean=4.
            let expando = unsafe {
                desc.cast::<u8>()
                    .add(WRAPPER_PROPS_OFF)
                    .cast::<*const c_void>()
                    .read()
            };
            let proto_tag = if t == crate::define_wrapper::TAG_STRING_WRAPPER {
                3
            } else if t == crate::define_wrapper::TAG_BOOLEAN_WRAPPER {
                4
            } else {
                0
            };
            let wp = unsafe { __torajs_get_builtin_prototype(proto_tag) };
            if expando.is_null() && wp.is_null() {
                None
            } else {
                Some(DescStore::Dyn(expando, wp))
            }
        }
        _ => None,
    };
    let Some(desc) = desc else {
        return unsafe { define_apply(obj_slot, key, 0, 0, 0, throw_on_refusal) };
    };

    let mut flags_byte: u64 = 0;
    let mut out_tag: u64 = 0;
    let mut out_value: u64 = 0;

    // Accessor half (R5b) — a §10.1.6.3 refusal answers per flavor;
    // a ToPropertyDescriptor rejection (face mix / non-callable face)
    // throws for either.
    if let Some(r) = unsafe { try_define_accessor(obj_slot, key, desc, throw_on_refusal) } {
        return r;
    }

    if let Some((v_anyv, v_owned)) = unsafe { desc_field_get(desc, "value") } {
        let v_tag = unsafe { __torajs_anyv_unbox_tag(v_anyv) } as u64;
        // define_apply consumes one rc of a Heap value. A borrowed
        // field (still owned by `desc`) owned-unboxes — cell +1, a
        // ShortStr materializes at rc=1 (the stake; the pre-fix
        // unbox + unconditional inc left it at rc=2, one 32B Str
        // leaked per struct-desc define). A getter product transfers
        // its fresh ref as-is (its ShortStr materializes its own).
        let v_val = if v_owned {
            unsafe { __torajs_anyv_unbox_value(v_anyv) as u64 }
        } else {
            unsafe { __torajs_anyv_unbox_value_owned(v_anyv) as u64 }
        };
        out_tag = v_tag;
        out_value = v_val;
        flags_byte |= DEFINE_PRESENT_VALUE;
    }

    if let Some((w, o)) = unsafe { desc_field_get(desc, "writable") } {
        flags_byte |= DEFINE_PRESENT_WRITABLE;
        if unsafe { __torajs_anyv_to_bool(w) } {
            flags_byte |= DEFINE_FLAG_WRITABLE;
        }
        unsafe { release_desc_field(w, o) };
    }
    if let Some((e, o)) = unsafe { desc_field_get(desc, "enumerable") } {
        flags_byte |= DEFINE_PRESENT_ENUMERABLE;
        if unsafe { __torajs_anyv_to_bool(e) } {
            flags_byte |= DEFINE_FLAG_ENUMERABLE;
        }
        unsafe { release_desc_field(e, o) };
    }
    if let Some((c, o)) = unsafe { desc_field_get(desc, "configurable") } {
        flags_byte |= DEFINE_PRESENT_CONFIGURABLE;
        if unsafe { __torajs_anyv_to_bool(c) } {
            flags_byte |= DEFINE_FLAG_CONFIGURABLE;
        }
        unsafe { release_desc_field(c, o) };
    }

    unsafe {
        define_apply(
            obj_slot,
            key,
            out_tag,
            out_value,
            flags_byte,
            throw_on_refusal,
        )
    }
}
