//! Annex B §B.2.2.2-5 legacy accessor surface (RFC
//! 20260713-annexb-legacy-accessor) — `Object.prototype.
//! __defineGetter__` / `__defineSetter__` install one accessor face
//! through the dynobj define kernel (desc = `{ [face], enumerable:
//! true, configurable: true }`; the kernel's per-face present bits
//! merge onto an existing pair); `__lookupGetter__` /
//! `__lookupSetter__` answer the matching face's closure, walking
//! the `__proto__` simulated key when the own probe misses.
//!
//! Receiver coverage: DynObj is the substrate arm; a primitive
//! receiver follows §B.2.2.2 step 1 ToObject — the wrapper is
//! unobservable, so define no-ops and lookup answers undefined; the
//! other heap shapes (Arr / Closure / struct expandos) throw the
//! loud no-such TypeError (recorded boundary — extend with the
//! expando-props store when a case demands it).
//!
//! Argument ledger: argv slots are BORROWED (the pair takes its own
//! +1 on the face closure); the key temp is owned and dropped here;
//! lookup answers an OWNED closure box.

use core::ffi::c_void;

use torajs_rc::{ANY_METHOD_DEFINE_GETTER, ANY_METHOD_DEFINE_SETTER, ANY_METHOD_LOOKUP_GETTER};

use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell};
use crate::nanbox_encode::__torajs_anyv_box_pointer;

unsafe extern "C" {
    /// torajs-str — release a heap Str reference (the key temp).
    fn __torajs_str_drop(s: *mut c_void);
    /// torajs-str — fresh Str from raw bytes (the `__proto__` key).
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// torajs-dynobj — fresh `+1`-rc AccessorPair (faces transfer).
    fn __torajs_accessor_pair_new(get: *mut c_void, set: *mut c_void, kinds: u64) -> *mut c_void;
    fn __torajs_accessor_get_getter(pair: *const c_void) -> *mut c_void;
    fn __torajs_accessor_get_setter(pair: *const c_void) -> *mut c_void;
    /// torajs-dynobj — define kernel (§10.1.6.3 apply core).
    fn __torajs_dynobj_define(
        obj_slot: *mut *mut c_void,
        key: *mut c_void,
        tag: u64,
        value: u64,
        flags_byte: u64,
    );
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
}

/// Flag-byte mirror of `torajs_dynobj::layout::DEFINE_*` — the
/// §B.2.2.2 descriptor is `{ enumerable: true, configurable: true }`
/// with one accessor face present.
const DEFINE_FLAG_ENUMERABLE: u64 = 1 << 1;
const DEFINE_FLAG_CONFIGURABLE: u64 = 1 << 2;
const DEFINE_PRESENT_ENUMERABLE: u64 = 1 << 4;
const DEFINE_PRESENT_CONFIGURABLE: u64 = 1 << 5;
const DEFINE_PRESENT_VALUE: u64 = 1 << 6;
const DEFINE_PRESENT_GET: u64 = 1 << 7;
const DEFINE_PRESENT_SET: u64 = 1 << 8;

/// `kinds` mirror of `torajs_dynobj::accessor::ACC_KIND_BOXED` on
/// both faces — runtime closures ride the boxed dual entry (same
/// posture as the runtime-descriptor defineProperty path).
const ACC_KINDS_BOXED_BOTH: u64 = 5 | (5 << 8);

/// Kinds flag bit mirror of `torajs_dynobj::accessor::ACC_KIND_RECV` —
/// the face closure takes the call-site `this` in argv[0].
const ACC_KIND_RECV: u8 = 0x40;

/// Accessor-entry sentinel in the dynobj probe's tag channel —
/// mirror of `method_call_dynobj.rs::ANY_ACCESSOR_TAG`.
const ANY_ACCESSOR_TAG: u64 = 6;

/// `__proto__` walk cap — mirrors the proto-chain traversal bound
/// used across the reflection surfaces (a cyclic chain terminates).
const PROTO_WALK_CAP: u32 = 32;

/// Universal arm entry — `mid` is one of the four legacy accessor
/// mids. `recv` is non-nullish (dispatch guards). The key temp is
/// built from `argv[0]` per §7.1.19 ToPropertyKey.
pub(crate) unsafe fn legacy_accessor_method(
    recv: AnyValue,
    mid: i64,
    recv_slot: *mut u64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    let is_define = mid == ANY_METHOD_DEFINE_GETTER || mid == ANY_METHOD_DEFINE_SETTER;
    let wants_getter = mid == ANY_METHOD_DEFINE_GETTER || mid == ANY_METHOD_LOOKUP_GETTER;
    unsafe {
        // §B.2.2.2 step 2 — the face argument must be callable,
        // checked before the property key conversion and on every
        // receiver shape (a primitive receiver still throws here).
        if is_define {
            let face_av = if argc >= 2 {
                *argv.add(1)
            } else {
                VALUE_UNDEFINED
            };
            if !anyv_is_closure(face_av) {
                // bun's exact wording for cross-runtime parity.
                if wants_getter {
                    __torajs_throw_type_error(c"invalid getter usage".as_ptr());
                } else {
                    __torajs_throw_type_error(c"invalid setter usage".as_ptr());
                }
                return VALUE_UNDEFINED;
            }
        }
        if !is_cell(recv) {
            // §B.2.2.2 step 1 ToObject — a primitive wraps into an
            // unobservable temp: define lands on the discarded
            // wrapper, lookup misses it (its prototype carries no
            // user accessors).
            return VALUE_UNDEFINED;
        }
        let ptr = as_void_ptr(recv);
        let tag = (ptr.cast::<u8>().add(4) as *const u16).read();
        if tag != torajs_rc::Tag::DynObj as u16 {
            __torajs_throw_type_error(
                c"legacy accessor methods are not supported on this receiver.".as_ptr(),
            );
            return VALUE_UNDEFINED;
        }
        let key_av = if argc >= 1 { *argv } else { VALUE_UNDEFINED };
        let key = crate::nanbox_ffi::__torajs_anyv_to_str(key_av) as *mut c_void;
        let out = if is_define {
            let face = as_void_ptr(*argv.add(1));
            define_face(ptr, key, face, wants_getter, recv_slot)
        } else {
            lookup_face(ptr, key, wants_getter)
        };
        __torajs_str_drop(key);
        out
    }
}

/// `true` iff the box holds a live Closure cell.
unsafe fn anyv_is_closure(v: AnyValue) -> bool {
    if !is_cell(v) {
        return false;
    }
    let p = as_void_ptr(v);
    unsafe { (p.cast::<u8>().add(4) as *const u16).read() == torajs_rc::Tag::Closure as u16 }
}

/// §B.2.2.2/3 step 3-4 — build the one-face pair and route it
/// through the define kernel (`DefinePropertyOrThrow`; a pending
/// throw propagates through the dispatch site's check). The dynobj
/// may relocate on define (dense-array grow) — write the moved cell
/// back through `recv_slot`.
unsafe fn define_face(
    obj: *mut c_void,
    key: *mut c_void,
    face: *mut c_void,
    is_getter: bool,
    recv_slot: *mut u64,
) -> AnyValue {
    unsafe {
        // The pair takes ownership of one face ref; argv is a borrow.
        __torajs_rc_inc(face);
        let (g, s) = if is_getter {
            (face, core::ptr::null_mut())
        } else {
            (core::ptr::null_mut(), face)
        };
        // RFC 20260717-fnexpr-this-channel knife 1 — a fn-expr face
        // whose body says `this` carries FLAG_CLOSURE_RECV_FIRST on
        // its env header; mark the face's kinds byte ACC_KIND_RECV so
        // the pair invoke puts the receiver in argv[0].
        let face_flags = (face as *const u8).add(6).cast::<u16>().read();
        let mut kinds = ACC_KINDS_BOXED_BOTH;
        if face_flags & torajs_rc::FLAG_CLOSURE_RECV_FIRST != 0 {
            kinds |= if is_getter {
                ACC_KIND_RECV as u64
            } else {
                (ACC_KIND_RECV as u64) << 8
            };
        }
        let pair = __torajs_accessor_pair_new(g, s, kinds);
        let flags = DEFINE_PRESENT_VALUE
            | if is_getter {
                DEFINE_PRESENT_GET
            } else {
                DEFINE_PRESENT_SET
            }
            | DEFINE_PRESENT_ENUMERABLE
            | DEFINE_FLAG_ENUMERABLE
            | DEFINE_PRESENT_CONFIGURABLE
            | DEFINE_FLAG_CONFIGURABLE;
        let mut slot = obj;
        // ANY_HEAP tag 4 — the pair's +1 transfers into the entry.
        __torajs_dynobj_define(&mut slot, key, 4, pair as u64, flags);
        if slot != obj && !recv_slot.is_null() {
            *recv_slot = __torajs_anyv_box_pointer(slot);
        }
        VALUE_UNDEFINED
    }
}

/// §B.2.2.4/5 — own probe first, then the internal
/// [`crate::member_get_own::PROTO_SLOT_KEY`] simulated-slot chain;
/// an accessor entry answers its matching face (undefined when that
/// face is absent), a data entry answers undefined and stops the
/// walk (it shadows).
unsafe fn lookup_face(obj: *mut c_void, key: *mut c_void, wants_getter: bool) -> AnyValue {
    unsafe {
        let proto_key = __torajs_str_alloc(
            crate::member_get_own::PROTO_SLOT_KEY.as_ptr(),
            crate::member_get_own::PROTO_SLOT_KEY.len() as i64,
        );
        let mut cur = obj as *const c_void;
        let mut out = VALUE_UNDEFINED;
        let mut depth = 0u32;
        loop {
            if __torajs_dynobj_has(cur, key as *const c_void) != 0 {
                let dtag = __torajs_dynobj_get_tag(cur, key as *const c_void);
                if dtag == ANY_ACCESSOR_TAG {
                    let pair =
                        __torajs_dynobj_get_value(cur, key as *const c_void) as *const c_void;
                    let face = if wants_getter {
                        __torajs_accessor_get_getter(pair)
                    } else {
                        __torajs_accessor_get_setter(pair)
                    };
                    if !face.is_null() {
                        __torajs_rc_inc(face);
                        out = __torajs_anyv_box_pointer(face);
                    }
                }
                break;
            }
            depth += 1;
            if depth > PROTO_WALK_CAP || __torajs_dynobj_has(cur, proto_key as *const c_void) == 0 {
                break;
            }
            let ptag = __torajs_dynobj_get_tag(cur, proto_key as *const c_void);
            let pval = __torajs_dynobj_get_value(cur, proto_key as *const c_void);
            // ANY_HEAP DynObj links continue the chain; a null /
            // non-object proto ends it.
            if ptag != 4 || pval == 0 {
                break;
            }
            let pp = pval as *const c_void;
            if (pp.cast::<u8>().add(4) as *const u16).read() != torajs_rc::Tag::DynObj as u16 {
                break;
            }
            cur = pp;
        }
        __torajs_str_drop(proto_key as *mut c_void);
        out
    }
}
