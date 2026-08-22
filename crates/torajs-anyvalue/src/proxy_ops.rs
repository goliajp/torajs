//! §10.5.9 [[Set]] / §10.5.7 [[HasProperty]] / §10.5.10 [[Delete]]
//! on a Proxy (RFC 20260823-proxy-substrate 刀 2).
//!
//! All three have the same skeleton as [[Get]]: look the trap up on
//! the handler, forward to the target when there is none, and
//! otherwise call it with the handler as `this`. The difference is
//! what the answer means — these three answer a **boolean**, and the
//! spec runs it through ToBoolean, so a handler returning `0` or
//! `""` refuses the operation just as `false` does.
//!
//! What a refusal COSTS is the caller's, not ours: a strict
//! assignment turns it into a TypeError, `Reflect.set` answers
//! `false`, and the two shells over the member-set core already
//! carry that split. The kernels here answer the boolean and let the
//! shell decide.
//!
//! The §10.5.x invariant checks (the non-extensible /
//! non-configurable consistency rules) are NOT here: they need
//! [[GetOwnProperty]] on the target, which knife 4 builds.

use core::ffi::c_void;

use crate::nanbox::AnyValue;
use crate::nanbox_ffi::{__torajs_anyv_rc_dec, __torajs_anyv_to_bool};
use crate::proxy::{live_slots, trap};

unsafe extern "C" {
    fn __torajs_throw_check() -> i64;
}

/// Call a trap with `(target, key[, extra…])` and ToBoolean the
/// answer. `extra` slots follow the key. Err = pending throw.
///
/// # Safety
/// `t` is an owned callable; `handler` / `target` live; `key` a live
/// Str or Symbol cell; every `extra` slot a valid AnyValue the
/// caller keeps alive.
unsafe fn call_bool_trap(
    t: AnyValue,
    handler: AnyValue,
    target: AnyValue,
    key: *const c_void,
    extra: &[AnyValue],
) -> Result<bool, ()> {
    unsafe {
        let key_av = crate::proxy_key::key_to_any(key);
        let mut argv: [u64; 4] = [target, key_av, 0, 0];
        for (i, v) in extra.iter().enumerate() {
            argv[2 + i] = *v;
        }
        let argc = 2 + extra.len() as i64;
        let out = crate::method_call_closure_dispatch::__torajs_any_call_with_this(
            t,
            handler,
            argv.as_ptr(),
            argc,
        );
        __torajs_anyv_rc_dec(t);
        __torajs_anyv_rc_dec(key_av);
        if __torajs_throw_check() != 0 {
            __torajs_anyv_rc_dec(out);
            return Err(());
        }
        let b = __torajs_anyv_to_bool(out);
        __torajs_anyv_rc_dec(out);
        Ok(b)
    }
}

/// §10.5.9 [[Set]]. `value` transfers in OWNED — the forwarding arm
/// hands it to the target's set core (which takes the transfer), the
/// trap arm releases it after the call. Answers `Ok(true)` when the
/// write was handled, `Ok(false)` on a refusal the caller must turn
/// into its own flavor of "no".
///
/// # Safety
/// `cell` is a live Proxy cell; `key` a live Str or Symbol cell.
pub(crate) unsafe fn set(
    cell: *mut c_void,
    key: *mut c_void,
    value_tag: u64,
    value_bits: u64,
    receiver: AnyValue,
) -> Result<bool, ()> {
    unsafe {
        let __s = live_slots(cell)?;
        let (target, handler) = (__s.target, __s.handler);
        let t = match trap(handler, b"set")? {
            None => {
                // §10.5.9 step 6 is `target.[[Set]](P, V, Receiver)`
                // — the lookup walks the TARGET while the write and
                // any setter's `this` go to the RECEIVER, which is
                // the proxy. Spelling it as a plain set on the target
                // loses that, and the loss is observable: the walk's
                // §10.1.9.2 step 2.e CreateDataProperty lands on the
                // receiver, so a proxy revoked during this very trap
                // lookup must raise the revoked TypeError there.
                let mut recv_slot: AnyValue = receiver;
                let handled = crate::member_set_receiver::__torajs_any_member_set_with_receiver(
                    target,
                    key,
                    value_tag,
                    value_bits,
                    &mut recv_slot,
                );
                if __torajs_throw_check() != 0 {
                    return Err(());
                }
                return Ok(handled != 0);
            }
            Some(t) => t,
        };
        let v =
            crate::nanbox_encode::__torajs_anyv_box_from_pair(value_tag as i64, value_bits as i64);
        let out = call_bool_trap(t, handler, target, key, &[v, receiver]);
        __torajs_anyv_rc_dec(v);
        out
    }
}

/// §10.5.7 [[HasProperty]] — the `has` trap or the target's own
/// walk (which is prototype-inclusive, per §7.3.11).
///
/// # Safety
/// `cell` is a live Proxy cell; `key` a live Str or Symbol cell.
pub(crate) unsafe fn has(cell: *mut c_void, key: *const c_void) -> Result<bool, ()> {
    unsafe {
        let __s = live_slots(cell)?;
        let (target, handler) = (__s.target, __s.handler);
        let Some(t) = trap(handler, b"has")? else {
            let r = crate::prop_has::__torajs_any_has_property(target, key);
            if __torajs_throw_check() != 0 {
                return Err(());
            }
            return Ok(r != 0);
        };
        call_bool_trap(t, handler, target, key, &[])
    }
}

/// §10.5.10 [[Delete]] — the `deleteProperty` trap or the target's
/// own delete. `throw_on_refusal` picks the strict-`delete` flavor
/// of the forwarding arm, matching the two shells over the ordinary
/// delete core.
///
/// # Safety
/// `cell` is a live Proxy cell; `key` a live Str or Symbol cell.
pub(crate) unsafe fn delete(
    cell: *mut c_void,
    key: *const c_void,
    throw_on_refusal: bool,
) -> Result<bool, ()> {
    unsafe {
        let __s = live_slots(cell)?;
        let (target, handler) = (__s.target, __s.handler);
        let Some(t) = trap(handler, b"deleteProperty")? else {
            let r = if throw_on_refusal {
                crate::prop_delete::__torajs_any_prop_delete(target, key)
            } else {
                crate::prop_delete::__torajs_any_prop_delete_soft(target, key)
            };
            if __torajs_throw_check() != 0 {
                return Err(());
            }
            return Ok(r != 0);
        };
        call_bool_trap(t, handler, target, key, &[])
    }
}
