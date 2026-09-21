//! §7.2.3 for a callback handed to a loop that speaks only the
//! boxed dual ABI.
//!
//! Every higher-order kernel takes its callback as a raw
//! `(env, entry)` pair and calls `entry` per element, because that
//! is the one calling convention a tight loop can use without
//! asking any questions. The pair comes from `closure_boxed_entry`,
//! which is a closure-cell probe — so the gate in front of each
//! loop was answering "is this a closure?" where the spec asks "is
//! this callable?". The other two shapes §7.2.3 names, a callable
//! Proxy and a class object, have no entry of their own and were
//! turned away: `[1, 2].map(new Proxy(fn, {}))` said "value is not
//! a function" about a value `typeof` calls a function.
//!
//! Swapping the gate's predicate would not help — the loop would
//! then get a null entry and jump to 0. What the loop needs is a
//! pair, so a shape without one gets a thunk: a bound cell with no
//! partial arguments, whose dual entry performs §10.4.1.1 step 5's
//! ordinary call on the target. The loop cannot tell the
//! difference, and never learns there is one.
//!
//! The fast path is first and unchanged: a real closure answers its
//! own pair, so nothing is minted and no loop body moves. Only a
//! shape that has no pair pays the one allocation, once per call,
//! outside the loop.

use core::ffi::c_void;

use crate::method_call_closure_dispatch::closure_boxed_entry;
use crate::nanbox::{AnyValue, as_void_ptr};

unsafe extern "C" {
    fn __torajs_value_drop_heap(p: *mut c_void);
}

/// A callback resolved to the `(env, entry)` pair a boxed-ABI loop
/// calls, plus ownership of the thunk if one had to be minted.
/// Drop releases it, so the pair is valid for exactly as long as
/// this value is held — keep it alive across the whole loop.
pub(crate) struct Callback {
    env: *mut c_void,
    entry: u64,
    /// NULL for a real closure: nothing was minted, and `env`
    /// belongs to the caller's argument.
    thunk: *mut c_void,
}

impl Callback {
    pub(crate) fn env(&self) -> *mut c_void {
        self.env
    }

    pub(crate) fn entry(&self) -> u64 {
        self.entry
    }
}

impl Drop for Callback {
    fn drop(&mut self) {
        if !self.thunk.is_null() {
            // SAFETY: minted by `resolve` below with rc 1 and never
            // handed to anything that takes a stake in it.
            unsafe { __torajs_value_drop_heap(self.thunk) };
        }
    }
}

/// §7.2.3 over `av`, answered as a callable pair. `this_arg` is the
/// receiver the spec says to call the callback with; it only has to
/// be carried when a thunk is minted, because a loop already routes
/// it to a real closure through the receiver channel.
///
/// `None` means not callable — each caller keeps its own TypeError,
/// which the spec spells differently per method.
///
/// # Safety
/// `av` and `this_arg` are valid AnyValues the caller keeps alive
/// across the resulting `Callback`'s lifetime.
pub(crate) unsafe fn resolve(av: AnyValue, this_arg: AnyValue) -> Option<Callback> {
    // The closure path, first and free: one tag compare and one
    // load, exactly what the gate did before this module existed.
    if let Some((env, entry)) = unsafe { closure_boxed_entry(av) } {
        return Some(Callback {
            env,
            entry,
            thunk: core::ptr::null_mut(),
        });
    }
    let thunk = unsafe { crate::method_bind::mint::callback_thunk(av, this_arg) }?;
    let p = as_void_ptr(thunk);
    // SAFETY: a freshly minted bound cell always carries its dual
    // entry, so this probe cannot miss.
    let (env, entry) = unsafe { closure_boxed_entry(thunk) }?;
    Some(Callback {
        env,
        entry,
        thunk: p,
    })
}
