//! RFC 20260916-private-element-brand slice 2c — private elements on
//! an object its class did not mint.
//!
//! A derived class whose base constructor returns some other object
//! (§10.2.2 step 13 — a plain object, an array, a Proxy, another
//! class's instance) installs its private elements on THAT object
//! (§7.3.34 on whatever `this` became). The object has no layout slot
//! and no install cursor for them, and they must stay invisible to
//! reflection and to Proxy traps, so they live here instead: one
//! torajs-weak WeakMap per private name, keyed by the object, holding
//! the element's value (a method brand holds `undefined`). The WeakMap
//! drops an entry when its key object dies.
//!
//! The spec's PrivateElementFind on an object is exactly "does this
//! map have the object", which is what makes this store the right
//! shape rather than an approximation.
//!
//! Atomics per the multi-thread-ready substrate rule (design principles
//! §6.2), with the codebase's single-mutator Relaxed ordering; the
//! table only grows, one entry per distinct private name seen.

use core::ffi::c_void;
use core::sync::atomic::{AtomicPtr, Ordering};

use torajs_rc::str_wtf8::StrWtf8;

use crate::nanbox::AnyValue;

unsafe extern "C" {
    fn __torajs_weakmap_create() -> *mut c_void;
    fn __torajs_weakmap_set(p: *mut c_void, key: *mut c_void, value: u64);
    fn __torajs_weakmap_get(p: *mut c_void, key: *mut c_void) -> u64;
    fn __torajs_weakmap_has(p: *mut c_void, key: *mut c_void) -> i64;
    fn __torajs_weak_key_from_any(av: u64) -> *mut c_void;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// The table, and the weak-collection kernels the readers call through.
///
/// The kernels are reached through these pointers rather than by name
/// because the readers sit on the any-lane method dispatch path, which
/// nearly every program links: naming the torajs-weak kernels there
/// pulled them into every such binary (rotation 662 measured +1.3 KB of
/// `__text` and a page more on 12 bench artifacts). Only [`foreign_add`]
/// names them, and it is reached from the return-override carry alone;
/// it fills this table on first use, so a program that never adopts an
/// object never links the kernels and every reader stops at the null
/// table.
struct Stores {
    names: Vec<(Box<[u8]>, *mut c_void)>,
    has: unsafe extern "C" fn(*mut c_void, *mut c_void) -> i64,
    get: unsafe extern "C" fn(*mut c_void, *mut c_void) -> u64,
    set: unsafe extern "C" fn(*mut c_void, *mut c_void, u64),
    key_of: unsafe extern "C" fn(u64) -> *mut c_void,
}

static STORES: AtomicPtr<Stores> = AtomicPtr::new(core::ptr::null_mut());

/// The table, when some object was ever adopted.
fn table() -> Option<&'static mut Stores> {
    let t = STORES.load(Ordering::Relaxed);
    // SAFETY: leaked for the process; single mutator.
    (!t.is_null()).then(|| unsafe { &mut *t })
}

/// The WeakMap for private name `key` in `t`, NULL when absent.
///
/// # Safety
/// `key` is a live Str cell.
unsafe fn store_in(t: &Stores, key: *const c_void) -> *mut c_void {
    let wtf8 = unsafe { StrWtf8::of(key) };
    let name = wtf8.as_bytes();
    t.names
        .iter()
        .find(|(n, _)| **n == *name)
        .map_or(core::ptr::null_mut(), |(_, m)| *m)
}

/// The table, the store and weak key for (`recv`, `key`) when `recv`
/// holds the element.
unsafe fn find(
    recv: AnyValue,
    key: *const c_void,
) -> Option<(&'static Stores, *mut c_void, *mut c_void)> {
    let t = table()?;
    let m = unsafe { store_in(t, key) };
    if m.is_null() {
        return None;
    }
    let k = unsafe { (t.key_of)(recv) };
    (!k.is_null() && unsafe { (t.has)(m, k) } != 0).then_some((t, m, k))
}

/// Whether `recv` holds private element `key` in this store.
///
/// # Safety
/// `key` is a live Str cell.
pub(crate) unsafe fn foreign_has(recv: AnyValue, key: *const c_void) -> bool {
    unsafe { find(recv, key) }.is_some()
}

/// The element's value, borrowed (the store keeps its own stake), when
/// `recv` holds it.
///
/// # Safety
/// `key` is a live Str cell.
pub(crate) unsafe fn foreign_get(recv: AnyValue, key: *const c_void) -> Option<AnyValue> {
    let (t, m, k) = unsafe { find(recv, key) }?;
    Some(unsafe { borrowed_get(t, m, k) })
}

/// `get` hands out a fresh stake on a cell; the map keeps its own, so the
/// answer is released back to a borrow.
unsafe fn borrowed_get(t: &Stores, m: *mut c_void, k: *mut c_void) -> AnyValue {
    let v = unsafe { (t.get)(m, k) };
    if crate::nanbox::is_cell(v) {
        unsafe { crate::nanbox_ffi::__torajs_anyv_rc_dec(v) };
    }
    v
}

/// PrivateSet on a held element: replaces the value. Answers false
/// when `recv` does not hold `key` here. `value` is borrowed.
///
/// # Safety
/// `key` is a live Str cell.
pub(crate) unsafe fn foreign_set(recv: AnyValue, key: *const c_void, value: AnyValue) -> bool {
    match unsafe { find(recv, key) } {
        Some((t, m, k)) => {
            unsafe { (t.set)(m, k, value) };
            true
        }
        None => false,
    }
}

/// §7.3.32 PrivateFieldAdd / §7.3.33 PrivateMethodOrAccessorAdd onto an
/// object the class did not mint: adding an element the object already
/// holds is the TypeError. `value` is borrowed; a value that cannot be
/// held weakly as a key never reaches here (the caller adopted it as
/// an object).
///
/// # Safety
/// `key` is a live Str cell.
pub(crate) unsafe fn foreign_add(recv: AnyValue, key: *const c_void, value: AnyValue) {
    let k = unsafe { __torajs_weak_key_from_any(recv) };
    if k.is_null() {
        return;
    }
    let t = match table() {
        Some(t) => t,
        None => {
            let t = Box::into_raw(Box::new(Stores {
                names: Vec::new(),
                has: __torajs_weakmap_has,
                get: __torajs_weakmap_get,
                set: __torajs_weakmap_set,
                key_of: __torajs_weak_key_from_any,
            }));
            STORES.store(t, Ordering::Relaxed);
            // SAFETY: just leaked for the process.
            unsafe { &mut *t }
        }
    };
    let mut m = unsafe { store_in(t, key) };
    if m.is_null() {
        m = unsafe { __torajs_weakmap_create() };
        let wtf8 = unsafe { StrWtf8::of(key) };
        t.names.push((wtf8.as_bytes().into(), m));
    }
    if unsafe { __torajs_weakmap_has(m, k) } != 0 {
        unsafe {
            __torajs_throw_type_error(
                c"cannot add a private element twice to the same object".as_ptr(),
            )
        };
        return;
    }
    unsafe { __torajs_weakmap_set(m, k, value) };
}

/// The prototype a foreign-store brand recorded for the class owning
/// the private method or accessor `key` (`__priv_<C>__<x>`), when
/// `recv` carries that class's brand (`__privbrand_<C>`) — borrowed.
///
/// # Safety
/// `key` is a live Str cell.
pub(crate) unsafe fn foreign_brand_proto(recv: AnyValue, key: *const c_void) -> Option<AnyValue> {
    let t = table()?;
    let k = unsafe { (t.key_of)(recv) };
    if k.is_null() {
        return None;
    }
    let wtf8 = unsafe { StrWtf8::of(key) };
    let member = wtf8.as_bytes().strip_prefix(b"__priv_")?;
    for (name, m) in &t.names {
        let Some(class) = name.strip_prefix(b"__privbrand_".as_slice()) else {
            continue;
        };
        if !member.starts_with(class) || !member[class.len()..].starts_with(b"__") {
            continue;
        }
        if unsafe { (t.has)(*m, k) } == 0 {
            continue;
        }
        let proto = unsafe { borrowed_get(t, *m, k) };
        if crate::nanbox::is_cell(proto) {
            return Some(proto);
        }
    }
    None
}

/// `o.#m(...args)` where `o` holds the brand of the class owning `#m` in
/// this store: that class's method, called with `o` as `this`. `None`
/// for every other receiver or name, so the caller keeps its own path.
/// The empty-store check comes first: a program that never adopted an
/// object pays one load.
///
/// # Safety
/// `name_str` is a live Str cell; `argv` holds `argc` live AnyValues.
pub(crate) unsafe fn foreign_private_call(
    recv: AnyValue,
    name_str: *const u8,
    argv: *const u64,
    argc: i64,
) -> Option<AnyValue> {
    if table().is_none() || name_str.is_null() {
        return None;
    }
    let key = name_str as *const c_void;
    if !unsafe { StrWtf8::of(key) }
        .as_bytes()
        .starts_with(b"__priv_")
    {
        return None;
    }
    let proto = unsafe { foreign_brand_proto(recv, key) }?;
    let tag = unsafe { crate::member_get::__torajs_any_member_get_tag(proto, key) };
    let val = unsafe { crate::member_get_value::__torajs_any_member_get_value(proto, key) };
    let f = unsafe { crate::nanbox_encode::__torajs_anyv_box_from_pair(tag as i64, val as i64) };
    unsafe { crate::method_call_closure_dispatch::closure_boxed_entry(f) }?;
    Some(unsafe {
        crate::method_call_closure_dispatch::__torajs_any_call_with_this(f, recv, argv, argc)
    })
}

/// §7.3.32 PrivateFieldAdd for an instance that is NOT a layout cell:
/// the runtime-value class lane builds its instances as plain objects,
/// which have no slot and no install cursor for a private field, so
/// the field lives in the store above from the moment its initializer
/// ran (`torajs-core` `ast/capturing_classes/private_fields.rs` spells
/// the constructor prefix as this call). Adding it twice is the
/// TypeError; every reader already consults the store first.
///
/// A receiver whose LAYOUT declares the field (a class instance cell
/// reaching a `__ctorany_` twin) keeps the brand-gated write the
/// prefix always was: the slot is the field.
///
/// # Safety
/// All three are live AnyValues; `key` holds the private name's Str.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_priv_field_define(
    recv: AnyValue,
    key: AnyValue,
    value: AnyValue,
) -> AnyValue {
    unsafe {
        let kp = crate::index_any_keyed::__torajs_anyv_to_property_key(key);
        if !kp.is_null() {
            if crate::member_get_private::priv_brand_declared(recv, kp as *const c_void) {
                // The write kernel takes the payload's stake over, so
                // it gets one of its own (`value` is borrowed here).
                let mut slot = recv;
                crate::member_set_private::__torajs_any_member_set_priv(
                    &mut slot,
                    kp,
                    crate::nanbox_encode::__torajs_anyv_unbox_tag(value) as u64,
                    crate::nanbox_encode::__torajs_anyv_unbox_value_owned(value) as u64,
                    0,
                );
            } else {
                foreign_add(recv, kp as *const c_void, value);
            }
            crate::index_any_keyed::__torajs_anyv_property_key_drop(kp);
        }
    }
    crate::nanbox::VALUE_UNDEFINED
}
