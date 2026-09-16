//! §27.2.5.4 steps 3-4 of `Promise.prototype.then` on the any lane —
//! `C = SpeciesConstructor(promise, %Promise%)` and, when `C` is not
//! %Promise%, `NewPromiseCapability(C)`: the derived promise is
//! whatever `C` constructs, and the reaction job settles it through
//! the capability's resolve / reject functions instead of writing a
//! result cell.
//!
//! The kernel lane (`method_call_promise::promise_method`) stays the
//! default. [`species_may_differ`] is the read-only precheck that
//! decides whether the `constructor` GET has to run at all: an
//! unpatched intrinsic cell's `constructor` can only come from
//! %Promise.prototype%, and that constructor's `@@species` only from
//! %Promise% itself, so a cell with no expando bag, no subclass flag,
//! no own `constructor` on the prototype singleton and an untouched
//! `@@species` accessor is exactly the default.

use core::ffi::c_void;

use torajs_rc::__torajs_rc_inc;

use crate::buffer_species::SpeciesResolved;
use crate::method_call_closure_dispatch::{__torajs_any_call, closure_boxed_entry, invoke_boxed};
use crate::method_call_promise::{
    P_PROPS_OFF, P_REPR_OFF, P_STATE_OFF, P_VALUE_OFF, STATE_REJECTED, box_settled,
};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED};
use crate::nanbox_encode::__torajs_anyv_box_from_pair;
use crate::nanbox_ffi::__torajs_anyv_rc_dec;

/// `torajs_rc::builtin_proto` tag of %Promise.prototype% (the
/// `promise_static_patch` constant).
const PROMISE_PROTO_TAG: i64 = 10;

unsafe extern "C" {
    fn __torajs_promise_attach_then(
        source: *mut c_void,
        invoke: Option<unsafe extern "C" fn(i64)>,
        arg: i64,
    );
    fn __torajs_promise_drop(p: *mut c_void);
    fn __torajs_throw_check() -> i64;
    fn __torajs_throw_take() -> i64;
    fn __torajs_throw_take_tag() -> i64;
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn malloc(n: usize) -> *mut c_void;
    fn free(p: *mut c_void);
}

/// Packed attach state for [`then_cap_dispatch`]. `source` and the
/// handler envs hold +1 stakes taken at attach; `resolve` / `reject`
/// are the capability's owned boxes.
#[repr(C)]
struct ThenCapArg {
    source: *mut c_void,
    on_ok: *mut c_void,
    on_ok_entry: u64,
    on_err: *mut c_void,
    on_err_entry: u64,
    resolve: AnyValue,
    reject: AnyValue,
}

/// Whether SpeciesConstructor might answer something other than
/// %Promise% for this cell — see module doc. No getter runs.
///
/// # Safety
/// `cell` is a live Promise-tagged heap pointer.
pub(crate) unsafe fn species_may_differ(cell: *mut c_void) -> bool {
    unsafe {
        if !(*(cell.cast::<u8>().add(P_PROPS_OFF) as *const *mut c_void)).is_null() {
            return true;
        }
        if (cell.cast::<u8>().add(6) as *const u16).read() & torajs_rc::FLAG_SUBCLASSED != 0 {
            return true;
        }
        let proto = torajs_rc::builtin_proto::__torajs_peek_builtin_prototype(PROMISE_PROTO_TAG);
        if !proto.is_null() && crate::method_support_proto::proto_own_has(proto, constructor_key())
        {
            return true;
        }
        let ctor = crate::method_value::ctor::ctor_cell_peek(PROMISE_PROTO_TAG);
        !ctor.is_null()
            && crate::closure_proto::ctor_species_accessor::species_replaced(
                PROMISE_PROTO_TAG,
                ctor.cast(),
            )
    }
}

/// The immortal `"constructor"` key the precheck probes under.
fn constructor_key() -> *const c_void {
    use core::sync::atomic::{AtomicU64, Ordering};
    static KEY: AtomicU64 = AtomicU64::new(0);
    let k = KEY.load(Ordering::Relaxed);
    if k != 0 {
        return k as *const c_void;
    }
    let cell = crate::method_value::mint_immortal_str(b"constructor");
    KEY.store(cell as u64, Ordering::Relaxed);
    cell as *const c_void
}

/// `then` (`ok_av`, `err_av` already sliced per method) through a
/// non-default species. `None` = the default: the caller's kernel
/// lane runs, and the one `constructor` GET the spec asks for has
/// already happened here. `Some(undefined)` carries a pending throw
/// (the GET, the species read, or `C`'s construction raised).
///
/// # Safety
/// `cell` is a live Promise-tagged heap pointer; the handler boxes are
/// borrowed.
pub(crate) unsafe fn then_via_species(
    cell: *mut c_void,
    ok_av: AnyValue,
    err_av: AnyValue,
) -> Option<AnyValue> {
    unsafe {
        let recv = __torajs_anyv_box_from_pair(4, cell as i64);
        let c = match crate::method_call_promise_invoke::species_ctor_of(recv) {
            SpeciesResolved::Default => return None,
            SpeciesResolved::Threw => return Some(VALUE_UNDEFINED),
            SpeciesResolved::Ctor(c) => c,
        };
        let cap = crate::promise_capability::new_promise_capability(c);
        __torajs_anyv_rc_dec(c);
        let (promise, resolve, reject) = cap?;
        let (ok_env, ok_entry) = closure_boxed_entry(ok_av).unwrap_or((core::ptr::null_mut(), 0));
        let (err_env, err_entry) =
            closure_boxed_entry(err_av).unwrap_or((core::ptr::null_mut(), 0));
        let a = malloc(core::mem::size_of::<ThenCapArg>()) as *mut ThenCapArg;
        a.write(ThenCapArg {
            source: cell,
            on_ok: ok_env,
            on_ok_entry: ok_entry,
            on_err: err_env,
            on_err_entry: err_entry,
            resolve,
            reject,
        });
        __torajs_rc_inc(cell);
        for env in [ok_env, err_env] {
            if !env.is_null() {
                __torajs_rc_inc(env);
            }
        }
        __torajs_promise_attach_then(cell, Some(then_cap_dispatch), a as i64);
        Some(promise)
    }
}

/// §27.2.2.1 NewPromiseReactionJob with a capability: an empty handler
/// slot passes the settlement to `resolve` / `reject`; a handler's
/// return goes to `resolve`, its throw to `reject`.
unsafe extern "C" fn then_cap_dispatch(arg: i64) {
    let a = arg as *mut ThenCapArg;
    unsafe {
        let src = (*a).source;
        let rejected = src.cast::<u8>().add(P_STATE_OFF).read() == STATE_REJECTED;
        let repr = src.cast::<u8>().add(P_REPR_OFF).read();
        let value = (src.cast::<u8>().add(P_VALUE_OFF) as *const i64).read();
        let settled = box_settled(repr, value);
        let (env, entry) = if rejected {
            ((*a).on_err, (*a).on_err_entry)
        } else {
            ((*a).on_ok, (*a).on_ok_entry)
        };
        let (settle_with, payload, owned) = if env.is_null() {
            let f = if rejected { (*a).reject } else { (*a).resolve };
            (f, settled, false)
        } else {
            let argv = [settled];
            let ret = invoke_boxed(env, entry, argv.as_ptr(), 1);
            if __torajs_throw_check() != 0 {
                __torajs_anyv_rc_dec(ret);
                let ttag = __torajs_throw_take_tag();
                let tval = __torajs_throw_take();
                ((*a).reject, __torajs_anyv_box_from_pair(ttag, tval), true)
            } else {
                ((*a).resolve, ret, true)
            }
        };
        let argv = [payload];
        __torajs_anyv_rc_dec(__torajs_any_call(settle_with, argv.as_ptr(), 1));
        if owned {
            __torajs_anyv_rc_dec(payload);
        }
        __torajs_promise_drop(src);
        for env in [(*a).on_ok, (*a).on_err] {
            if !env.is_null() {
                __torajs_value_drop_heap(env);
            }
        }
        __torajs_anyv_rc_dec((*a).resolve);
        __torajs_anyv_rc_dec((*a).reject);
        free(a as *mut c_void);
    }
}
