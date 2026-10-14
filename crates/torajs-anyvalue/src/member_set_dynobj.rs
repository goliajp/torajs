//! `Tag::DynObj` receiver arm of [`crate::member_set`] — the
//! §10.1.9.2 OrdinarySet inherited-chain walk and the ordinary
//! own-entry write (Annex B `__proto__` setter route included).
//! Split out of `member_set.rs` (rotation 354 — the Promise arm
//! pushed the parent over the 500-line file cap; mechanical move,
//! bodies verbatim; the parent re-exports [`inherited_set_handled`]
//! so `member_set_symbol`'s import path stays canonical).

use core::ffi::c_void;

use crate::nanbox::as_void_ptr;

use crate::member_set::{
    __torajs_dynobj_has, __torajs_throw_type_error, drop_payload, dynobj_set_flavored,
};
use crate::nanbox::AnyValue;
use crate::nanbox_encode::__torajs_anyv_box_from_pair;

unsafe extern "C" {
    /// torajs-meta — the Annex B `__proto__` setter core (knife 3).
    fn __torajs_anyv_proto_member_set(obj: u64, proto: u64);
    /// torajs-dynobj — setter dispatch; `0` = getter-only accessor.
    fn __torajs_accessor_invoke_setter(pair: *const c_void, recv_anyv: u64, value_anyv: u64)
    -> i32;
    /// torajs-dynobj — chain-entry probes for the §10.1.9.2 walk
    /// (tag distinguishes an AccessorPair entry; flags carry the
    /// packed W/E/C bits, bit 0 = writable).
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_flags(obj: *const c_void, key: *const c_void) -> u64;
    /// torajs-dynobj — the write into an own entry in one probe;
    /// [`DYNOBJ_SET_OWN_MISS`] when the key has none (nothing written).
    fn __torajs_dynobj_set_own(
        obj: *mut c_void,
        key: *mut c_void,
        tag: u64,
        value: u64,
        throw_on_refusal: bool,
    ) -> i64;
}

/// `torajs_dynobj::set::DYNOBJ_SET_OWN_MISS` mirror.
const DYNOBJ_SET_OWN_MISS: i64 = -1;

/// `DYNOBJ_HDR_FLAG_NULL_PROTO` mirror (torajs-dynobj layout, header
/// flag bit 6) — the null-proto shape has no inherited `__proto__`
/// setter.
const DYNOBJ_HDR_FLAG_NULL_PROTO: u16 = 1 << 6;

/// `dynobj_get_tag` accessor sentinel
/// (`torajs_dynobj::layout::ANY_ACCESSOR` mirror).
const MEMBER_SET_ANY_ACCESSOR: u64 = 6;

/// §10.1.9.2 OrdinarySet — an own miss consults the [[Prototype]]
/// chain: an inherited accessor writes through its setter with the
/// ORIGINAL receiver; an inherited non-writable data property
/// rejects; a writable (or absent) chain answer falls through to the
/// caller's ordinary own create. Returns `Some(1)` when the setter
/// ran, `Some(0)` when the chain refused (flavored — the strict
/// flavor records the TypeError first), and `None` for the
/// own-create fall-through.
///
/// Receiver-shape agnostic, and that is the point: what a receiver's
/// [[Prototype]] is has ONE answer
/// ([`crate::member_set_proto_root::chain_parent`]), so a dynobj, a
/// closure, an array, a struct and a wrapper all reach the chain
/// through this call rather than each lane deciding for itself
/// whether it has one. Key-kind agnostic too — the string lane and
/// the §6.1.7 symbol lane in [`crate::member_set_symbol`] share it,
/// because OrdinarySet does not care how the key is spelled.
///
/// The consult belongs BEFORE the receiver's own integrity gate, not
/// after: a frozen receiver still runs an inherited setter (bun
/// agrees), because freezing bars the own CREATE, not the walk.
///
/// The common fresh create on an implicit-chain dynobj pays one
/// own-has probe plus one interned proto-slot lookup, and then the
/// %Object.prototype% link.
///
/// # Safety
/// `ptr` is a live cell that `recv` boxes; `key` is a live key cell;
/// `(tag, value)` carries the caller's +1 on heap payloads.
pub(crate) unsafe fn inherited_set_handled(
    ptr: *mut c_void,
    recv: AnyValue,
    key: *mut c_void,
    tag: u64,
    value: u64,
    throw_on_refusal: bool,
) -> Option<i64> {
    unsafe {
        let level = crate::member_set_proto_root::chain_parent(ptr as u64);
        inherited_set_walk(level, recv, key, tag, value, throw_on_refusal)
    }
}

/// §28.1.13 Reflect.set's seed — the walk starting at the target
/// ITSELF rather than at its prototype, with a receiver that need not
/// be the target. Reflect.set is spelled `target.[[Set]](P, V,
/// receiver)`, so the property lookup that decides between "run a
/// setter" and "write a data property" walks the TARGET (own entry
/// first), while the write and the setter's `this` both go to the
/// RECEIVER — the one place in §10.1.9.2 where the two objects come
/// apart. The two-seed shape is why that split costs nothing here:
/// the walk already took its receiver as a parameter.
///
/// Verdict contract as [`inherited_set_handled`]: `Some(1)` a setter
/// ran, `Some(0)` refused, `None` = the caller writes an ordinary own
/// data property — on the receiver, per §10.1.9.2 step 2.e.
///
/// # Safety
/// `cell` is a live object cell; `key` is a live key cell; `(tag,
/// value)` carries the caller's +1 on heap payloads.
pub(crate) unsafe fn chain_set_from_self(
    cell: u64,
    recv: AnyValue,
    key: *mut c_void,
    tag: u64,
    value: u64,
) -> Option<i64> {
    unsafe { inherited_set_walk(Some(cell), recv, key, tag, value, false) }
}

/// The walk the seeds share — see [`inherited_set_handled`] for the
/// verdict contract. A lane that keeps its own entries in a side bag (a
/// function's `+24` props) seeds it at the cell itself, so an own
/// accessor runs its setter with the CELL as `this`; a plain write into
/// the bag would make the bag the receiver.
///
/// # Safety
/// `level` is `None` or a live cell; the rest per the seeds.
pub(crate) unsafe fn inherited_set_walk(
    mut level: Option<u64>,
    recv: AnyValue,
    key: *mut c_void,
    tag: u64,
    value: u64,
    throw_on_refusal: bool,
) -> Option<i64> {
    unsafe {
        let mut depth = 0usize;
        while let Some(cell) = level {
            // Simulated-slot cycle guard (obj_forin_keys mirror).
            depth += 1;
            if depth > 64 {
                break;
            }
            // %Function.prototype% is a Closure (§20.2.3 makes it a
            // built-in FUNCTION object), and §10.2.4's
            // restricted-property accessors living there are exactly
            // what a receiver one link down has to write through:
            // `Object.create(Function.prototype).caller = {}` throws,
            // and a class constructor — whose [[Prototype]] link IS
            // that singleton — throws for the same reason.
            // A Proxy on the chain answers the rest of the walk:
            // §10.1.9.2 step 2.a hands it `[[Set]](P, V, Receiver)`.
            if crate::proxy::is_proxy(cell) {
                let r = crate::proxy_ops::set(as_void_ptr(cell), key, tag, value, recv);
                return Some(match r {
                    Ok(true) => 1,
                    Ok(false) => {
                        if throw_on_refusal {
                            __torajs_throw_type_error(
                                c"proxy 'set' trap returned falsish".as_ptr(),
                            );
                        }
                        0
                    }
                    Err(()) => 0,
                });
            }
            let Some(dict) = crate::member_set_proto_root::entry_dict(cell) else {
                // A shape whose property face this walk does not model
                // (a struct parent's accessors are a recorded
                // boundary) — the own-create fall-through stands.
                break;
            };
            if dict.is_null() {
                // Modeled but empty: nothing owns the key HERE, which
                // is not the same as the chain ending.
                level = crate::member_set_proto_root::chain_parent(cell);
                continue;
            }
            if __torajs_dynobj_has(dict, key as *const c_void) != 0 {
                let etag = __torajs_dynobj_get_tag(dict, key as *const c_void);
                if etag == MEMBER_SET_ANY_ACCESSOR {
                    let pair =
                        __torajs_dynobj_get_value(dict, key as *const c_void) as *const c_void;
                    let value_anyv = __torajs_anyv_box_from_pair(tag as i64, value as i64);
                    // The setter borrows the value, so its stake ends
                    // here either way; a getter-only pair refuses the
                    // strict assignment.
                    let ran = __torajs_accessor_invoke_setter(pair, recv, value_anyv);
                    drop_payload(tag, value);
                    if ran == 0 {
                        if throw_on_refusal {
                            __torajs_throw_type_error(
                                c"Attempted to assign to readonly property.".as_ptr(),
                            );
                        }
                        return Some(0);
                    }
                    return Some(1);
                }
                if __torajs_dynobj_get_flags(dict, key as *const c_void) & 0x1 == 0 {
                    drop_payload(tag, value);
                    if throw_on_refusal {
                        __torajs_throw_type_error(
                            c"Attempted to assign to readonly property.".as_ptr(),
                        );
                    }
                    return Some(0);
                }
                break;
            }
            level = crate::member_set_proto_root::chain_parent(cell);
        }
    }
    None
}

/// `Tag::DynObj` receiver — the ordinary own-entry write, plus the
/// Annex B §B.2.2.1 `__proto__` setter route.
pub(crate) unsafe fn set_dynobj_member(
    recv: AnyValue,
    ptr: *mut c_void,
    key: *mut c_void,
    tag: u64,
    value: u64,
    throw_on_refusal: bool,
) -> i64 {
    unsafe {
        // Annex B §B.2.2.1 — `o.__proto__ = v` runs the
        // inherited setter, not an ordinary entry write (RFC
        // 20260717-user-proto-chain knife 3): [[SetPrototypeOf]]
        // with the cycle walk; an invalid v is silently ignored.
        // The null-proto shape has no inherited setter — its
        // write IS an ordinary own entry (the dynobj_set below).
        // An own `__proto__` DATA entry (shorthand `{__proto__}`
        // / defineProperty) shadows the setter the same way
        // (§10.1.9.2 OrdinarySet finds the own data property
        // first) — its write stays ordinary too.
        // §10.1.9.2 step 2 — an own entry decides the write alone, so
        // the hit is one probe; everything below is the absent-key
        // path.
        let own = __torajs_dynobj_set_own(ptr, key, tag, value, throw_on_refusal);
        if own != DYNOBJ_SET_OWN_MISS {
            return own;
        }
        let hdr_flags = (ptr.cast::<u8>().add(6) as *const u16).read();
        // Function.prototype's virtual own name/length pair is
        // {writable: false} (§20.2.3, RFC 20260722 刀 3) — while no
        // own entry shadows it (a defineProperty recreate may), the
        // module-strict assign throws. Tombstoned = absent, so the
        // write walks on and creates an ordinary entry.
        if crate::method_support_proto_meta::builtin_proto_own_meta(ptr, key).is_some() {
            drop_payload(tag, value);
            if throw_on_refusal {
                __torajs_throw_type_error(c"Attempted to assign to readonly property.".as_ptr());
            }
            return 0;
        }
        if hdr_flags & DYNOBJ_HDR_FLAG_NULL_PROTO == 0 && crate::prop_has::key_is(key, b"__proto__")
        {
            let boxed = __torajs_anyv_box_from_pair(tag as i64, value as i64);
            __torajs_anyv_proto_member_set(recv, boxed);
            // The setter takes its own stake on the stored cell;
            // the caller's transferred reference dies here.
            drop_payload(tag, value);
            return 1;
        }
        if let Some(handled) = inherited_set_handled(ptr, recv, key, tag, value, throw_on_refusal) {
            return handled;
        }
        // A resize swaps the store inside the header cell, so the cell
        // never moves and the slot needs no write-back.
        let mut obj = ptr;
        dynobj_set_flavored(&mut obj, key, tag, value, throw_on_refusal)
    }
}
