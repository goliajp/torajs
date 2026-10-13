//! Whether user code has added a key to %Object.prototype%.
//!
//! A struct cell that lacks a declared member (an object literal left
//! it out, `delete` took it) and carries no user [[Prototype]] inherits
//! from %Object.prototype% alone, so `"a" in o` on it asks whether that
//! one object owns `a`. The typed `in` reads the answer inline off the
//! word below (`GlobalRef` + `Load`, like the throw flag) and skips the
//! chain walk while the word is down.

use core::sync::atomic::{AtomicI64, Ordering};

/// Non-zero once a key was inserted into the %Object.prototype%
/// singleton by anything but its mint: torajs-dynobj raises it from
/// the entry insert of a dict marked `DYNOBJ_HDR_FLAG_OBJECT_PROTO`,
/// and the mint marks the dict after its own `__proto__` entry lands.
/// Monotonic: a key deleted again leaves it up. An `AtomicI64` so the
/// inline load is the one word shape every flag read emits.
#[unsafe(no_mangle)]
pub static __torajs_object_proto_touched: AtomicI64 = AtomicI64::new(0);

/// torajs-dynobj's seam onto [`__torajs_object_proto_touched`]: it
/// keeps zero Cargo deps and reaches torajs-rc across the staticlib
/// boundary.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_object_proto_touch() {
    __torajs_object_proto_touched.store(1, Ordering::Release);
}
