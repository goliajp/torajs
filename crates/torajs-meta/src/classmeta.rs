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

unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
    /// Locks `name` / `length` / `prototype` slots on the class-object
    /// dynobj to the ES §17 built-in Function attribute pattern (no-op
    /// if `obj` is NULL, non-cell, or non-dynobj). Cross-crate FFI
    /// declared here to keep the classmeta dep tree lean.
    fn __torajs_dynobj_lock_builtin_fn_class_slots(obj: *mut c_void);
}

const MAX_CLASSES: usize = 256;

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
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_anyv_class_register(tag: i64, class_anyv: u64) {
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
    }
}

/// `Object.getPrototypeOf(instance)` → owned AnyValue immediate.
/// Returns `VALUE_NULL_IMM` (the NaN-box `null` sentinel) on
/// out-of-range tag or unregistered class. rc_inc's the heap
/// payload when the stored value is a cell so the caller owns
/// the returned reference.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_proto_get(tag: i64) -> u64 {
    if !in_range(tag) {
        return VALUE_NULL_IMM;
    }
    // SAFETY: single-threaded JS; reading a registered slot.
    let v = unsafe { PROTOS_BY_TAG_IMM[tag as usize] };
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
    if !in_range(tag) {
        return VALUE_UNDEFINED_IMM;
    }
    // SAFETY: as above.
    let v = unsafe { CLASSES_BY_TAG_IMM[tag as usize] };
    if v == 0 {
        return VALUE_UNDEFINED_IMM;
    }
    if is_cell_imm(v) {
        // SAFETY: as above.
        unsafe { __torajs_rc_inc(v as *mut c_void) };
    }
    v
}
