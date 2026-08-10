//! `__torajs_any_prop_delete` — `delete recv.k` / `delete recv[k]`
//! on an `any` receiver (ES §13.5.1 / §10.1.10 OrdinaryDelete).
//!
//! Per-receiver dispatch mirrors the `member_get` gate:
//!
//! - null / undefined receiver → catchable TypeError (§13.5.1.2
//!   evaluates the property reference first; ToObject throws),
//!   answers 0 — the lowering's throw-check propagates before the
//!   value is consumed.
//! - `Tag::DynObj` → `__torajs_dynobj_delete` (drops the entry's
//!   key + heap value, tombstones the slot), answers 1 regardless —
//!   an absent key deletes to true per spec, and a dynobj has no
//!   non-configurable properties.
//! - `Tag::Arr` / `Tag::Closure` → expando delete through the props
//!   dynobj (NULL props slot = absent = true).
//! - `Tag::Obj` (struct cell) → the expando dict deletes like any
//!   other; a DECLARED layout member answers 0, since a fixed slot
//!   has nothing to remove (recorded divergence: bun deletes and
//!   answers true — structs-through-any are a reflection boundary),
//!   except that a non-configurable one refuses loudly. See
//!   [`struct_delete`].
//! - every other receiver (Str / Num / Bool / boxed primitives) →
//!   1: `delete` on a non-object base answers true (§13.5.1.2 —
//!   the property reference never materializes an own property).

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::member_get::{closure_props, header_flag_set, recv_cell};

/// torajs-arr inline props-dynobj slot at +24
/// (`torajs_arr::layout::ARR_PROPS_OFF` mirror — same constant
/// `prop_has` uses).
unsafe fn arr_props(arr: *mut c_void) -> *const c_void {
    unsafe { arr.cast::<u8>().add(24).cast::<*const c_void>().read() }
}
use crate::nanbox::{AnyValue, is_null, is_undefined};

unsafe extern "C" {
    /// torajs-dynobj — OrdinaryDelete (1 = an entry was removed).
    fn __torajs_dynobj_delete(obj: *mut c_void, key: *const c_void) -> i32;
    /// torajs-dynobj — own-entry presence + packed W/E/C flags
    /// (bit 2 = configurable) for the §10.1.10 step-4 refusal.
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
    fn __torajs_dynobj_get_flags(obj: *const c_void, key: *const c_void) -> u64;
    /// torajs-arr — expando delete through the props slot.
    fn __torajs_arrprops_delete(arr: *mut c_void, key: *const c_void) -> i32;
    /// torajs-arr — arguments-materialization length delete (hole
    /// tombstone); 0 = plain array, caller keeps the refusal.
    fn __torajs_arr_arguments_length_delete(arr: *mut c_void, key: *mut c_void) -> i64;
    /// torajs-arr — sloppy callee tombstone (S2); -1 = not applicable.
    fn __torajs_arr_arguments_callee_delete(arr: *mut c_void, key: *mut c_void) -> i64;
    /// torajs-arr — canonical-index delete (§10.4.2 [[Delete]], RFC
    /// 20260713 chunk C): 1 = deleted / absent, 0 = refused
    /// (non-configurable).
    fn __torajs_arr_delete_index(arr: *mut c_void, key: *mut c_void, idx: u64) -> i32;
    /// torajs-dynobj — the live attributes of a DECLARED class member
    /// (its redefine sidecar when one exists, else the layout default
    /// the integrity level moves), or -1 when the layout declares
    /// nothing under the key. See that fn's doc for why the question
    /// is asked rather than restated here.
    fn __torajs_obj_declared_field_attrs(obj: *mut c_void, key: *mut c_void) -> i64;
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// `torajs_dynobj::layout::BUCKET_FLAG_CONFIGURABLE` mirror — bit 2 of
/// a dict entry's packed W/E/C word, the only one a delete reads.
const BUCKET_FLAG_CONFIGURABLE: u64 = 1 << 2;

/// See module doc. `key` is a live Str cell (the lowering interns
/// static names and materializes dynamic string keys before the
/// call).
///
/// # Safety
/// Cell receivers are valid heap pointers; `key` is a live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_prop_delete(recv: AnyValue, key: *const c_void) -> i64 {
    unsafe { any_prop_delete_impl(recv, key, true) }
}

/// §28.1.3 Reflect.deleteProperty flavor — identical OrdinaryDelete
/// walk, but a non-configurable refusal answers 0 with NO pending
/// throw (the §13.5.1.2 strict TypeError belongs to the delete
/// expression's caller strictness, not to OrdinaryDelete itself).
/// The nullish-receiver TypeError still records — Reflect's strict
/// IsObject gate runs first, so that arm is unreachable through the
/// Reflect path anyway.
///
/// # Safety
/// Same contract as [`__torajs_any_prop_delete`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_prop_delete_soft(recv: AnyValue, key: *const c_void) -> i64 {
    unsafe { any_prop_delete_impl(recv, key, false) }
}

/// Shared refusal answer — see the two extern shells above.
unsafe fn refuse(throw_on_refusal: bool) -> i64 {
    if throw_on_refusal {
        unsafe {
            __torajs_throw_type_error(c"cannot delete a non-configurable property".as_ptr());
        }
    }
    0
}

unsafe fn any_prop_delete_impl(recv: AnyValue, key: *const c_void, throw_on_refusal: bool) -> i64 {
    if is_null(recv) || is_undefined(recv) {
        unsafe {
            __torajs_throw_type_error(c"cannot delete a property of null or undefined".as_ptr());
        }
        return 0;
    }
    // §6.1.7 — OrdinaryDelete of a symbol key is exactly an entry-table
    // removal: the index domain, `length`, the fn virtual pair and the
    // builtin-prototype tombstones below are all string-keyed and read
    // the key's Str payload. A shape with no dict has nothing to
    // remove, and §13.5.1.2 makes deleting an absent property `true`.
    if unsafe { crate::member_get_symbol::key_is_symbol(key) } {
        let Some((ptr, t)) = recv_cell(recv) else {
            return 1;
        };
        let dict = unsafe { crate::member_get_symbol::own_dict(ptr, t) };
        if dict.is_null() {
            return 1;
        }
        if unsafe { refuse_non_configurable(dict, key, throw_on_refusal) } {
            return 0;
        }
        unsafe { __torajs_dynobj_delete(dict as *mut c_void, key) };
        return 1;
    }
    match recv_cell(recv) {
        Some((ptr, t)) if t == Tag::DynObj as u16 => {
            if unsafe { refuse_non_configurable(ptr, key, throw_on_refusal) } {
                return 0;
            }
            unsafe { __torajs_dynobj_delete(ptr, key) };
            unsafe { tombstone_proto_method(ptr, key) };
            1
        }
        Some((ptr, t)) if t == Tag::Arr as u16 => {
            // Canonical index — element-domain delete (hole shadow
            // entry, chunk C). The expando dynobj never owns the
            // index domain.
            if let Some(idx) = unsafe { crate::prop_has::canonical_index(key) } {
                if unsafe { __torajs_arr_delete_index(ptr, key as *mut c_void, idx) } == 0 {
                    return unsafe { refuse(throw_on_refusal) };
                }
                return 1;
            }
            // `length` is permanently non-configurable (§10.4.2) —
            // EXCEPT on an arguments materialization, whose length
            // is a plain configurable data property (§10.4.4): the
            // kernel leaves a hole tombstone under the `"length"`
            // key and answers 1; a plain array answers 0 and keeps
            // the refusal.
            if unsafe { crate::prop_has::key_is(key, b"length") } {
                if unsafe { __torajs_arr_arguments_length_delete(ptr, key as *mut c_void) } != 0 {
                    return 1;
                }
                return unsafe { refuse(throw_on_refusal) };
            }
            // RFC 20260810-sloppy-goal-arguments S2 — a sloppy
            // arguments materialization's `callee` is configurable:
            // the kernel tombstones the live bag entry (so the keyed
            // readers can tell "deleted" from "strict mint") and
            // answers 1. -1 = not applicable — strict mint / plain
            // array keep the ordinary expando path below.
            if unsafe { crate::prop_has::key_is(key, b"callee") }
                && unsafe { __torajs_arr_arguments_callee_delete(ptr, key as *mut c_void) } == 1
            {
                return 1;
            }
            let props = unsafe { arr_props(ptr) };
            if !props.is_null() && unsafe { refuse_non_configurable(props, key, throw_on_refusal) }
            {
                return 0;
            }
            unsafe { __torajs_arrprops_delete(ptr, key) };
            // `Array.prototype` is an Arr cell (ES §23.1.3), so the
            // tombstone the dynobj protos take above has to be taken
            // here too — otherwise `delete Array.prototype.map` drops
            // a monkey-patch shadow that may not exist and leaves the
            // interned `map` answering every reader.
            unsafe { tombstone_proto_method(ptr, key) };
            1
        }
        Some((ptr, t)) if t == Tag::Closure as u16 => {
            // §22.1.2.4 family — a builtin ctor cell's `prototype`
            // is {[[Configurable]]: false}: the delete refuses with
            // the strict TypeError (RFC 20260721 刀 11 G11).
            if unsafe { crate::prop_has::key_is(key, b"prototype") }
                && crate::method_value::ctor::ctor_tag_of_cell(ptr).is_some()
            {
                return unsafe { refuse(throw_on_refusal) };
            }
            // §6.1.5.1 — the Symbol ctor's well-known data statics
            // are {configurable: false} (RFC 20260722 刀 2).
            if unsafe { crate::method_value::symbol_static::is_wellknown_on_symbol_ctor(ptr, key) }
            {
                return unsafe { refuse(throw_on_refusal) };
            }
            let props = unsafe { closure_props(ptr) };
            if !props.is_null() {
                if unsafe { refuse_non_configurable(props as *mut c_void, key, throw_on_refusal) } {
                    return 0;
                }
                unsafe { __torajs_dynobj_delete(props as *mut c_void, key) };
            }
            // chunk C — the virtual §20.2.4 name/length pair is
            // configurable: a delete tombstones the header bit so
            // every reader skips the virtual answer (idempotent on
            // a re-delete / post-recreate delete).
            if unsafe { crate::prop_has::key_is(key, b"name") } {
                unsafe { header_flag_set(ptr, torajs_rc::FLAG_FN_NAME_DELETED) };
            }
            if unsafe { crate::prop_has::key_is(key, b"length") } {
                unsafe { header_flag_set(ptr, torajs_rc::FLAG_FN_LENGTH_DELETED) };
            }
            // RFC 20260722-builtin-proto-reflection 刀 1 — a table
            // static (`delete Promise.all`) is {configurable: true}:
            // tombstone its id so every table-backed reader answers
            // absent (an expando restore shadows the bit).
            if let Some(id) = unsafe { crate::method_value::ctor_static_table_id(ptr, key) } {
                torajs_rc::ns_static_mark_deleted(id);
            }
            1
        }
        // Rotation 354 — promise bag delete (the +32 expando the
        // defineProperty / plain-assign arms write): configurability
        // gate, then the entry drop. No virtual pair and no ctor
        // statics on an instance cell; a NULL bag answers 1
        // idempotently (spec success on a nonexistent key).
        Some((ptr, t)) if t == Tag::Promise as u16 => {
            let props = unsafe { crate::member_get::promise_props(ptr) };
            if !props.is_null() {
                if unsafe { refuse_non_configurable(props as *mut c_void, key, throw_on_refusal) } {
                    return 0;
                }
                unsafe { __torajs_dynobj_delete(props as *mut c_void, key) };
            }
            1
        }
        Some((ptr, t)) if t == Tag::Obj as u16 => unsafe {
            struct_delete(ptr, key, throw_on_refusal)
        },
        // RFC 20260716 刀 5 (rotation 121 chunk 5) — wrapper expando
        // delete (mirror of the closure arm). A NULL props slot
        // (never any assign) answers 1 idempotently: `delete <expr>`
        // on a nonexistent key is a spec success.
        Some((ptr, t))
            if t == Tag::NumberWrapper as u16
                || t == Tag::StringWrapper as u16
                || t == Tag::BooleanWrapper as u16 =>
        {
            // §10.4.3 — a StringWrapper's inherent own face (every
            // canonical index in range, plus `length`) is
            // {configurable: false}, so the module-strict delete
            // throws. The expando below never owns those keys.
            if t == Tag::StringWrapper as u16
                && let Some(inner) =
                    unsafe { crate::wrapper_view_through::resolve_inner_recv(ptr, t) }
                && unsafe { crate::member_get_str::str_own_pair(inner, key) }.is_some()
            {
                return unsafe { refuse(throw_on_refusal) };
            }
            let props = unsafe { crate::member_get::wrapper_props(ptr) };
            if !props.is_null() {
                if unsafe { refuse_non_configurable(props as *mut c_void, key, throw_on_refusal) } {
                    return 0;
                }
                unsafe { __torajs_dynobj_delete(props as *mut c_void, key) };
            }
            1
        }
        // RFC 20260722 刀 4 — a RegExp instance's `lastIndex` is
        // {configurable: false} (§22.2.4.1); the module-strict
        // delete throws. Every other key owns nothing → success.
        Some((_, t)) if t == Tag::RegExp as u16 => {
            if unsafe { crate::prop_has::key_is(key, b"lastIndex") } {
                return unsafe { refuse(throw_on_refusal) };
            }
            1
        }
        _ => 1,
    }
}

/// §10.1.10 over a class-instance cell — the `Tag::Obj` arm.
///
/// A struct cell carries own properties in two places, and only one of
/// them can lose an entry. The `+24` expando dict is an ordinary
/// dynobj, so a key it owns deletes exactly as it does on every other
/// receiver — including the step-4 refusal a `defineProperty`'d entry
/// can arm. A DECLARED member is a fixed layout slot with nowhere to
/// go, so it keeps the recorded divergence (bun deletes and answers
/// true; we answer false) — but "not removable" and "not
/// configurable" are different sentences, and only the second one
/// throws. Asking for the attributes tells us which we are in:
/// a frozen or sealed instance, or one whose field was redefined
/// `{configurable: false}`, refuses with the strict TypeError that
/// test262's `verifyNotConfigurable` probes for.
///
/// The one declared member that CAN detach is the error `message`
/// line (§20.5.6.1.1): its slot holds an own-absence sentinel, so the
/// delete swaps rather than removes. It reaches that swap through the
/// same configurable gate as everything else.
///
/// # Safety
/// `ptr` is a live `Tag::Obj` heap pointer; `key` is a live Str cell
/// (symbol keys are routed by the caller before this arm).
unsafe fn struct_delete(ptr: *mut c_void, key: *const c_void, throw_on_refusal: bool) -> i64 {
    let attrs = unsafe { __torajs_obj_declared_field_attrs(ptr, key as *mut c_void) };
    if attrs >= 0 {
        if attrs as u64 & BUCKET_FLAG_CONFIGURABLE == 0 {
            return unsafe { refuse(throw_on_refusal) };
        }
        return unsafe { crate::struct_error_msg::error_message_delete(ptr, key) };
    }
    let props = unsafe { crate::member_get_layout::struct_props(ptr) };
    if props.is_null() {
        // §13.5.1.2 — deleting a property that is not there succeeds.
        return 1;
    }
    if unsafe { refuse_non_configurable(props, key, throw_on_refusal) } {
        return 0;
    }
    unsafe { __torajs_dynobj_delete(props as *mut c_void, key) };
    1
}

/// RFC 20260712 chunk 3 — a builtin `<Ctor>.prototype` singleton
/// hides its interned family method behind the deleted-mid tombstone;
/// the entry delete the callers run first only removes a monkey-patch
/// shadow, if any. No-op on every other receiver. Idempotent.
///
/// Shared by the dynobj arm and the Arr one, since `Array.prototype`
/// is an Arr cell rather than a dynobj (ES §23.1.3).
///
/// # Safety
/// `ptr` is a live heap cell (compared, not dereferenced); `key` is a
/// live Str cell.
unsafe fn tombstone_proto_method(ptr: *mut c_void, key: *const c_void) {
    let proto_tag = unsafe { torajs_rc::builtin_proto::__torajs_builtin_proto_tag_of(ptr) };
    if proto_tag < 0 {
        return;
    }
    let mid = unsafe { crate::method_value::key_method_id(key) };
    if mid != torajs_rc::ANY_METHOD_UNKNOWN
        && crate::method_support::proto_tag_family_owns(proto_tag, mid)
    {
        unsafe { torajs_rc::builtin_proto::__torajs_builtin_proto_mark_deleted(proto_tag, mid) };
    } else if unsafe { crate::prop_has::key_is(key, b"constructor") } {
        // The virtual `constructor` own face tombstones through its
        // non-interning slot (RFC 20260721 刀 11 G11).
        unsafe {
            torajs_rc::builtin_proto::__torajs_builtin_proto_mark_deleted(
                proto_tag,
                torajs_rc::ANY_METHOD_CONSTRUCTOR_SLOT,
            )
        };
    } else if let Some(slot) =
        unsafe { crate::method_support_proto_meta::fn_proto_meta_slot(ptr, key) }
    {
        // Function.prototype's virtual own name/length pair is
        // {configurable: true} (§20.2.3, RFC 20260722 刀 3).
        unsafe { torajs_rc::builtin_proto::__torajs_builtin_proto_mark_deleted(proto_tag, slot) };
    } else if let Some(amid) =
        unsafe { crate::method_support::proto_tag_accessor_mid(proto_tag, key) }
    {
        // The non-interning `size` accessor id (C2-size) tombstones
        // through the same bitmask.
        unsafe { torajs_rc::builtin_proto::__torajs_builtin_proto_mark_deleted(proto_tag, amid) };
    }
}

/// §10.1.10 OrdinaryDelete step 4 (chunk D-2a, RFC 20260711): a
/// present own property whose `configurable` flag is clear refuses
/// the delete; tr programs are module-strict so §13.5.1.2 then
/// throws the catchable TypeError (test262 propertyHelper's
/// isConfigurable probe relies on exactly this shape). A plain
/// `o.k = v` entry carries BUCKET_FLAGS_DEFAULT (all set), so only
/// defineProperty-shaped entries can refuse.
unsafe fn refuse_non_configurable(
    obj: *const c_void,
    key: *const c_void,
    throw_on_refusal: bool,
) -> bool {
    if unsafe { __torajs_dynobj_has(obj, key) } != 0
        && unsafe { __torajs_dynobj_get_flags(obj, key) } & BUCKET_FLAG_CONFIGURABLE == 0
    {
        let _ = unsafe { refuse(throw_on_refusal) };
        return true;
    }
    false
}
