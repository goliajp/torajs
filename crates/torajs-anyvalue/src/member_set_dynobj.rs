//! `Tag::DynObj` receiver arm of [`crate::member_set`] — the
//! §10.1.9.2 OrdinarySet inherited-chain walk and the ordinary
//! own-entry write (Annex B `__proto__` setter route included).
//! Split out of `member_set.rs` (rotation 354 — the Promise arm
//! pushed the parent over the 500-line file cap; mechanical move,
//! bodies verbatim; the parent re-exports [`inherited_set_handled`]
//! so `member_set_symbol`'s import path stays canonical).

use core::ffi::c_void;

use crate::member_set::{
    __torajs_dynobj_has, __torajs_throw_type_error, drop_payload, dynobj_set_flavored,
};
use crate::nanbox::AnyValue;
use crate::nanbox_encode::__torajs_anyv_box_from_pair;
use torajs_rc::Tag;

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
    /// torajs-meta — borrow read of `PROTOS_BY_TAG_IMM[tag]` (the
    /// struct seed's chain root; 0 = unregistered tag).
    fn __torajs_proto_cell_raw(tag: i64) -> u64;
}

/// `DYNOBJ_HDR_FLAG_NULL_PROTO` mirror (torajs-dynobj layout, header
/// flag bit 6) — the null-proto shape has no inherited `__proto__`
/// setter.
const DYNOBJ_HDR_FLAG_NULL_PROTO: u16 = 1 << 6;

/// `dynobj_get_tag` accessor sentinel
/// (`torajs_dynobj::layout::ANY_ACCESSOR` mirror).
const MEMBER_SET_ANY_ACCESSOR: u64 = 6;

/// §10.1.9.2 OrdinarySet — an own miss consults the user
/// [[Prototype]] chain (RFC 20260721 候补刀): an inherited accessor
/// writes through its setter with the ORIGINAL receiver; an inherited
/// non-writable data property rejects; a writable (or absent) chain
/// answer falls through to the caller's ordinary own create. Returns
/// `Some(1)` when the setter ran, `Some(0)` when the chain refused
/// (flavored — the strict flavor records the TypeError first), and
/// `None` for the own-create fall-through. The common fresh create
/// on an implicit-chain dynobj pays one own-has probe plus one
/// interned proto-slot lookup.
///
/// Key-kind agnostic, which is why both the string lane and the
/// §6.1.7 symbol lane in [`crate::member_set_symbol`] share it —
/// OrdinarySet does not care how the key is spelled.
///
/// # Safety
/// `ptr` is a live `Tag::DynObj` cell that `recv` boxes; `key` is a
/// live key cell; `(tag, value)` carries the caller's +1 on heap
/// payloads.
pub(crate) unsafe fn inherited_set_handled(
    ptr: *mut c_void,
    recv: AnyValue,
    key: *mut c_void,
    tag: u64,
    value: u64,
    throw_on_refusal: bool,
) -> Option<i64> {
    unsafe {
        let level = crate::member_get_own::user_proto_cell(ptr);
        inherited_set_walk(level, recv, key, tag, value, throw_on_refusal)
    }
}

/// Rotation 441 (3c) — the STRUCT receiver's seed of the same
/// §10.1.9.2 walk. A struct cell carries no user [[Prototype]] slot;
/// its chain root is the class prototype (`__proto_<C>`, where
/// runtime-computed accessors reify). Pre-entry a keyed write whose
/// key a proto AccessorPair owned fell straight to the +24 expando
/// create — the own entry then shadowed the getter, so
/// `c[k] = v; c[k]` answered `v` instead of the getter's result.
///
/// # Safety
/// `ptr` is a live `Tag::Obj` cell that `recv` boxes; `key` is a live
/// key cell; `(tag, value)` carries the caller's +1 on heap payloads.
pub(crate) unsafe fn inherited_set_from_class_proto(
    ptr: *mut c_void,
    recv: AnyValue,
    key: *mut c_void,
    tag: u64,
    value: u64,
    throw_on_refusal: bool,
) -> Option<i64> {
    unsafe {
        let class_tag = ptr.cast::<u8>().add(8).cast::<u32>().read();
        // Borrow read of the registry slot (process-lifetime); 0 =
        // unregistered tag, no chain.
        let root = __torajs_proto_cell_raw(class_tag as i64);
        if !crate::nanbox::is_cell(root) {
            return None;
        }
        inherited_set_walk(Some(root), recv, key, tag, value, throw_on_refusal)
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

/// The walk both seeds share — see [`inherited_set_handled`] for the
/// verdict contract.
///
/// # Safety
/// `level` is `None` or a live cell; the rest per the seeds.
unsafe fn inherited_set_walk(
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
            let cptr = cell as *const c_void;
            if (cptr.cast::<u8>().add(4) as *const u16).read() != Tag::DynObj as u16 {
                // A struct parent keeps the own-create fall-through
                // (its accessor face is a recorded boundary).
                break;
            }
            if __torajs_dynobj_has(cptr, key as *const c_void) != 0 {
                let etag = __torajs_dynobj_get_tag(cptr, key as *const c_void);
                if etag == MEMBER_SET_ANY_ACCESSOR {
                    let pair =
                        __torajs_dynobj_get_value(cptr, key as *const c_void) as *const c_void;
                    let value_anyv = __torajs_anyv_box_from_pair(tag as i64, value as i64);
                    // The setter consumes the value stake (the arr
                    // accessor arm's ledger); a getter-only pair
                    // refuses the strict assignment.
                    if __torajs_accessor_invoke_setter(pair, recv, value_anyv) == 0 {
                        if throw_on_refusal {
                            __torajs_throw_type_error(
                                c"Attempted to assign to readonly property.".as_ptr(),
                            );
                        }
                        return Some(0);
                    }
                    return Some(1);
                }
                if __torajs_dynobj_get_flags(cptr, key as *const c_void) & 0x1 == 0 {
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
            level = crate::member_get_own::user_proto_cell(cptr);
        }
    }
    None
}

/// `Tag::DynObj` receiver — the ordinary own-entry write, plus the
/// Annex B §B.2.2.1 `__proto__` setter route.
pub(crate) unsafe fn set_dynobj_member(
    recv_slot: *mut AnyValue,
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
        let hdr_flags = (ptr.cast::<u8>().add(6) as *const u16).read();
        let has_own = __torajs_dynobj_has(ptr, key as *const c_void) != 0;
        // Function.prototype's virtual own name/length pair is
        // {writable: false} (§20.2.3, RFC 20260722 刀 3) — while no
        // own entry shadows it (a defineProperty recreate may), the
        // module-strict assign throws. Tombstoned = absent, so the
        // write walks on and creates an ordinary entry.
        if !has_own && crate::method_support_proto_meta::builtin_proto_own_meta(ptr, key).is_some()
        {
            drop_payload(tag, value);
            if throw_on_refusal {
                __torajs_throw_type_error(c"Attempted to assign to readonly property.".as_ptr());
            }
            return 0;
        }
        if hdr_flags & DYNOBJ_HDR_FLAG_NULL_PROTO == 0
            && !has_own
            && crate::prop_has::key_is(key, b"__proto__")
        {
            let boxed = __torajs_anyv_box_from_pair(tag as i64, value as i64);
            __torajs_anyv_proto_member_set(recv, boxed);
            // The setter takes its own stake on the stored cell;
            // the caller's transferred reference dies here.
            drop_payload(tag, value);
            return 1;
        }
        if !has_own
            && let Some(handled) =
                inherited_set_handled(ptr, recv, key, tag, value, throw_on_refusal)
        {
            return handled;
        }
        let mut obj = ptr;
        let wrote = dynobj_set_flavored(&mut obj, key, tag, value, throw_on_refusal);
        if obj != ptr {
            // Relocated block — the NaN-box cell encoding is the
            // pointer bits; transfer, no rc traffic (same object
            // identity, moved storage).
            *recv_slot = __torajs_anyv_box_from_pair(4, obj as i64);
        }
        wrote
    }
}
