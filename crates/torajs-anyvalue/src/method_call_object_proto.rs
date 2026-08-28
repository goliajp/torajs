//! The inherited `Object.prototype` surface arms that dispatch on
//! EVERY receiver shape — the §20.1.4.3/.5 universal own-property
//! probes and the §20.1.3.6 toString badge classifier (split from
//! `method_call.rs`, file-size limit; RFC
//! 20260713-array-proto-residual blade 2 added the badge).

use core::ffi::c_void;

use torajs_rc::{ANY_METHOD_HAS_OWN_PROPERTY, AnySlotTag, Tag};

use crate::nanbox::{
    AnyValue, VALUE_UNDEFINED, is_bool, is_double, is_int32, is_null, is_short_str, is_undefined,
};
use crate::nanbox_encode::__torajs_anyv_box_pointer;

unsafe extern "C" {
    /// torajs-buffer — the element-kind discriminant behind a typed
    /// array's `@@toStringTag`.
    fn __torajs_typedarray_kind(av: u64) -> i64;
    /// torajs-meta — the [[Prototype]] answer for any AnyValue
    /// (owned cell / null immediate) — knife 4's chain step.
    fn __torajs_anyv_get_proto_of_any(v: u64) -> u64;
    /// torajs-dynobj — own-property probe pair ((5, 0) = absent);
    /// the borrowed Array-toString `Get(this, "join")` step.
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
    /// torajs-str — allocate a fresh Str from raw bytes.
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    /// torajs-str — release a heap Str/Substr reference.
    fn __torajs_str_drop(s: *mut c_void);
    /// torajs-meta — Error.prototype.toString (§20.5.3.4): render
    /// `name: message` from a FLAG_ERROR OBJ instance pointer.
    fn __torajs_error_to_string(p: *const u8) -> *mut u8;
    /// torajs-throw — pending-throw probe (override invoke abort).
    fn __torajs_throw_check() -> i64;
    /// torajs-throw — typed-tier non-string override boundary.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// chunk D-1 — `hasOwnProperty` / `propertyIsEnumerable` universal
/// arm: ToPropertyKey the first argument, probe the prop_has /
/// prop_enumerable substrate, answer a Bool box.
///
/// §7.1.19 ToPropertyKey step 2 returns a Symbol key as-is; only the
/// remaining shapes reach step 3's ToString (a missing slot
/// stringifies undefined). Coercing a symbol would raise §7.1.17's
/// "cannot convert a Symbol to a string" on a call that must simply
/// answer whether the slot is there. A string key temp is owned and
/// dropped here; a symbol key is the caller's argv borrow.
pub(crate) unsafe fn own_prop_probe(
    recv: AnyValue,
    mid: i64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    let key_av = if argc >= 1 {
        unsafe { *argv }
    } else {
        VALUE_UNDEFINED
    };
    let sym_key = unsafe { symbol_key_cell(key_av) };
    let key = match sym_key {
        Some(cell) => cell,
        None => unsafe { crate::nanbox_ffi::__torajs_anyv_to_str(key_av) as *const c_void },
    };
    let hit = if mid == ANY_METHOD_HAS_OWN_PROPERTY {
        unsafe { crate::prop_has::__torajs_any_prop_has(recv, key) }
    } else {
        unsafe { crate::prop_enumerable::__torajs_any_prop_enumerable(recv, key) }
    };
    if sym_key.is_none() {
        unsafe { __torajs_str_drop(key as *mut c_void) };
    }
    if hit != 0 {
        crate::nanbox::VALUE_TRUE
    } else {
        crate::nanbox::VALUE_FALSE
    }
}

/// The argument as a borrowed `Tag::Symbol` key cell, or `None` when it
/// is any other value (which §7.1.19 step 3 sends through ToString).
///
/// # Safety
/// `v` carries a valid AnyValue bit pattern.
unsafe fn symbol_key_cell(v: AnyValue) -> Option<*const c_void> {
    let (ptr, t) = crate::member_get::recv_cell(v)?;
    if t == Tag::Symbol as u16 {
        Some(ptr as *const c_void)
    } else {
        None
    }
}

/// §20.1.3.6 `Object.prototype.toString` — classify the this-value
/// into its "[object X]" badge. Steps 1-2 answer Undefined / Null
/// without ToObject; the builtinTag walk maps each cell tag onto
/// the legacy badge set (Array / Function / Error / Boolean /
/// Number / String / Date / RegExp) plus the well-known
/// `Symbol.toStringTag` surfaces bun answers for the container
/// tags (Map / Set / Promise / Symbol / BigInt / WeakMap /
/// WeakSet / WeakRef). Everything else is "Object".
/// §23.1.3.36 — the reified `Array.prototype.toString` cell invoked
/// with an arbitrary receiver (RFC 20260721 刀 11 G12). An Array
/// receiver runs the ordinary join route (a re-dispatch under the
/// shared TO_STRING id — the method body never re-resolves own
/// shadows); any other receiver runs step 2's `Get(this, "join")` —
/// a dynobj's own callable `join` is invoked with the receiver,
/// everything else (no join / non-callable join) falls back to the
/// %Object.prototype.toString% badge below.
pub(crate) unsafe fn arr_to_string_borrowed(recv: AnyValue) -> AnyValue {
    unsafe {
        if let Some((ptr, tag)) = crate::member_get::recv_cell(recv) {
            if tag == Tag::Arr as u16 {
                return crate::method_call::any_method_redispatch(
                    recv,
                    torajs_rc::ANY_METHOD_TO_STRING,
                    core::ptr::null(),
                    0,
                );
            }
            if tag == Tag::DynObj as u16 {
                let key = {
                    let bytes = b"join";
                    let s = crate::__torajs_str_alloc_pooled(bytes.len() as u64);
                    core::ptr::copy_nonoverlapping(bytes.as_ptr(), s.add(16), bytes.len());
                    s
                };
                let jtag = __torajs_dynobj_get_tag(ptr, key as *const c_void);
                let jval = __torajs_dynobj_get_value(ptr, key as *const c_void);
                __torajs_str_drop(key as *mut c_void);
                // Only a Heap tag makes `jval` an address. Asking
                // merely that the slot not be `undefined` let every
                // other tag's PAYLOAD through as a pointer, so
                // `Array.prototype.toString.call({ join: true })`
                // dereferenced the boolean 1. Step 3 says a
                // non-callable join takes the badge, and a
                // non-cell can never be callable.
                if jtag == AnySlotTag::Heap as u64
                    && let Some((env, entry)) =
                        crate::method_call::closure_cell_entry(jval as *mut c_void)
                {
                    return crate::method_call::invoke_with_this(
                        env,
                        entry,
                        recv,
                        core::ptr::null(),
                        0,
                    );
                }
            }
        }
        object_proto_to_string(recv)
    }
}

/// §7.2.2 IsArray — `Err(())` when the proxy is revoked and step
/// 3.a's TypeError is in flight.
///
/// The predicate itself lives in `torajs_rc::is_array`, because
/// `Array.isArray` asks the same question and the two used to answer
/// it differently. Everything it has to know (the Proxy layout, the
/// arguments flag) is header data this crate does not own.
///
/// # Safety
/// `recv` carries a valid AnyValue bit pattern.
unsafe fn is_array_spec(recv: AnyValue) -> Result<bool, ()> {
    unsafe { torajs_rc::is_array::is_array_spec(recv as i64) }
}

pub(crate) unsafe fn object_proto_to_string(recv: AnyValue) -> AnyValue {
    // §20.1.3.6 step 3 — `IsArray(O)` runs BEFORE the @@toStringTag
    // Get, and it is the step that can throw first: a revoked proxy
    // takes its TypeError here rather than answering a badge.
    let proxy_array = if is_undefined(recv) || is_null(recv) {
        false
    } else {
        match unsafe { is_array_spec(recv) } {
            Err(()) => return VALUE_UNDEFINED,
            Ok(b) => b,
        }
    };
    // §20.1.3.6 steps 15-16 — the object gets to name itself, and that
    // name wins over every builtinTag below when it is a String. Steps
    // 1-2 answer before the Get, so undefined / null skip it.
    if !is_undefined(recv)
        && !is_null(recv)
        && let Some(tagged) = unsafe { crate::method_call_object_proto_tag::try_tag_badge(recv) }
    {
        return tagged;
    }
    let badge: &'static [u8] = if is_undefined(recv) {
        b"Undefined"
    } else if is_null(recv) {
        b"Null"
    } else if proxy_array {
        // Step 4 — the wrapped Array won; the tag walk below would
        // have said "Object" for the proxy cell itself.
        b"Array"
    } else if is_bool(recv) {
        b"Boolean"
    } else if is_int32(recv) || is_double(recv) {
        b"Number"
    } else if is_short_str(recv) {
        b"String"
    } else if let Some((ptr, tag)) = crate::member_get::recv_cell(recv) {
        unsafe { cell_badge(ptr, tag) }
    } else {
        b"Object"
    };
    unsafe { badge_string(badge) }
}

/// The badge a heap cell classifies into. Shared with the
/// `Object.prototype.toString` fallback a builtin prototype reaches
/// when nothing else claims the call.
///
/// # Safety
/// `ptr` is a live heap cell whose header tag is `tag`.
pub(crate) unsafe fn cell_badge(ptr: *mut c_void, tag: u16) -> &'static [u8] {
    // What remains here is builtinTag only — a prototype that IS an
    // instance of its own kind, which the cell tag alone cannot say
    // because tr mints these as dynobjs: `Number.prototype` has a
    // [[NumberData]] slot per §21.1.3, `Array.prototype` is an Array,
    // and so on down to `Function.prototype`'s [[Call]].
    //
    // The prototypes whose badge came from a well-known
    // `Symbol.toStringTag` instead (Symbol / BigInt / Promise / Map /
    // Set / the three weak collections) are NOT listed: they carry the
    // real property now, so step 15 answers for them before this walk
    // runs. Same for the four namespace singletons, which used to be
    // recognised here by pointer identity. Object / RegExp / Date /
    // Error are absent because the spec gives them no tag at all —
    // they keep "Object", matching bun.
    //
    // The INSTANCE walk below follows the same rule, and for the same
    // reason it now says less than it used to: a `Map` cell has no
    // builtinTag of its own either (steps 4-14 name only Array /
    // Arguments / Function / Error / Boolean / Number / String / Date
    // / RegExp), so it must reach its badge through the tag its
    // prototype carries. Emulating it here answered the same string
    // by a different route, and the two routes disagreed the moment
    // the property was deleted — the tag is configurable, and
    // `delete Map.prototype[Symbol.toStringTag]` left tr still saying
    // "[object Map]" where bun says "[object Object]"
    // (`proto_tostringtag_install`'s own module doc recorded this
    // survival when it installed the real properties).
    let proto_tag = unsafe { torajs_rc::builtin_proto::__torajs_builtin_proto_tag_of(ptr) };
    if proto_tag >= 0 {
        return match proto_tag {
            0 => b"Number",
            2 => b"Array",
            3 => b"String",
            4 => b"Boolean",
            13 => b"Function",
            _ => b"Object",
        };
    }
    match tag {
        t if t == Tag::Str as u16 => b"String",
        // RECORDED GAP (517-06) — §20.1.3.6 step 5 owes an arguments
        // object the "Arguments" badge, but tr mints it as an Arr
        // cell and FLAG_ARR_ARGUMENTS shares bit 1 with
        // FLAG_SPLIT_BLOCK on that same tag, so reading the bit
        // called `"a-b".split("-")` an arguments object. Until the
        // identity has a signal of its own, both answer "Array".
        t if t == Tag::Arr as u16 => b"Array",
        t if t == Tag::Closure as u16 => b"Function",
        // §20.1.3.6 step 6 — IsCallable answers the "Function" badge.
        // A class constructor is a dynobj carrying [[Call]] via
        // FLAG_DYNOBJ_CLASS_CTOR (420-06).
        t if t == Tag::DynObj as u16
            && unsafe { (ptr.cast::<u8>().add(6) as *const u16).read() }
                & torajs_rc::FLAG_DYNOBJ_CLASS_CTOR
                != 0 =>
        {
            b"Function"
        }
        t if t == Tag::Date as u16 => b"Date",
        t if t == Tag::RegExp as u16 => b"RegExp",
        // §23.2.3.32 — a typed array names itself through an
        // ACCESSOR on %TypedArray%.prototype, not the data property
        // the ten collections above carry, and tr's symbol-keyed
        // [[Get]] is a borrow-shaped slot read that never fires a
        // getter (`member_get_symbol::probe_dict`). So this one badge
        // is still emulated here, and it is the only one that has to
        // be. When the symbol accessor lane lands, this arm goes the
        // way of the other ten.
        t if t == Tag::TypedArray as u16 => unsafe {
            typedarray_badge(__torajs_typedarray_kind(crate::nanbox::box_void_ptr(ptr)))
        },
        t if t == Tag::Undefined as u16 => b"Undefined",
        // RFC 20260716 刀 2 — primitive-wrapper cells classify by what
        // they wrap.
        t if t == Tag::NumberWrapper as u16 => b"Number",
        t if t == Tag::StringWrapper as u16 => b"String",
        t if t == Tag::BooleanWrapper as u16 => b"Boolean",
        // Errors are static-layout structs carrying FLAG_ERROR
        // (disjoint-by-tag bit 7).
        t if t == Tag::Obj as u16 => {
            let flags = unsafe { (ptr.cast::<u8>().add(6) as *const u16).read() };
            if flags & torajs_rc::FLAG_ERROR != 0 {
                b"Error"
            } else {
                b"Object"
            }
        }
        _ => b"Object",
    }
}

/// §23.2.3.32 — the eleven `[[TypedArrayName]]`s, in the
/// `torajs_buffer::typedarray::Kind` discriminant order (wire
/// format; the substrate's `name()` is the same table on its side).
fn typedarray_badge(kind: i64) -> &'static [u8] {
    match kind {
        0 => b"Int8Array",
        1 => b"Uint8Array",
        2 => b"Uint8ClampedArray",
        3 => b"Int16Array",
        4 => b"Uint16Array",
        5 => b"Int32Array",
        6 => b"Uint32Array",
        7 => b"Float32Array",
        8 => b"Float64Array",
        9 => b"BigInt64Array",
        10 => b"BigUint64Array",
        11 => b"Float16Array",
        _ => b"Object",
    }
}

/// The badge a receiver reaching the `Object.prototype` fallback
/// answers with. A struct receiver has no badge of its own; every
/// cell classifies through [`cell_badge`].
///
/// # Safety
/// `obj` is a live heap cell.
pub(crate) unsafe fn cell_badge_string(obj: *mut c_void, is_struct: bool) -> AnyValue {
    // Same steps 15-16 as `object_proto_to_string` — this is the entry
    // a builtin prototype's own `toString()` reaches, and the tag it
    // carries is exactly what that call is asking about.
    if let Some(tagged) = unsafe {
        crate::method_call_object_proto_tag::try_tag_badge(crate::nanbox::box_void_ptr(obj))
    } {
        return tagged;
    }
    let badge: &'static [u8] = if is_struct {
        b"Object"
    } else {
        // HeapHeader: type_tag @ +4 (u16).
        let tag = unsafe { obj.cast::<u8>().add(4).cast::<u16>().read() };
        unsafe { cell_badge(obj, tag) }
    };
    unsafe { badge_string(badge) }
}

/// `"[object " + badge + "]"` as an owned Str box.
///
/// # Safety
/// `badge` is at most 9 bytes (every caller passes a literal).
unsafe fn badge_string(badge: &'static [u8]) -> AnyValue {
    let mut buf = [0u8; 24];
    buf[..8].copy_from_slice(b"[object ");
    buf[8..8 + badge.len()].copy_from_slice(badge);
    buf[8 + badge.len()] = b']';
    let len = 8 + badge.len() + 1;
    unsafe {
        let p = __torajs_str_alloc(buf.as_ptr(), len as i64);
        __torajs_anyv_box_pointer(p as *mut c_void)
    }
}

/// `Object.prototype.isPrototypeOf(V)` — §20.1.3.3 (RFC
/// 20260717-user-proto-chain knife 4): a primitive V is `false`
/// (step 1), BEFORE step 2's ToObject can reject the receiver — so
/// a null / undefined receiver only throws when V is an object (the
/// dispatcher routes this mid ahead of its nullish guard for exactly
/// that ordering). A primitive receiver is `false` — its ToObject
/// wrapper is minted fresh and can never sit on a chain. Each chain
/// step's answer is owned (the getter incs) and released as the walk
/// moves past it.
///
/// # Safety
/// `recv` carries a valid AnyValue bit pattern; `argv` points at
/// `argc` live AnyValue slots.
pub(crate) unsafe fn is_prototype_of(recv: AnyValue, argv: *const u64, argc: i64) -> AnyValue {
    let v = if argc >= 1 {
        unsafe { *argv }
    } else {
        VALUE_UNDEFINED
    };
    if !crate::nanbox::is_cell(v) {
        return crate::nanbox::VALUE_FALSE;
    }
    if is_null(recv) || is_undefined(recv) {
        unsafe {
            __torajs_throw_type_error(c"cannot call a method of null or undefined".as_ptr());
        }
        return VALUE_UNDEFINED;
    }
    if !crate::nanbox::is_cell(recv) {
        return crate::nanbox::VALUE_FALSE;
    }
    let target = crate::nanbox::as_void_ptr(recv);
    unsafe {
        let mut cur = __torajs_anyv_get_proto_of_any(v);
        loop {
            if !crate::nanbox::is_cell(cur) {
                return crate::nanbox::VALUE_FALSE;
            }
            if core::ptr::eq(crate::nanbox::as_void_ptr(cur), target) {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(cur);
                return crate::nanbox::VALUE_TRUE;
            }
            let next = __torajs_anyv_get_proto_of_any(cur);
            crate::nanbox_ffi::__torajs_anyv_rc_dec(cur);
            cur = next;
        }
    }
}
