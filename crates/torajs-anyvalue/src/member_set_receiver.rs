//! §10.1.9.2 OrdinarySet with the two objects pulled apart — the one
//! place in the language where the object whose property table
//! decides the write is NOT the object the write lands on.
//!
//! Every ordinary assignment is `O.[[Set]](P, V, O)`: the lookup and
//! the write share one object, which is what
//! [`crate::member_set::__torajs_any_member_set`] assumes throughout.
//! `Reflect.set(target, key, V, receiver)` (§28.1.13 step 4) and a
//! `super.x = v` SuperProperty write (§13.3.7 — base is the home
//! object's prototype, receiver is `this`) both spell the four-argument
//! form, and there the lookup walks the TARGET while the write and any
//! setter's `this` go to the RECEIVER.
//!
//! The split costs almost nothing here because
//! [`crate::member_set_dynobj`]'s chain walk already took its receiver
//! as a parameter — it needed one for the ordinary inherited-accessor
//! case (`obj.x = v` where `x` is a setter on the prototype runs with
//! `obj` as `this`). Seeding that walk at the target itself instead of
//! at the target's prototype is the whole difference.

use core::ffi::c_void;

use crate::nanbox::{AnyValue, is_cell};

/// §10.1.9.2 with an explicit receiver. Answers the spec boolean as
/// `1` / `0` — a refusal never throws here, which is the flavor both
/// callers want (`Reflect.set` reports it, a strict SuperProperty
/// write raises its own TypeError from the answer).
///
/// `recv_slot` is a slot rather than a value so a DynObj receiver that
/// relocates on grow writes its fresh cell back to the caller's
/// binding — the same 7d-A contract
/// [`crate::member_set::__torajs_any_member_set_soft`] carries.
///
/// Boundary (recorded, not silently wrong): the walk understands a
/// dynobj chain, which is every object literal, every `{}`, and every
/// prototype reached through one. A target of another shape — a class
/// instance, an array — takes the walk's own-create fall-through, so
/// the write still lands on the receiver (the half this kernel exists
/// for) while a class-prototype accessor on the target goes
/// unconsulted. That is the same boundary the walk already records
/// when it meets a non-dynobj level part-way up a chain.
///
/// # Safety
/// `target` is an AnyValue; `key` is a live key cell; `(tag, value)`
/// carries the caller's +1 on heap payloads; `recv_slot` points at a
/// live AnyValue slot the caller owns.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_member_set_with_receiver(
    target: u64,
    key: *mut c_void,
    tag: u64,
    value: u64,
    recv_slot: *mut AnyValue,
) -> i64 {
    unsafe {
        let receiver = *recv_slot;
        // The receiver defaults to the target in both callers'
        // shorter spellings, and a caller may hand back the same
        // object explicitly. That collapses to an ordinary [[Set]],
        // so take the path the rest of the language takes rather than
        // a second implementation of it.
        if receiver == target {
            return crate::member_set::__torajs_any_member_set_soft(recv_slot, key, tag, value, -1);
        }
        // §10.1.9.2 step 2.b — a primitive receiver has nowhere to
        // put the property, and the answer is a plain `false`, not a
        // throw. The caller's payload stake dies here.
        if !is_cell(receiver) {
            crate::member_set::drop_payload(tag, value);
            return 0;
        }
        if is_cell(target)
            && let Some(verdict) =
                crate::member_set_dynobj::chain_set_from_self(target, receiver, key, tag, value)
        {
            return verdict;
        }
        // Fall-through = step 2.e CreateDataProperty on the receiver.
        // Spelled as the receiver's own soft [[Set]] so an own
        // getter-only or non-writable entry there still refuses,
        // which is what steps 2.d.i-ii ask for.
        crate::member_set::__torajs_any_member_set_soft(recv_slot, key, tag, value, -1)
    }
}
