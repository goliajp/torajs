//! `Tag::Proxy` cells — §10.5 exotic Proxy objects
//! (RFC 20260823-proxy-substrate 刀 1).
//!
//! ```text
//! { header:8 | target:8 (AnyValue) | handler:8 (AnyValue) }   (24 B)
//! ```
//!
//! Both slots own their value. Revocation writes `null` into both,
//! which is what §10.5.4.1 literally says to do — so `is_null` on
//! the handler IS the revoked predicate and there is no separate
//! flag byte that could drift out of step with it.
//!
//! A Proxy has no static type: `new Proxy(t, h)` checks as
//! `Type::Any` and every operation reaches the cell through the
//! any-lane kernels. That is not a shortcut — a Proxy impersonates
//! its target, so any `Type::` variant would be a claim the checker
//! cannot honor.
//!
//! ## Why [[Get]] does not run the trap twice
//!
//! `__torajs_any_member_get_tag` / `_value` is a **pair**: two
//! kernel calls per read. A `get` trap must run exactly once
//! (§10.5.8). The pattern for that already exists for accessors —
//! the pair answers the `ANY_ACCESSOR` sentinel *purely* and the
//! lowering does the single [[Get]] through
//! `__torajs_any_accessor_get`. A Proxy receiver answers the same
//! sentinel with a zero value channel and that one kernel dispatches
//! on the receiver's tag. Neither channel touches the handler.

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell, is_null};
use crate::nanbox_encode::__torajs_anyv_box_pointer;
use crate::nanbox_ffi::{__torajs_anyv_rc_dec, __torajs_anyv_rc_inc};
use crate::to_primitive::is_object_value;

pub(crate) const TARGET_OFF: usize = 8;
pub(crate) const HANDLER_OFF: usize = 16;
const CELL_SIZE: usize = 24;

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_check() -> i64;
    /// torajs-str — key mint / release for the named + index faces.
    fn __torajs_str_alloc(bytes: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
    /// torajs-num — the canonical decimal spelling of an index key.
    fn __torajs_i64_to_str(n: i64) -> *mut c_void;
}

/// Is `av` a Proxy cell? Answers on the heap tag alone — callers use
/// it to decide whether to take the exotic route before any ordinary
/// layout read happens.
#[inline]
pub(crate) fn is_proxy(av: AnyValue) -> bool {
    if !is_cell(av) {
        return false;
    }
    unsafe { as_void_ptr(av).cast::<u8>().add(4).cast::<u16>().read() == Tag::Proxy as u16 }
}

/// The cell's `[[ProxyTarget]]` / `[[ProxyHandler]]`, BORROWED.
///
/// # Safety
/// `ptr` is a live Proxy cell.
#[inline]
pub(crate) unsafe fn slots(ptr: *mut c_void) -> (AnyValue, AnyValue) {
    unsafe {
        let p = ptr.cast::<u8>();
        (
            (p.add(TARGET_OFF) as *const u64).read(),
            (p.add(HANDLER_OFF) as *const u64).read(),
        )
    }
}

/// §10.5.14 ProxyCreate. Both arguments must be objects; the result
/// is an owned Proxy cell boxed as `any`. A rejected argument
/// records the catchable TypeError and answers `undefined` for the
/// caller's throw check.
///
/// Arguments are BORROWED — the cell takes its own `+1` on each.
///
/// # Safety
/// `target` / `handler` are valid AnyValues alive across the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_proxy_create(target: AnyValue, handler: AnyValue) -> AnyValue {
    if !is_object_value(target) {
        unsafe {
            __torajs_throw_type_error(c"Cannot create proxy with a non-object as target".as_ptr())
        };
        return VALUE_UNDEFINED;
    }
    if !is_object_value(handler) {
        unsafe {
            __torajs_throw_type_error(c"Cannot create proxy with a non-object as handler".as_ptr())
        };
        return VALUE_UNDEFINED;
    }
    unsafe {
        __torajs_anyv_rc_inc(target);
        __torajs_anyv_rc_inc(handler);
        let layout = core::alloc::Layout::from_size_align(CELL_SIZE, 8).unwrap();
        let cell = std::alloc::alloc_zeroed(layout);
        *(cell as *mut u32) = 1;
        *(cell.add(4) as *mut u16) = Tag::Proxy as u16;
        *(cell.add(TARGET_OFF) as *mut u64) = target;
        *(cell.add(HANDLER_OFF) as *mut u64) = handler;
        __torajs_anyv_box_pointer(cell as *mut c_void)
    }
}

/// `value_drop`'s Proxy arm — release both slots and free.
///
/// # Safety
/// `cell` is a live Proxy cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_proxy_drop(cell: *mut c_void) {
    unsafe {
        if torajs_rc::__torajs_rc_dec(cell) == 0 {
            return;
        }
        let p = cell.cast::<u8>();
        __torajs_anyv_rc_dec((p.add(TARGET_OFF) as *const u64).read());
        __torajs_anyv_rc_dec((p.add(HANDLER_OFF) as *const u64).read());
        let layout = core::alloc::Layout::from_size_align(CELL_SIZE, 8).unwrap();
        std::alloc::dealloc(p, layout);
    }
}

/// The §10.5.x step-2/3 revoked check every internal method opens
/// with, answering the two slots as OWNED stakes.
///
/// They have to be owned. The spec captures `handler` and `target`
/// as references before it looks the trap up, and looking the trap
/// up can run user code — a HANDLER THAT IS ITSELF A PROXY revokes
/// this one from inside its own `get` trap (test262's
/// `Proxy/revoke-as-side-effect`). Revocation drops the cell's two
/// stakes, so a borrowed read taken beforehand is dangling by the
/// time the trap answers: a use-after-free, observed as SIGSEGV.
///
/// [`Slots`] releases both on the way out, so every internal method
/// gets the spec's lifetime by holding one.
pub(crate) struct Slots {
    pub(crate) target: AnyValue,
    pub(crate) handler: AnyValue,
}

impl Drop for Slots {
    fn drop(&mut self) {
        unsafe {
            __torajs_anyv_rc_dec(self.target);
            __torajs_anyv_rc_dec(self.handler);
        }
    }
}

/// # Safety
/// `ptr` is a live Proxy cell.
pub(crate) unsafe fn live_slots(ptr: *mut c_void) -> Result<Slots, ()> {
    let (target, handler) = unsafe { slots(ptr) };
    if is_null(handler) {
        unsafe {
            __torajs_throw_type_error(
                c"Cannot perform operation on a proxy that has been revoked".as_ptr(),
            )
        };
        return Err(());
    }
    unsafe {
        __torajs_anyv_rc_inc(target);
        __torajs_anyv_rc_inc(handler);
    }
    Ok(Slots { target, handler })
}

/// §7.3.11 GetMethod(handler, name) for a trap.
///
/// `Ok(None)` = absent or nullish, i.e. forward to the target;
/// `Ok(Some(f))` = an OWNED callable; `Err(())` = a pending throw
/// (the [[Get]] on the handler threw, or the entry is present but
/// not callable — the step-4 TypeError).
///
/// # Safety
/// `handler` is a live object AnyValue.
pub(crate) unsafe fn trap(handler: AnyValue, name: &[u8]) -> Result<Option<AnyValue>, ()> {
    let Some(got) = (unsafe { crate::proxy_get_prop::get_by_name(handler, name) }) else {
        return Err(());
    };
    if crate::nanbox::is_undefined(got) || is_null(got) {
        unsafe { __torajs_anyv_rc_dec(got) };
        return Ok(None);
    }
    if unsafe { crate::method_call_closure_dispatch::closure_boxed_entry(got) }.is_some() {
        return Ok(Some(got));
    }
    unsafe {
        __torajs_anyv_rc_dec(got);
        __torajs_throw_type_error(c"proxy handler trap is not a function".as_ptr());
    }
    Err(())
}

/// §10.5.8 [[Get]] — `trap(handler, target, key, receiver)` when the
/// handler has one, the target's own [[Get]] otherwise. Result is
/// OWNED.
///
/// `receiver` is the value the read was addressed on. For a plain
/// `p.k` that is the proxy itself; the parameter exists so a
/// prototype-chain walk can hand down the original receiver.
///
/// # Safety
/// `ptr` is a live Proxy cell; `key` a live Str or Symbol cell.
pub(crate) unsafe fn get(ptr: *mut c_void, key: *const c_void, receiver: AnyValue) -> AnyValue {
    unsafe {
        let Ok(s) = live_slots(ptr) else {
            return VALUE_UNDEFINED;
        };
        let (target, handler) = (s.target, s.handler);
        let t = match trap(handler, b"get") {
            Err(()) => return VALUE_UNDEFINED,
            Ok(None) => return crate::proxy_get_prop::get_by_key(target, key),
            Ok(Some(t)) => t,
        };
        let key_av = crate::proxy_key::key_to_any(key);
        let argv = [target, key_av, receiver];
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
            return VALUE_UNDEFINED;
        }
        out
    }
}

/// [[Get]] on a Proxy for a plain ASCII property name — the shape
/// the special-cased member intrinsics (`.length`, `.name`) and the
/// index lanes reach it through. Answers OWNED.
///
/// # Safety
/// `recv` is a live Proxy AnyValue.
pub(crate) unsafe fn get_named(recv: AnyValue, name: &[u8]) -> AnyValue {
    unsafe {
        let key = __torajs_str_alloc(name.as_ptr(), name.len() as i64) as *const c_void;
        let out = get(as_void_ptr(recv), key, recv);
        __torajs_str_drop(key as *mut c_void);
        out
    }
}

/// [[Get]] on a Proxy for a canonical numeric index — §6.1.7 says a
/// property key is a String, and `p[0]` must reach the trap spelled
/// `"0"`, not as a number.
///
/// # Safety
/// `recv` is a live Proxy AnyValue.
pub(crate) unsafe fn get_index(recv: AnyValue, idx: i64) -> AnyValue {
    unsafe {
        let key = __torajs_i64_to_str(idx);
        let out = get(as_void_ptr(recv), key as *const c_void, recv);
        __torajs_str_drop(key);
        out
    }
}

/// [[Get]] on a Proxy for an existing key cell (Str or Symbol) —
/// the keyed index lane's face.
///
/// # Safety
/// `recv` is a live Proxy AnyValue; `key` a live Str or Symbol cell.
pub(crate) unsafe fn get_key_cell(recv: AnyValue, key: *const c_void) -> AnyValue {
    unsafe { get(as_void_ptr(recv), key, recv) }
}

/// The slot a trap-less forward writes through.
///
/// Normally that is the cell's own target slot, so a dynobj that
/// relocates on grow writes its fresh address back and the proxy
/// follows it. But the trap LOOKUP can revoke this proxy (a handler
/// that is itself a proxy — see [`Slots`]), and then the cell's slot
/// holds `null`: the forward must go to the target the spec captured
/// BEFORE the lookup, which is what `captured` holds. `fallback` is
/// the caller's own storage for that case.
///
/// # Safety
/// `cell` is a live Proxy cell; `fallback` is a live slot the caller
/// owns for the duration of the forward.
pub(crate) unsafe fn forward_slot(
    cell: *mut c_void,
    captured: AnyValue,
    fallback: *mut AnyValue,
) -> *mut AnyValue {
    unsafe {
        let (_, live) = slots(cell);
        if is_null(live) {
            *fallback = captured;
            return fallback;
        }
        cell.cast::<u8>().add(TARGET_OFF) as *mut AnyValue
    }
}
