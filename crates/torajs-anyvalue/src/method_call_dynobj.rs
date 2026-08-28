//! `Tag::DynObj` + `Tag::Obj` receiver arms for
//! `__torajs_any_method_call` (split out of `method_call.rs` — the
//! dispatcher stays the id-switch, the property-probing receivers
//! live here).
//!
//! - `dynobj_method` — probe the property by the interned name Str;
//!   a closure-cell value with a boxed dual entry invokes through
//!   the uniform ABI; an accessor entry's getter runs first and its
//!   answer dispatches as the callee (C4+ chunk 523).
//! - `struct_method` (L3b #9, chunk 524) — static-layout class /
//!   anonymous-struct instances (`Tag::Obj`) store fields at fixed
//!   byte offsets, so the probe walks the toolchain-emitted
//!   `__torajs_class_layouts` table (torajs-structmeta W-J Phase A4
//!   read side): class_tag@+8 → layout lookup → field find by the
//!   name Str's bytes → the slot's static type decides the callee
//!   shape. A `Closure`-typed slot (field type_tag 8) holds the raw
//!   env cell; an `Any`-typed slot (tag 0) holds a NaN-box that may
//!   wrap one. Everything else — no layout (class_tag 0), absent
//!   field, non-callable slot type — answers the same catchable
//!   TypeError as the other arms, never a layout mis-read. This is
//!   what makes `n.inner.op(1, 1)` work when `inner: { op: (a, b) =>
//!   a + b }` nests inside an `any` ObjectLit init (the nested
//!   literal lowers as an anon struct, not a dynobj).
//!
//! Argument ledger: identical to the dispatcher — the property
//! probe borrows (a struct field slot is owned by the receiver,
//! which outlives the call); the adapter's return is caller-owned
//! per the boxed-value convention.

use core::ffi::c_void;

use crate::method_call::{closure_boxed_entry, closure_cell_entry, not_callable};
use crate::method_call_dynobj_chain::proto_chain_method;
use crate::method_call_dynobj_proto::object_proto_fallback;
use crate::nanbox::AnyValue;
use crate::nanbox_encode::__torajs_anyv_box_pointer;

unsafe extern "C" {
    /// torajs-str — allocate a fresh Str from raw bytes (the
    /// "[object Object]" text + the re-dispatch "toString" key).
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    /// torajs-str — release a heap Str reference.
    fn __torajs_str_drop(s: *mut c_void);
    /// torajs-dynobj — property probe by Str key: the slot's ANY_TAG
    /// (5 = absent) / per-tag payload.
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
    /// torajs-dynobj — own-key membership probe (1 = present).
    /// Disambiguates `get_tag`'s ANY_UNDEF answer: absent key vs an
    /// own entry storing undefined.
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
    /// torajs-throw — pending-throw probe (non-zero = pending); the
    /// getter-as-callee arm aborts before the callee probe so
    /// `not_callable` can't clobber the user throw.
    fn __torajs_throw_check() -> i64;
    /// torajs-dynobj — run an accessor entry's getter; the answer is
    /// an owned AnyValue per the boxed-value convention.
    fn __torajs_accessor_invoke_getter(pair: *const c_void, recv_anyv: u64) -> u64;
    /// torajs-arr — arr-props expando probe (NULL props answers 5 =
    /// absent internally); same ANY_TAG channel as the dynobj pair.
    fn __torajs_arrprops_get_tag(arr: *mut c_void, key: *const c_void) -> u64;
    fn __torajs_arrprops_get_value(arr: *mut c_void, key: *const c_void) -> u64;
    /// torajs-arr — own-key membership over the side props (1 =
    /// present); the get_tag ANY_UNDEF disambiguator.
    fn __torajs_arrprops_has(arr: *mut c_void, key: *const c_void) -> i32;
    /// torajs-structmeta — read side over `__torajs_class_layouts`
    /// (NULL for class_tag 0 / past the table).
    fn __torajs_struct_layout_lookup(class_tag: u32) -> *const c_void;
    /// torajs-structmeta — field index by name bytes (u32::MAX miss).
    fn __torajs_struct_field_find(layout: *const c_void, name: *const u8, name_len: u32) -> u32;
    /// torajs-structmeta — field byte offset + coarse type tag
    /// (zeroed for a miss; a real offset is never 0).
    fn __torajs_struct_field_info(layout: *const c_void, idx: u32) -> FieldInfo;
    /// torajs-structmeta 刀 4 — class-method boxed adapter by name
    /// bytes (NULL miss).
    fn __torajs_struct_method_find(
        layout: *const c_void,
        name: *const u8,
        name_len: u32,
    ) -> *const c_void;
    /// torajs-structmeta 404-01 — the flags-aware variant, the one
    /// finder that also answers twin-primary records (recv-first
    /// adapters). Writes the record's flags word on a hit.
    fn __torajs_struct_method_find_flags(
        layout: *const c_void,
        name: *const u8,
        name_len: u32,
        out_flags: *mut u32,
    ) -> *const c_void;
    /// torajs-structmeta — does a name spell an accessor SLOT
    /// (`__getter_v`)? 255 = a plain, user-callable name.
    fn __torajs_accessor_name_kind(name: *const u8, name_len: u32) -> u8;
}

/// Mirror of `torajs-structmeta::FieldInfo` (returned by value
/// across the FFI).
#[repr(C)]
struct FieldInfo {
    field_byte_offset: u32,
    type_tag: u8,
}

/// Accessor-entry sentinel in the dynobj probe's tag channel —
/// mirror of torajs-core `ssa_lower_accessor.rs::ANY_ACCESSOR_TAG`.
const ANY_ACCESSOR_TAG: u64 = 6;

/// Byte offset of the `class_tag` u32 inside a `Tag::Obj` instance —
/// mirror of torajs-core `ssa_lower::OBJ_CLASS_TAG_OFF`.
const OBJ_CLASS_TAG_OFF: usize = 8;

/// Interned name Str layout — mirror of torajs-str
/// `layout::{STR_LEN_OFF, STR_DATA_OFF}`.
const STR_LEN_OFF: usize = 8;
const STR_DATA_OFF: usize = 16;

/// Field type_tags whose slot can hold a callee — mirror of
/// torajs-core `ssa::module_methods::field_type_tag_of` (0 = Any,
/// 8 = Closure).
const FIELD_TAG_ANY: u8 = 0;
const FIELD_TAG_CLOSURE: u8 = 8;

/// `Tag::DynObj` arm — probe the property by name; a closure-cell
/// value invokes through its boxed dual entry. The property probe
/// borrows (dynobj keeps its own value reference; the receiver
/// outlives the call), and the adapter's return is caller-owned per
/// the boxed-value convention. `recv_slot` threads to the generic
/// array-like mutators (dynobj relocation writeback; NULL for
/// non-variable receivers).
pub(crate) unsafe fn dynobj_method(
    obj: *mut c_void,
    mid: i64,
    name_str: *const u8,
    recv_slot: *mut u64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        // RFC 20260712 chunks 2+3a — a NULL name is the reified-cell
        // `call` / `apply` re-dispatch (ordinary calls always carry
        // name bytes): the mid is authoritative, so an array-family
        // mid runs the ES generic array-like semantics over this
        // receiver instead of falling to the not-callable exit.
        if name_str.is_null() && crate::method_call_arraylike_concat::obj_supported(mid) {
            return crate::method_call_arraylike_concat::obj_method(
                obj, mid, recv_slot, argv, argc,
            );
        }
        if !name_str.is_null() {
            let key = name_str as *const c_void;
            let dtag = __torajs_dynobj_get_tag(obj, key);
            // Absent own entry → the inherited Object.prototype
            // surface. Ordered AFTER the own probe so a user
            // monkey-patch (`o.valueOf = fn`) always wins; a
            // resolved-but-not-callable entry keeps the TypeError.
            if dtag == 5 {
                // `get_tag` conflates absent with an own entry
                // STORING undefined; the latter shadows the
                // prototype, so `{toString: undefined}.toString()`
                // is the resolved-not-callable TypeError, not the
                // inherited "[object Object]".
                if __torajs_dynobj_has(obj, key) != 0 {
                    return not_callable();
                }
                // knife 2 — user chain first; this = receiver.
                if let Some(r) = proto_chain_method(obj, name_str, recv_slot, argv, argc) {
                    return r;
                }
                return object_proto_fallback(obj, mid, name_str, false, argv, argc);
            }
            // ANY_HEAP = 4 — a plain closure-cell property.
            if dtag == 4 {
                let cell = __torajs_dynobj_get_value(obj, key);
                // A reified builtin cell stored as an own property
                // (`obj.pop = Array.prototype.pop`) re-dispatches
                // its CARRIED mid with this receiver — the bare
                // entry would be the this=undefined TypeError.
                // Array-family mids run the ES generic array-like
                // semantics over this receiver; every other carried
                // mid (the §20.1.3.6 badge cell, per-type methods)
                // re-enters the dispatcher with the boxed receiver
                // (RFC 20260713 blade 2 — a miss there stays loud).
                if crate::nanbox::is_cell(cell)
                    && let Some(mid2) =
                        crate::method_value::builtin_method_mid(crate::nanbox::as_void_ptr(cell))
                {
                    // Rotation 204 — the mint family picks the
                    // family-generic lane FIRST (the `.call` /
                    // proto-patch stations' gate): a String-
                    // prototype-minted cell runs the §22.1.3
                    // ToString(this) generic instead of being
                    // captured by the shared-mid array-like arm
                    // (`inst.slice = String.prototype.slice;
                    // inst.slice(0,2)` answered the array-like `[]`
                    // — silent wrong). A family-less or
                    // non-generic-family cell answers None and keeps
                    // the arms below unchanged.
                    let fam = crate::method_value::builtin_method_family(
                        crate::nanbox::as_void_ptr(cell),
                    );
                    if let Some(out) = crate::method_call_closure::generic_builtin_this(
                        mid2,
                        __torajs_anyv_box_pointer(obj),
                        argv,
                        argc,
                        fam,
                    ) {
                        return out;
                    }
                    if crate::method_call_arraylike::arraylike_supported(mid2) {
                        return crate::method_call_arraylike::arraylike_method(
                            obj, mid2, recv_slot, argv, argc,
                        );
                    }
                    return crate::method_call::any_method_call_inner(
                        __torajs_anyv_box_pointer(obj),
                        mid2,
                        core::ptr::null(),
                        recv_slot,
                        argv,
                        argc,
                    );
                }
                // A reified class-method cell (knife B cut 1) — e.g.
                // `C.prototype.m()` — invokes its carried boxed
                // adapter with this receiver in the env slot.
                if crate::nanbox::is_cell(cell)
                    && let Some(adapter) = crate::method_value_class::class_method_adapter(
                        crate::nanbox::as_void_ptr(cell),
                    )
                {
                    // 398-02 — a STATIC face (`(tag 0, twin ≠ 0)`,
                    // knife 3b's encoding) has no receiver channel in
                    // its mono body (a static `this` resolved to the
                    // DECLARING class at compile time), so a member
                    // call on a dynobj receiver — the class object is
                    // one — must ride the `__smany_` twin with the
                    // receiver per §13.3.6.2, or `X.make(6)` on
                    // `const X: any = Sub` silently constructs Base.
                    // The same verdict `invoke_with_this` applies on
                    // the `.call` / `.apply` spellings; instance
                    // faces keep the mono env path below.
                    let cellp = crate::nanbox::as_void_ptr(cell);
                    let (face_tag, twin) =
                        crate::method_value_class::class_method_face_guard(cellp);
                    if face_tag == 0 && twin != 0 {
                        let recv = __torajs_anyv_box_pointer(obj);
                        return crate::method_call_closure_dispatch::invoke_boxed_recv_first(
                            cellp, twin, recv, argv, argc,
                        );
                    }
                    // Blade 3 guard, own-entry edition (405-01 face
                    // 2 probe p17) — this arm's receiver is always a
                    // dynobj (`C.prototype.m()` runs with this =
                    // the prototype object itself), never the owning
                    // class's struct layout, so a guarded instance
                    // face rides the `__cmany_` twin; the mono path
                    // read nanbox bits at baked offsets. twin 0
                    // keeps the mono path (this-free bodies never
                    // read the receiver; super-route bodies are the
                    // recorded residue).
                    if face_tag != 0 && twin != 0 {
                        let recv = __torajs_anyv_box_pointer(obj);
                        return crate::method_call_closure_dispatch::invoke_boxed_recv_first(
                            cellp, twin, recv, argv, argc,
                        );
                    }
                    return crate::method_call_closure_dispatch::invoke_boxed(
                        obj, adapter, argv, argc,
                    );
                }
                // The cell's NaN-box encoding is its pointer bits.
                if let Some((env, entry)) = closure_boxed_entry(cell) {
                    // RFC 20260717-objlit-anylane-recv knife 1 — a
                    // receiver-first closure (an any-lane object-
                    // literal method whose body says `this`) carries
                    // FLAG_CLOSURE_RECV_FIRST on its env header; its
                    // first declared param is `__this: any`, so the
                    // receiver rides argv[0] and the user args shift
                    // up (pre-fix the body read garbage — SIGSEGV).
                    let flags = (env as *const u8).add(6).cast::<u16>().read();
                    if flags & torajs_rc::FLAG_CLOSURE_RECV_FIRST != 0 {
                        let recv = __torajs_anyv_box_pointer(obj);
                        return crate::method_call::invoke_boxed_recv_first(
                            env, entry, recv, argv, argc,
                        );
                    }
                    return crate::method_call::invoke_boxed(env, entry, argv, argc);
                }
            }
            // C4+ chunk 523 — getter-as-callee: an accessor entry's
            // getter runs first, its (owned) answer dispatches as
            // the callee, and the reference releases after the call
            // (the invoke keeps the cell alive across it).
            if dtag == ANY_ACCESSOR_TAG {
                let pair = __torajs_dynobj_get_value(obj, key) as *const c_void;
                let got = __torajs_accessor_invoke_getter(
                    pair,
                    crate::nanbox_encode::__torajs_anyv_box_from_pair(4, obj as i64),
                );
                // A throwing getter aborts the call (§13.3.6.1 Get
                // ReturnIfAbrupt) — propagate before the callee
                // probe, or `not_callable` below would clobber the
                // user's pending throw with its own TypeError.
                if __torajs_throw_check() != 0 {
                    return got;
                }
                if let Some((env, entry)) = closure_boxed_entry(got) {
                    // A recv-first callee binds the holder as `this`
                    // (§13.3.6 EvaluateCall — the Reference base;
                    // RFC 20260717-objlit-anylane-recv knife 2f).
                    let recv = crate::nanbox_encode::__torajs_anyv_box_from_pair(4, obj as i64);
                    let r = crate::method_call::invoke_with_this(env, entry, recv, argv, argc);
                    crate::nanbox_ffi::__torajs_anyv_rc_dec(got);
                    return r;
                }
                crate::nanbox_ffi::__torajs_anyv_rc_dec(got);
            }
        }
        not_callable()
    }
}

/// `Tag::Arr` own-expando probe (RFC 20260713-array-proto-residual
/// blade 2) — §10.1.8.1 OrdinaryGet: an own arr-props entry shadows
/// the built-in method surface (`arr.getClass = Object.prototype.
/// toString; arr.getClass()` / a user closure stored on the array).
/// `None` = no own entry, the caller proceeds to the builtin mid
/// dispatch; `Some` = the entry resolved (invoked, or the
/// resolved-but-not-callable TypeError fired — a shadowing
/// non-callable must NOT fall through to the builtin, per spec).
pub(crate) unsafe fn arr_expando_method(
    arr: *mut c_void,
    recv: AnyValue,
    name_str: *const u8,
    recv_slot: *mut u64,
    argv: *const u64,
    argc: i64,
) -> Option<AnyValue> {
    unsafe {
        let key = name_str as *const c_void;
        let dtag = __torajs_arrprops_get_tag(arr, key);
        if dtag == 5 {
            // Same absent/stored-undefined conflation as the dynobj
            // arm: an own entry storing undefined SHADOWS the builtin
            // (`a.join = undefined; a.join()` is the resolved-not-
            // callable TypeError, not the builtin join).
            if __torajs_arrprops_has(arr, key) != 0 {
                return Some(not_callable());
            }
            return None;
        }
        // ANY_HEAP = 4 — a closure-cell property.
        if dtag == 4 {
            let cell = __torajs_arrprops_get_value(arr, key);
            // A reified builtin cell re-dispatches its CARRIED mid
            // with this receiver (NULL name — the mid is
            // authoritative; the badge / array-family arms apply).
            if crate::nanbox::is_cell(cell)
                && let Some(mid2) =
                    crate::method_value::builtin_method_mid(crate::nanbox::as_void_ptr(cell))
            {
                // Rotation 204 — family-generic gate first (mirror
                // of the dynobj arm above): a String-prototype-
                // minted cell stored as an array expando runs the
                // §22.1.3 ToString(this) generic; None falls through
                // to the ordinary mid re-dispatch.
                let fam =
                    crate::method_value::builtin_method_family(crate::nanbox::as_void_ptr(cell));
                if let Some(out) =
                    crate::method_call_closure::generic_builtin_this(mid2, recv, argv, argc, fam)
                {
                    return Some(out);
                }
                return Some(crate::method_call::any_method_call_inner(
                    recv,
                    mid2,
                    core::ptr::null(),
                    recv_slot,
                    argv,
                    argc,
                ));
            }
            if let Some((env, entry)) = closure_boxed_entry(cell) {
                // Recv-first expando method binds the array as
                // `this` (knife 2f).
                let recv = crate::nanbox_encode::__torajs_anyv_box_from_pair(4, arr as i64);
                return Some(crate::method_call::invoke_with_this(
                    env, entry, recv, argv, argc,
                ));
            }
        }
        // Accessor entry — getter runs first, its (owned) answer
        // dispatches as the callee (mirror of the dynobj arm).
        if dtag == ANY_ACCESSOR_TAG {
            let pair = __torajs_arrprops_get_value(arr, key) as *const c_void;
            let got = __torajs_accessor_invoke_getter(
                pair,
                crate::nanbox_encode::__torajs_anyv_box_from_pair(4, arr as i64),
            );
            if let Some((env, entry)) = closure_boxed_entry(got) {
                // Recv-first callee binds the array as `this`
                // (knife 2f, mirror of the dynobj getter arm).
                let recv = crate::nanbox_encode::__torajs_anyv_box_from_pair(4, arr as i64);
                let r = crate::method_call::invoke_with_this(env, entry, recv, argv, argc);
                crate::nanbox_ffi::__torajs_anyv_rc_dec(got);
                return Some(r);
            }
            crate::nanbox_ffi::__torajs_anyv_rc_dec(got);
        }
        Some(not_callable())
    }
}

mod tail;
pub(crate) use tail::*;
