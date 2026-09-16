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
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// Private name bytes → its WeakMap cell (owned by the table for the
/// process).
type Stores = Vec<(Box<[u8]>, *mut c_void)>;

static STORES: AtomicPtr<Stores> = AtomicPtr::new(core::ptr::null_mut());

/// The WeakMap for private name `key`, created on first use when
/// `create`; NULL when absent and not created.
///
/// # Safety
/// `key` is a live Str cell.
unsafe fn store(key: *const c_void, create: bool) -> *mut c_void {
    let mut table = STORES.load(Ordering::Relaxed);
    if table.is_null() {
        if !create {
            return core::ptr::null_mut();
        }
        table = Box::into_raw(Box::new(Stores::new()));
        STORES.store(table, Ordering::Relaxed);
    }
    // SAFETY: the table is leaked for the process and only this module
    // touches it, one mutator at a time.
    let table = unsafe { &mut *table };
    let wtf8 = unsafe { StrWtf8::of(key) };
    let name = wtf8.as_bytes();
    if let Some((_, m)) = table.iter().find(|(n, _)| **n == *name) {
        return *m;
    }
    if !create {
        return core::ptr::null_mut();
    }
    let m = unsafe { __torajs_weakmap_create() };
    table.push((name.into(), m));
    m
}

/// The store and weak key for (`recv`, `key`) when `recv` holds the
/// element.
unsafe fn find(recv: AnyValue, key: *const c_void) -> Option<(*mut c_void, *mut c_void)> {
    let k = unsafe { __torajs_weak_key_from_any(recv) };
    if k.is_null() {
        return None;
    }
    let m = unsafe { store(key, false) };
    (!m.is_null() && unsafe { __torajs_weakmap_has(m, k) } != 0).then_some((m, k))
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
    let (m, k) = unsafe { find(recv, key) }?;
    let v = unsafe { __torajs_weakmap_get(m, k) };
    if crate::nanbox::is_cell(v) {
        // `get` handed out a fresh stake; the map still holds its own.
        unsafe { __torajs_value_drop_heap(v as usize as *mut c_void) };
    }
    Some(v)
}

/// PrivateSet on a held element: replaces the value. Answers false
/// when `recv` does not hold `key` here. `value` is borrowed.
///
/// # Safety
/// `key` is a live Str cell.
pub(crate) unsafe fn foreign_set(recv: AnyValue, key: *const c_void, value: AnyValue) -> bool {
    match unsafe { find(recv, key) } {
        Some((m, k)) => {
            unsafe { __torajs_weakmap_set(m, k, value) };
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
    let m = unsafe { store(key, true) };
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
    let k = unsafe { __torajs_weak_key_from_any(recv) };
    let table = STORES.load(Ordering::Relaxed);
    if k.is_null() || table.is_null() {
        return None;
    }
    let wtf8 = unsafe { StrWtf8::of(key) };
    let member = wtf8.as_bytes().strip_prefix(b"__priv_")?;
    // SAFETY: leaked for the process, single mutator.
    for (name, m) in unsafe { &*table } {
        let Some(class) = name.strip_prefix(b"__privbrand_".as_slice()) else {
            continue;
        };
        if !member.starts_with(class) || !member[class.len()..].starts_with(b"__") {
            continue;
        }
        if unsafe { __torajs_weakmap_has(*m, k) } == 0 {
            continue;
        }
        let proto = unsafe { __torajs_weakmap_get(*m, k) };
        if crate::nanbox::is_cell(proto) {
            unsafe { __torajs_value_drop_heap(proto as usize as *mut c_void) };
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
    if STORES.load(Ordering::Relaxed).is_null() || name_str.is_null() {
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
