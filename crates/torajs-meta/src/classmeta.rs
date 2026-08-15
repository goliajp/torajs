//! Class / prototype registries keyed by class runtime tag —
//! port of `runtime_str.c` L820-867. Step 7 NaN-box AnyValue
//! cutover: registry slots store `AnyValue` immediates; the
//! legacy `*AnyBox`-shape fns were deleted in 7f-D-1.
//!
//! Two parallel fixed-size arrays sized at `MAX_CLASSES = 256`:
//!
//! - `protos_by_tag_imm[c]` — borrowed `AnyValue` of the
//!   `__proto_<C>` LetDecl. Read by `Object.getPrototypeOf(instance)`.
//! - `classes_by_tag_imm[c]` — borrowed `AnyValue` of the
//!   `__class_<C>` LetDecl. Read by `__torajs_anyv_class_get`
//!   (P4.5 `new.target` plumbing).
//!
//! Lifetime-of-process — the `__proto_<C>` / `__class_<C>` Any-values
//! live in module-scope let bindings whose lifetime spans the whole
//! program; no rc bump on register (the let binding keeps them
//! alive). `_anyv_proto_get` / `_anyv_class_get` rc_inc the heap
//! payload (when the slot encodes a cell) on every read so the
//! caller receives an OWNED reference.
//!
//! Concurrency: JS execution is single-threaded so unsynchronized
//! `static mut` matches the pre-port C bit-for-bit. Cross-thread
//! access from native plugins would be UB — never the model.

use core::ffi::c_void;

use crate::reflect::{ANY_HEAP, TAG_DYNOBJ, alloc_str_key, heap_type_tag};

mod define;
mod error_family;
mod generic_alias;
mod reify;

unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
    /// Locks `name` / `length` / `prototype` slots on the class-object
    /// dynobj to the ES §17 built-in Function attribute pattern (no-op
    /// if `obj` is NULL, non-cell, or non-dynobj). Cross-crate FFI
    /// declared here to keep the classmeta dep tree lean.
    fn __torajs_dynobj_lock_builtin_fn_class_slots(obj: *mut c_void);
    fn __torajs_dynobj_mark_class_ctor(obj: *mut c_void);
    fn __torajs_get_builtin_prototype(tag: i64) -> *mut c_void;
    fn __torajs_dynobj_define(
        obj_slot: *mut *mut c_void,
        key: *const u8,
        tag: u64,
        value: u64,
        flags_byte: u64,
    );
    fn __torajs_str_drop(s: *mut u8);
    fn __torajs_struct_layout_lookup(class_tag: u32) -> *const c_void;
    fn __torajs_struct_method_count(layout: *const c_void) -> u32;
    fn __torajs_struct_method_at(
        layout: *const c_void,
        idx: u32,
        out_name: *mut *const u8,
        out_len: *mut u32,
    ) -> *const c_void;
    /// torajs-structmeta — the record's flags word (S2.38, bit 0 =
    /// this-free body).
    fn __torajs_struct_method_flags_at(layout: *const c_void, idx: u32) -> u32;
    /// torajs-structmeta — the record's `__cmany_` twin adapter
    /// vaddr (blade 3; NULL = no twin minted).
    fn __torajs_struct_method_twin_at(layout: *const c_void, idx: u32) -> *const c_void;
    fn __torajs_class_method_cell_new(
        adapter: u64,
        this_free: u64,
        class_tag: u64,
        twin: u64,
    ) -> *mut u8;
    fn __torajs_builtin_method_cell(mid: i64) -> *mut u8;
    /// torajs-anyvalue — reified class-accessor face (RFC
    /// 20260718-accessor-reify 刀 2; name transfers).
    fn __torajs_class_accessor_cell_new(adapter: u64, name: *mut u8, length: u64) -> *mut u8;
    /// torajs-dynobj — fresh `+1`-rc AccessorPair (faces transfer).
    fn __torajs_accessor_pair_new(get: *mut c_void, set: *mut c_void, kinds: u64) -> *mut c_void;
}

const MAX_CLASSES: usize = 256;

/// Builtin-proto singleton tag for %Function.prototype% (mirrors
/// `torajs-rc::builtin_proto`'s tag table; same value genfn.rs uses).
const FUNCTION_PROTO_TAG: i64 = 13;

/// `flags_byte` for `__torajs_dynobj_define` encoding the §10.2.3
/// MakeConstructor "constructor" descriptor `{[[Value]], writable:
/// true, enumerable: false, configurable: true}` — DEFINE_PRESENT_
/// {VALUE, WRITABLE, ENUMERABLE, CONFIGURABLE} + DEFINE_FLAG_
/// {WRITABLE, CONFIGURABLE} (torajs-dynobj layout mirror).
const DEFINE_CTOR_FLAGS: u64 = (1 << 6) | (1 << 3) | (1 << 4) | (1 << 5) | (1 << 0) | (1 << 2);

/// `flags_byte` for a static-FIELD own entry — ClassFieldDefinition
/// data properties are `{writable: true, enumerable: true,
/// configurable: true}` (CreateDataPropertyOrThrow §7.3.6), so the
/// ctor set plus the enumerable flag bit.
const DEFINE_FIELD_FLAGS: u64 = DEFINE_CTOR_FLAGS | (1 << 1);

/// `flags_byte` for an accessor own entry `{enumerable: false,
/// configurable: true}` with both faces present — DEFINE_PRESENT_
/// {VALUE, GET, SET, ENUMERABLE, CONFIGURABLE} + DEFINE_FLAG_
/// CONFIGURABLE (the pair rides the value channel; RFC
/// 20260718-accessor-reify 刀 2, mirror of the 刀 1 install flags).
const DEFINE_ACCESSOR_FLAGS: u64 = (1 << 6) | (1 << 7) | (1 << 8) | (1 << 4) | (1 << 5) | (1 << 2);

/// Kinds mirror of `torajs_dynobj::accessor::ACC_KIND_BOXED` on both
/// faces (the invoke path probes the class-face sentinel before the
/// kinds dispatch, so the value is nominal).
const ACC_KINDS_BOXED_BOTH: u64 = 5 | (5 << 8);

// NaN-box constants mirrored from torajs-anyvalue::nanbox —
// re-declared here to keep deps tree narrow.
const VALUE_NULL_IMM: u64 = 0x02;
const VALUE_UNDEFINED_IMM: u64 = 0x0A;
const TAG_BIT_TYPE_OTHER: u64 = 0x02;
/// Step 8b-B — top-16-bit mask for strict cell detection (mirrors
/// `torajs-anyvalue::nanbox::TOP_16_MASK`). ShortStr (top16 =
/// 0x0001) must NOT pass is_cell_imm.
const TOP_16_MASK: u64 = 0xFFFF_0000_0000_0000;

#[inline]
const fn is_cell_imm(v: u64) -> bool {
    (v & TOP_16_MASK) == 0 && (v & TAG_BIT_TYPE_OTHER) == 0 && v != 0
}

#[inline]
fn in_range(tag: i64) -> bool {
    (0..MAX_CLASSES as i64).contains(&tag)
}

static mut PROTOS_BY_TAG_IMM: [u64; MAX_CLASSES] = [0u64; MAX_CLASSES];
static mut CLASSES_BY_TAG_IMM: [u64; MAX_CLASSES] = [0u64; MAX_CLASSES];

/// rotation 186 — borrow read of the class object's CURRENT cell
/// bits (a define may have resized/moved it). ssa_lower emits this
/// after every reify/register call to refresh the `__class_<C>`
/// module binding; 0 = unregistered tag (caller leaves the slot).
/// Pure read: no rc traffic, the table keeps its stake.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_class_cell_raw(tag: i64) -> u64 {
    if !in_range(tag) {
        return 0;
    }
    // SAFETY: single-threaded JS runtime, no aliased writes.
    unsafe { CLASSES_BY_TAG_IMM[tag as usize] }
}

/// rotation 186 — proto twin of [`__torajs_class_cell_raw`] for the
/// `__proto_<C>` module binding.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_proto_cell_raw(tag: i64) -> u64 {
    if !in_range(tag) {
        return 0;
    }
    // SAFETY: single-threaded JS runtime, no aliased writes.
    unsafe { PROTOS_BY_TAG_IMM[tag as usize] }
}

/// Register the class's `__proto_<C>` AnyValue immediate at
/// module init.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_anyv_proto_register(tag: i64, proto_anyv: u64) {
    if !in_range(tag) {
        return;
    }
    // SAFETY: single-threaded JS runtime, no aliased writes.
    unsafe {
        PROTOS_BY_TAG_IMM[tag as usize] = proto_anyv;
    }
}

/// Register the class's `__class_<C>` AnyValue immediate.
///
/// After stashing the slot, lock the class-object dynobj's `name` /
/// `length` / `prototype` entries to the ES §17 built-in Function
/// attribute shape (see [`crate::seal::__torajs_dynobj_lock_builtin_fn_class_slots`]
/// upstream). Uniform for user + built-in classes because ES §10.2.3
/// MakeConstructor mandates the same attribute set on user classes.
/// Non-cell / non-dynobj slots (e.g. `class_anyv` set to a sentinel by
/// tests) are silently ignored by the helper.
///
/// `is_synth_gen != 0` flags a desugar-synthesized generator class
/// (`__Gen_<f>`): its `__proto_<C>` IS the generator fn's `.prototype`
/// object, which per §27.3.3.2 carries no own `constructor`, and the
/// class object itself is unreachable from user code — the
/// first-class MakeConstructor wiring is skipped.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_anyv_class_register(
    tag: i64,
    class_anyv: u64,
    is_synth_gen: i64,
    parent_tag: i64,
) {
    if !in_range(tag) {
        return;
    }
    // SAFETY: same as anyv_proto_register.
    unsafe {
        CLASSES_BY_TAG_IMM[tag as usize] = class_anyv;
    }
    if is_cell_imm(class_anyv) {
        // SAFETY: cell-encoded AnyValue → 48-bit user-VA pointer to a
        // valid heap object; the helper self-guards against non-dynobj
        // shape via its own header tag check.
        unsafe { __torajs_dynobj_lock_builtin_fn_class_slots(class_anyv as *mut c_void) };
        if is_synth_gen == 0 {
            // SAFETY: same cell; each wiring step re-guards shape itself.
            unsafe { wire_first_class_links(tag, class_anyv, parent_tag) };
        }
    }
}

/// RFC 20260717-class-first-class-value knife A — the first-class
/// function-object links, wired once per class at register time:
///
/// 1. mark the class dynobj `FLAG_DYNOBJ_CLASS_CTOR` so `typeof C`
///    answers `"function"` (ES: a class constructor IS a function
///    object; tr models it as a dynobj whose tag alone reads
///    "object"),
/// 2. link `C.[[Prototype]]` → %Function.prototype% (§10.2.3
///    MakeConstructor / §15.7.14: constructor functions inherit from
///    %Function.prototype%),
/// 3. define `__proto_<C>.constructor = C` with `{writable: true,
///    enumerable: false, configurable: true}` (§10.2.3 step 4).
///
/// The `constructor ↔ prototype` reference cycle is between two
/// module-scope singletons whose lifetime spans the process — no
/// leak surface.
///
/// # Safety
/// `class_anyv` is a cell-encoded AnyValue pointing at a live heap
/// object; `PROTOS_BY_TAG_IMM[tag]` was registered by the emit
/// sequence just before this call (class_globals.rs emit order).
unsafe fn wire_first_class_links(tag: i64, class_anyv: u64, parent_tag: i64) {
    unsafe {
        let class_cell = class_anyv as *mut c_void;
        __torajs_dynobj_mark_class_ctor(class_cell);
        // Step 2 — §15.7.14 class heritage: a derived class ctor's
        // [[Prototype]] IS the parent ctor (`getPrototypeOf(Sub) ===
        // Super`; §20.5.6.2 NativeError.[[Prototype]] = Error is the
        // same rule). A root class — or a parent that never
        // registered (dropout) — links %Function.prototype% per
        // §10.2.3. Registration runs in source order, so the parent
        // slot is filled before any subclass wires (extends is
        // TDZ-gated upstream). Both link targets outlive the entry
        // (registry slots are process-lifetime, the singleton is
        // immortal); ordinary_set_prototype_of rc_incs the stored
        // link itself.
        let parent = if in_range(parent_tag) {
            CLASSES_BY_TAG_IMM[parent_tag as usize]
        } else {
            0
        };
        if is_cell_imm(parent) && heap_type_tag(parent as *const c_void) == TAG_DYNOBJ {
            crate::reflect_proto_set::ordinary_set_prototype_of(class_cell, parent);
        } else {
            let func_proto = __torajs_get_builtin_prototype(FUNCTION_PROTO_TAG);
            if !func_proto.is_null() {
                crate::reflect_proto_set::ordinary_set_prototype_of(class_cell, func_proto as u64);
            }
        }
        // Step 3 — define transfers the value stake, so the entry owns
        // one reference to the class object.
        let proto = PROTOS_BY_TAG_IMM[tag as usize];
        if is_cell_imm(proto) && heap_type_tag(proto as *const c_void) == TAG_DYNOBJ {
            let key = alloc_str_key(b"constructor");
            __torajs_rc_inc(class_cell);
            let mut slot = proto as *mut c_void;
            __torajs_dynobj_define(
                &mut slot,
                key,
                ANY_HEAP as u64,
                class_anyv,
                DEFINE_CTOR_FLAGS,
            );
            __torajs_str_drop(key);
            // rotation 186 — dynobj define may RESIZE (fresh block +
            // free old); every table read after a define must see
            // the moved cell or it dereferences freed memory. Same
            // writeback on every class/proto define site below.
            PROTOS_BY_TAG_IMM[tag as usize] = slot as u64;
            reify::reify_prototype_methods(tag, slot);
        }
    }
}

/// Installs the §20.5.3.2/3.3 + §20.5.6.3/6.4 own data properties
/// on an INJECTED error class's `__proto_<C>` — `name = "<C>"` and
/// `message = ""`, both `{W:1, E:0, C:1}` (RFC
/// 20260718-builtin-error-ctor-first-class 刀 1). Only the
/// synthesized Error family runs this (a user subclass's prototype
/// carries neither per spec — it inherits them). `name` is a
/// BORROWED Str cell (the caller drops its temp); the entries take
/// their own stakes (rc_inc on the name value, a fresh mint for the
/// empty message).
///
/// # Safety
/// `name` is NULL or a live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_error_proto_install(tag: i64, name: *const c_void) {
    if !in_range(tag) || name.is_null() {
        return;
    }
    // Feed the globalThis fill's Error-family registry (see
    // classmeta/error_family.rs) — independent of the proto-shape
    // gates below.
    unsafe { error_family::record_error_family_class(tag, name) };
    // SAFETY: single-threaded JS; proto_register filled the slot
    // before this runs (class_globals.rs emit order).
    let proto = unsafe { PROTOS_BY_TAG_IMM[tag as usize] };
    if !is_cell_imm(proto) || unsafe { heap_type_tag(proto as *const c_void) } != TAG_DYNOBJ {
        return;
    }
    unsafe {
        let mut slot = proto as *mut c_void;
        let name_key = alloc_str_key(b"name");
        __torajs_rc_inc(name as *mut c_void);
        __torajs_dynobj_define(
            &mut slot,
            name_key,
            ANY_HEAP as u64,
            name as u64,
            DEFINE_CTOR_FLAGS,
        );
        __torajs_str_drop(name_key);
        let msg_key = alloc_str_key(b"message");
        let empty = alloc_str_key(b"");
        __torajs_dynobj_define(
            &mut slot,
            msg_key,
            ANY_HEAP as u64,
            empty as u64,
            DEFINE_CTOR_FLAGS,
        );
        __torajs_str_drop(msg_key);
        // §20.5.3.4 — `Error.prototype.toString` own function entry
        // (刀 4), on the ROOT prototype only: subclass prototypes
        // inherit it (`gOPD(RangeError.prototype, "toString")` is
        // undefined per spec). The cell carries the dedicated
        // ANY_METHOD_ERROR_TO_STRING mid: its dispatch arm runs the
        // §20.5.3.4 generic steps (Get name/message + abrupt) over
        // any object receiver, with a FLAG_ERROR fast lane through
        // `__torajs_error_to_string` — so the own entry,
        // `e.toString()` and `Error.prototype.toString.call({...})`
        // all agree. name / length answer from the mid's meta row
        // ("toString", 0).
        if str_is(name, b"Error") {
            let cell = __torajs_builtin_method_cell(ANY_METHOD_ERROR_TO_STRING_MID);
            let ts_key = alloc_str_key(b"toString");
            __torajs_dynobj_define(
                &mut slot,
                ts_key,
                ANY_HEAP as u64,
                cell as u64,
                DEFINE_CTOR_FLAGS,
            );
            __torajs_str_drop(ts_key);
        }
        // rotation 186 — see wire_first_class_links: a define may
        // resize; publish the moved cell.
        PROTOS_BY_TAG_IMM[tag as usize] = slot as u64;
    }
}

/// Mirror of `torajs-rc/src/any_method.rs`
/// `ANY_METHOD_ERROR_TO_STRING` (append-only ABI table; same mirror
/// discipline as the error instance layout offsets in
/// `error_to_string.rs`).
const ANY_METHOD_ERROR_TO_STRING_MID: i64 = 156;

/// Content equality of a Str cell against an ASCII literal — Str
/// length u32 @+8, data @+16 (Latin-1 for ASCII payloads).
///
/// # Safety-free: read-only; `s` was validated non-null by the caller.
fn str_is(s: *const c_void, lit: &[u8]) -> bool {
    unsafe {
        let p = s as *const u8;
        let len = (p.add(8) as *const u32).read() as usize;
        if len != lit.len() {
            return false;
        }
        core::slice::from_raw_parts(p.add(16), len) == lit
    }
}

/// `Object.getPrototypeOf(instance)` → owned AnyValue immediate.
/// Returns `VALUE_NULL_IMM` (the NaN-box `null` sentinel) on
/// out-of-range tag or unregistered class. rc_inc's the heap
/// payload when the stored value is a cell so the caller owns
/// the returned reference.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_proto_get(tag: i64) -> u64 {
    // SAFETY: single-threaded JS; reading a registered slot.
    let mut v = if in_range(tag) {
        unsafe { PROTOS_BY_TAG_IMM[tag as usize] }
    } else {
        0
    };
    if v == 0 {
        // 405-04 knife 2 — a generic specialization tag (often beyond
        // MAX_CLASSES) reads its class's MAIN slot; see
        // classmeta/generic_alias.rs.
        if let Some(main) = generic_alias::main_tag_of(tag) {
            v = unsafe { PROTOS_BY_TAG_IMM[main] };
        }
    }
    if v == 0 {
        return VALUE_NULL_IMM;
    }
    if is_cell_imm(v) {
        // SAFETY: cell-encoded AnyValue → 48-bit user-VA pointer
        // to a valid heap object.
        unsafe { __torajs_rc_inc(v as *mut c_void) };
    }
    v
}

/// `new.target` lookup — AnyValue-immediate variant. Returns
/// `VALUE_UNDEFINED_IMM` (NaN-box `undefined` sentinel) on
/// out-of-range / unregistered class.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_class_get(tag: i64) -> u64 {
    // SAFETY: as above.
    let mut v = if in_range(tag) {
        unsafe { CLASSES_BY_TAG_IMM[tag as usize] }
    } else {
        0
    };
    if v == 0 {
        // 405-04 knife 2 — generic specialization tags alias the
        // main slot, same as proto_get above.
        if let Some(main) = generic_alias::main_tag_of(tag) {
            v = unsafe { CLASSES_BY_TAG_IMM[main] };
        }
    }
    if v == 0 {
        return VALUE_UNDEFINED_IMM;
    }
    if is_cell_imm(v) {
        // SAFETY: as above.
        unsafe { __torajs_rc_inc(v as *mut c_void) };
    }
    v
}
