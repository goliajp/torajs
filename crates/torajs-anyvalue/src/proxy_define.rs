//! §10.5.6 [[DefineOwnProperty]] on a Proxy
//! (RFC 20260823-proxy-substrate 刀 8).
//!
//! This is the trap the `set` family reaches THROUGH, which is why
//! its absence was not merely a missing feature. §10.1.9.2 step 2.e
//! ends an ordinary [[Set]] with `CreateDataProperty(Receiver, P,
//! V)` — the receiver's **[[DefineOwnProperty]]**, not its [[Set]].
//! Without this kernel a proxy receiver took its [[Set]] there
//! instead, so `Reflect.set(target, key, v, proxy)` inside a `set`
//! trap re-entered the same trap forever: a stack overflow, which
//! test262's `with`-over-a-proxy-environment cases produced as a
//! SIGSEGV.
//!
//! A descriptor is an ordinary object here, so the trap argument is
//! the caller's descriptor verbatim; the CreateDataProperty spelling
//! mints the `{value, writable: true, enumerable: true,
//! configurable: true}` §6.2.6.4 shape.

use core::ffi::c_void;

use crate::nanbox::{AnyValue, as_void_ptr, box_bool, box_void_ptr};
use crate::nanbox_ffi::{__torajs_anyv_rc_dec, __torajs_anyv_to_bool};
use crate::proxy::{live_slots, trap};

unsafe extern "C" {
    fn __torajs_throw_check() -> i64;
    fn __torajs_dynobj_define_from_desc_soft(
        obj_slot: *mut *mut c_void,
        key: *mut c_void,
        desc: *const c_void,
    ) -> i64;
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_dynobj_set(obj_slot: *mut *mut c_void, key: *mut c_void, tag: u64, value: u64);
    fn __torajs_str_alloc(bytes: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
    fn __torajs_value_drop_heap(p: *mut c_void);
}

/// §10.5.6. `desc` is a live descriptor object (borrowed). Answers 1
/// on acceptance, 0 on a refusal the caller flavors.
///
/// # Safety
/// `recv` is a live Proxy AnyValue; `key` a live Str or Symbol cell;
/// `desc` a live descriptor cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_proxy_define_own(
    recv: AnyValue,
    key: *mut c_void,
    desc: *const c_void,
) -> i64 {
    unsafe {
        let cell = as_void_ptr(recv);
        let Ok(__s) = live_slots(cell) else {
            return 0;
        };
        let (target, handler) = (__s.target, __s.handler);
        let t = match trap(handler, b"defineProperty") {
            Err(()) => return 0,
            Ok(None) => {
                // The target's own [[DefineOwnProperty]]. A heap-cell
                // AnyValue IS its pointer, so the cell's target slot
                // doubles as the relocation slot the define core
                // writes a grown dynobj back through.
                let mut fallback: AnyValue = target;
                let slot =
                    crate::proxy::forward_slot(cell, target, &mut fallback) as *mut *mut c_void;
                let r = __torajs_dynobj_define_from_desc_soft(slot, key, desc);
                if __torajs_throw_check() != 0 {
                    return 0;
                }
                return r;
            }
            Ok(Some(t)) => t,
        };
        let key_av = crate::proxy_key::key_to_any(key);
        let argv = [target, key_av, box_void_ptr(desc as *mut c_void)];
        let out = crate::method_call_closure_dispatch::__torajs_any_call_with_this(
            t,
            handler,
            argv.as_ptr(),
            3,
        );
        __torajs_anyv_rc_dec(t);
        __torajs_anyv_rc_dec(key_av);
        if __torajs_throw_check() != 0 {
            __torajs_anyv_rc_dec(out);
            return 0;
        }
        let b = __torajs_anyv_to_bool(out);
        __torajs_anyv_rc_dec(out);
        b as i64
    }
}

/// §7.3.5 CreateDataProperty over a Proxy receiver — the ending of
/// §10.1.9.2 step 2.e. `(tag, value)` transfers in.
///
/// # Safety
/// `recv` is a live Proxy AnyValue; `key` a live key cell.
pub(crate) unsafe fn create_data_property(
    recv: AnyValue,
    key: *mut c_void,
    tag: u64,
    value: u64,
) -> i64 {
    unsafe {
        let mut desc = __torajs_dynobj_alloc();
        put(&mut desc, b"value", tag, value);
        let yes = box_bool(true);
        let (btag, bval) = (
            crate::__torajs_anyv_unbox_tag(yes) as u64,
            crate::__torajs_anyv_unbox_value(yes) as u64,
        );
        put(&mut desc, b"writable", btag, bval);
        put(&mut desc, b"enumerable", btag, bval);
        put(&mut desc, b"configurable", btag, bval);
        let r = __torajs_proxy_define_own(recv, key, desc as *const c_void);
        __torajs_value_drop_heap(desc);
        r
    }
}

/// One descriptor field; the `(tag, value)` pair transfers.
unsafe fn put(desc: &mut *mut c_void, name: &[u8], tag: u64, value: u64) {
    unsafe {
        let k = __torajs_str_alloc(name.as_ptr(), name.len() as i64);
        __torajs_dynobj_set(desc, k as *mut c_void, tag, value);
        __torajs_str_drop(k as *mut c_void);
    }
}

/// The compile-time-literal define path's face — the descriptor
/// arrives as a flags byte plus one `(tag, value)` pair rather than
/// as an object, so it is rebuilt into the object §10.5.6 hands the
/// trap. Only the attributes the flags mark PRESENT appear, which is
/// what makes a partial literal descriptor stay partial.
///
/// `(tag, value)` transfers in when the value is present.
///
/// # Safety
/// `recv` is a live Proxy AnyValue; `key` a live key cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_proxy_define_from_flags(
    recv: AnyValue,
    key: *mut c_void,
    tag: u64,
    value: u64,
    flags_byte: u64,
) -> i64 {
    unsafe {
        let mut desc = __torajs_dynobj_alloc();
        if flags_byte & DEFINE_PRESENT_VALUE != 0 {
            put(&mut desc, b"value", tag, value);
        }
        for (present, bit, name) in [
            (
                DEFINE_PRESENT_WRITABLE,
                DEFINE_FLAG_WRITABLE,
                &b"writable"[..],
            ),
            (
                DEFINE_PRESENT_ENUMERABLE,
                DEFINE_FLAG_ENUMERABLE,
                &b"enumerable"[..],
            ),
            (
                DEFINE_PRESENT_CONFIGURABLE,
                DEFINE_FLAG_CONFIGURABLE,
                &b"configurable"[..],
            ),
        ] {
            if flags_byte & present == 0 {
                continue;
            }
            let b = box_bool(flags_byte & bit != 0);
            put(
                &mut desc,
                name,
                crate::__torajs_anyv_unbox_tag(b) as u64,
                crate::__torajs_anyv_unbox_value(b) as u64,
            );
        }
        let r = __torajs_proxy_define_own(recv, key, desc as *const c_void);
        __torajs_value_drop_heap(desc);
        r
    }
}

/// `torajs_dynobj::layout` mirrors — the literal path's flags byte.
const DEFINE_FLAG_WRITABLE: u64 = 1 << 0;
const DEFINE_FLAG_ENUMERABLE: u64 = 1 << 1;
const DEFINE_FLAG_CONFIGURABLE: u64 = 1 << 2;
const DEFINE_PRESENT_WRITABLE: u64 = 1 << 3;
const DEFINE_PRESENT_ENUMERABLE: u64 = 1 << 4;
const DEFINE_PRESENT_CONFIGURABLE: u64 = 1 << 5;
const DEFINE_PRESENT_VALUE: u64 = 1 << 6;
