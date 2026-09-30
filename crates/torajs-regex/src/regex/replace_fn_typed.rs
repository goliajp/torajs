//! The typed lanes' invoke strategy for the replace-callback walk
//! (`super::replace_fn`): build the callback's declared capture Strs
//! and transmute-dispatch through
//! [`super::replace_fn_dispatch::invoke_replace_cb`].

use core::ffi::c_void;

use super::replace_fn_dispatch::invoke_replace_cb;
use super::{__torajs_str_drop, __torajs_str_undef, str_from_bytes};
use crate::vm::save_slot;

unsafe extern "C" {
    /// torajs-throw — non-zero iff a pending throw is recorded.
    fn __torajs_throw_check() -> i64;
}

/// Build N capture Strs from saves[]. Each cap slot reads
/// `saves[2*(i+1)] / saves[2*(i+1)+1]` (group 0 = whole match is
/// handled separately).
///
/// A group that did not participate is `undefined` per §22.2.6.11
/// step 14.g, and it reaches the callback as the immortal undefined
/// sentinel — the same cell the `match` / `exec` array lanes already
/// push for exactly this case. The empty Str this used to build made
/// `"xz".replace(/x(y)?(z)/, (m, p1) => "<" + p1 + ">")` answer
/// `<>` where every engine answers `<undefined>`, silently. (The
/// `$1` expansion in the string-replacement lane is NOT this case:
/// §22.2.6.11's GetSubstitution really does substitute "" for an
/// undefined capture.)
///
/// Caller owns the returned Strs and must drop them; the sentinel
/// carries `FLAG_STATIC_LITERAL`, so its drop is a no-op.
///
/// # Safety
///
/// `s` must outlive the returned pointers; `out_caps` is sized for
/// at least `n_caps` entries (max 9).
unsafe fn build_capture_strs(
    n_caps: i64,
    saves: &[i64],
    s: &[u8],
    out_caps: &mut [*mut c_void; 9],
) {
    for i in 0..(n_caps as usize) {
        let gs = save_slot(saves, 2 * (i + 1));
        let ge = save_slot(saves, 2 * (i + 1) + 1);
        let p = if gs < 0 || ge < 0 {
            unsafe { __torajs_str_undef() }
        } else {
            unsafe { str_from_bytes(&s[gs as usize..ge as usize]) }
        };
        out_caps[i] = p as *mut c_void;
    }
}

/// Build the callback's declared `n_caps` capture Strs and
/// transmute-dispatch through [`invoke_replace_cb`]. `Err` on a
/// callback that left a pending throw, which ends the walk the way
/// the boxed lane's does.
///
/// # Safety
///
/// `closure_env` is a live closure heap block (env+8 holds the cb fn
/// pointer); `m` is a live Str; `s` outlives the call.
#[allow(clippy::too_many_arguments)]
pub(super) unsafe fn typed_invoke(
    closure_env: *mut c_void,
    n_caps: i64,
    has_off_input: bool,
    str_ptr: *const c_void,
    m: *mut c_void,
    saves: &[i64],
    s: &[u8],
    off: i64,
) -> Result<*mut c_void, ()> {
    // Load fn_addr from env+8 — same closure ABI as
    // promise_then_closure.
    let fn_ptr = unsafe { *((closure_env as *mut u8).add(8) as *mut *mut c_void) };
    let mut caps: [*mut c_void; 9] = [core::ptr::null_mut(); 9];
    unsafe { build_capture_strs(n_caps, saves, s, &mut caps) };
    let ret_str = unsafe {
        invoke_replace_cb(
            n_caps,
            has_off_input,
            closure_env,
            fn_ptr,
            m,
            &caps,
            off,
            str_ptr as *mut c_void,
        )
    };
    for cap in caps.iter().take(n_caps as usize) {
        unsafe { __torajs_str_drop(*cap) };
    }
    // §22.2.6.11 step 14.j — a callback that throws ends the walk
    // right there. The typed lane used to let the pending throw ride
    // all the way to the caller's check after the WHOLE walk, with
    // three visible consequences: the remaining matches still ran the
    // callback (`"axbxc".replace(/(y)?x/g, cb)` called it twice where
    // the spec calls it once), the throwing replace's result was
    // spliced and returned, and the throw surfaced so late that the
    // `try` around the replace no longer covered it — it exited the
    // program uncaught. A throw the callback raised itself was always
    // caught; only one the KERNEL raised inside it (a method-call
    // guard on the `string | undefined` an unmatched capture group
    // binds) took this path, which is why it read as a regex bug.
    //
    // `ret_str` is deliberately not released here, matching the boxed
    // lane: on the throw path the callback never returned a value to
    // own.
    if unsafe { __torajs_throw_check() } != 0 {
        return Err(());
    }
    Ok(ret_str)
}
