//! Class-object own-entry defines — the `__torajs_class_*_define`
//! family split out of the parent registry module (file-size debt,
//! rotation 146). Verbatim move: the three entry points, their
//! attribute-set choices and their safety contracts are unchanged.
//!
//! All three resolve a class tag to its `__class_<C>` dynobj through
//! the parent's `CLASSES_BY_TAG_IMM` slot, mint the reified cell the
//! prototype entries already use, and hand it to `__torajs_dynobj_define`
//! with the §10.2.10 attribute set for that entry kind.

use core::ffi::c_void;

use super::*;

/// Knife B cut 2 — one static-method own entry on the class object.
/// ssa_lower hands the resolved triple (`tag`, the method-name Str
/// cell, the `__sm_<C>__<M>` boxed adapter's vaddr); the minted cell
/// is the same reified-method shape the prototype entries use, and
/// the define applies the §10.2.10 `{writable: true, enumerable:
/// false, configurable: true}` attribute set. A `name` / `length`
/// static method redefines the reflection slot the register lock
/// shaped (both are configurable, so the redefine is legal — and the
/// function-valued entry is the spec answer for a static method
/// shadow).
///
/// # Safety
/// `name_str` is a live Str cell (caller-owned; the define takes its
/// own key reference); `adapter` is a live boxed-adapter code
/// address.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_class_static_method_define(
    tag: i64,
    name_str: *const u8,
    adapter: u64,
    this_free: u64,
) {
    if !in_range(tag) || name_str.is_null() || adapter == 0 {
        return;
    }
    // SAFETY: single-threaded JS; the register sequence filled the
    // slot before any reify call (class_globals.rs emit order).
    let class_anyv = unsafe { CLASSES_BY_TAG_IMM[tag as usize] };
    if !is_cell_imm(class_anyv) {
        return;
    }
    if unsafe { heap_type_tag(class_anyv as *const c_void) } != TAG_DYNOBJ {
        return;
    }
    unsafe {
        // S2.38 — a static body never depends on a runtime receiver
        // (its `this` resolves to the class object at parse time and
        // the `__sm_` adapter drops its env argument); the compiler
        // still gates the flag on a lossless argument surface
        // (all-`Any` params, no caller-side defaults), so the emit
        // side decides and this define just carries the verdict.
        let cell = __torajs_class_method_cell_new(adapter, this_free);
        let mut slot = class_anyv as *mut c_void;
        __torajs_dynobj_define(
            &mut slot,
            name_str,
            ANY_HEAP as u64,
            cell as u64,
            DEFINE_CTOR_FLAGS,
        );
        // rotation 186 — a define may resize (fresh block + free
        // old); publish the moved cell so later table reads don't
        // dereference freed memory. Same on every sibling below.
        CLASSES_BY_TAG_IMM[tag as usize] = slot as u64;
    }
}

/// RFC L3b static-field-reflect (2026-07-22) — one static-FIELD own
/// entry on the class object. ssa_lower hands the resolved
/// `(tag, name-Str, value-tag, value)` quad: the value pair comes
/// from `box_to_tag_value` over the `__sf_<C>__<f>` global slot's
/// current value (the heap arm's rc_inc is the entry's stake — the
/// define takes it). Attribute set is the §7.3.6 data-field triple
/// `{writable: true, enumerable: true, configurable: true}`.
/// Reflection-only: the compile-time `C.<f>` fold keeps reading the
/// global slot (the static-method reify precedent's split).
///
/// # Safety
/// `name_str` is a live Str cell (caller-owned; the define takes its
/// own key reference); `vtag`/`vvalue` follow the dynobj_define pair
/// contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_class_static_field_define(
    tag: i64,
    name_str: *const u8,
    vtag: u64,
    vvalue: u64,
) {
    if !in_range(tag) || name_str.is_null() {
        return;
    }
    // SAFETY: single-threaded JS; the register sequence filled the
    // slot before any reify call (class_globals.rs emit order).
    let class_anyv = unsafe { CLASSES_BY_TAG_IMM[tag as usize] };
    if !is_cell_imm(class_anyv) {
        return;
    }
    if unsafe { heap_type_tag(class_anyv as *const c_void) } != TAG_DYNOBJ {
        return;
    }
    unsafe {
        let mut slot = class_anyv as *mut c_void;
        __torajs_dynobj_define(&mut slot, name_str, vtag, vvalue, DEFINE_FIELD_FLAGS);
        CLASSES_BY_TAG_IMM[tag as usize] = slot as u64;
    }
}

/// RFC 20260718-accessor-reify 刀 2 — one accessor own entry on the
/// class prototype. ssa_lower hands the resolved quad (`tag`, the
/// property-name Str cell, the `__cm_<C>__<M>_get` / `_set` boxed
/// adapters' vaddrs — either may be 0 for a get-/set-only pair).
/// The minted faces carry their §17 "get <p>" / "set <p>" names and
/// spec lengths (getter 0, setter 1); the pair defines under
/// `{enumerable: false, configurable: true}` (§10.2.10).
///
/// # Safety
/// `name_str` is NULL or a live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_class_accessor_define(
    tag: i64,
    name_str: *const u8,
    get_adapter: u64,
    set_adapter: u64,
) {
    if !in_range(tag) || name_str.is_null() || (get_adapter == 0 && set_adapter == 0) {
        return;
    }
    // SAFETY: single-threaded JS; the register sequence filled the
    // slot before any reify call (class_globals.rs emit order).
    let proto_anyv = unsafe { PROTOS_BY_TAG_IMM[tag as usize] };
    if !is_cell_imm(proto_anyv) {
        return;
    }
    if unsafe { heap_type_tag(proto_anyv as *const c_void) } != TAG_DYNOBJ {
        return;
    }
    unsafe {
        let prop_len = (name_str.add(8) as *const u32).read() as usize;
        let prop = core::slice::from_raw_parts(name_str.add(16), prop_len);
        let mint_face = |adapter: u64, prefix: &[u8], length: u64| -> *mut c_void {
            if adapter == 0 {
                return core::ptr::null_mut();
            }
            let mut full = Vec::with_capacity(prefix.len() + prop.len());
            full.extend_from_slice(prefix);
            full.extend_from_slice(prop);
            let face_name = alloc_str_key(&full);
            __torajs_class_accessor_cell_new(adapter, face_name, length) as *mut c_void
        };
        let get_cell = mint_face(get_adapter, b"get ", 0);
        let set_cell = mint_face(set_adapter, b"set ", 1);
        let pair = __torajs_accessor_pair_new(get_cell, set_cell, ACC_KINDS_BOXED_BOTH);
        let flags = DEFINE_ACCESSOR_FLAGS;
        let mut slot = proto_anyv as *mut c_void;
        __torajs_dynobj_define(&mut slot, name_str, ANY_HEAP as u64, pair as u64, flags);
        PROTOS_BY_TAG_IMM[tag as usize] = slot as u64;
    }
}

/// RFC 20260802-class-computed-member 刀 2 — one METHOD own entry
/// under a RUNTIME property key. `key` is the ToPropertyKey product
/// ssa_lower's `lower_key` resolved (a Str cell, or a Symbol cell
/// passed through per §7.1.19 step 2 — `__torajs_dynobj_define`'s
/// key slot is polymorphic over both). The minted cell is the same
/// reified-method shape the prototype entries use; §10.2.10 method
/// attribute set. `is_static != 0` lands the entry on the class
/// object instead of the prototype.
///
/// # Safety
/// `key` is a live Str / Symbol cell (caller-owned; the define takes
/// its own key reference); `adapter` is a live boxed-adapter code
/// address.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_class_computed_method_define(
    tag: i64,
    key: *const u8,
    adapter: u64,
    is_static: u64,
) {
    if !in_range(tag) || key.is_null() || adapter == 0 {
        return;
    }
    // SAFETY: single-threaded JS; the register sequence filled the
    // slot before the class-decl-position reify runs.
    let target_anyv = unsafe {
        if is_static != 0 {
            CLASSES_BY_TAG_IMM[tag as usize]
        } else {
            PROTOS_BY_TAG_IMM[tag as usize]
        }
    };
    if !is_cell_imm(target_anyv) {
        return;
    }
    if unsafe { heap_type_tag(target_anyv as *const c_void) } != TAG_DYNOBJ {
        return;
    }
    unsafe {
        let cell = __torajs_class_method_cell_new(adapter, 0);
        let mut slot = target_anyv as *mut c_void;
        __torajs_dynobj_define(
            &mut slot,
            key,
            ANY_HEAP as u64,
            cell as u64,
            DEFINE_CTOR_FLAGS,
        );
        if is_static != 0 {
            CLASSES_BY_TAG_IMM[tag as usize] = slot as u64;
        } else {
            PROTOS_BY_TAG_IMM[tag as usize] = slot as u64;
        }
    }
}

/// 刀 2 accessor twin — a SINGLE-face AccessorPair define under a
/// runtime key. The flags carry only the present face
/// (DEFINE_PRESENT_GET / _SET), so `get [k]` and `set [k]` whose
/// keys evaluate to the same property merge into one pair through
/// the dynobj define kernel's §7.3.9 redefine semantics (each
/// ComputedPropertyName still evaluated once, in declaration order).
/// The §17 "get <p>" face name derives from a Str key's bytes; a
/// Symbol key's face carries an empty name (recorded boundary — the
/// §10.2.9 "[description]" spelling needs the symbol's description
/// surface here).
///
/// # Safety
/// `key` is a live Str / Symbol cell; exactly one of
/// `get_adapter` / `set_adapter` is a live boxed-adapter address,
/// the other 0.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_class_computed_accessor_define(
    tag: i64,
    key: *const u8,
    get_adapter: u64,
    set_adapter: u64,
    is_static: u64,
) {
    if !in_range(tag) || key.is_null() || (get_adapter == 0 && set_adapter == 0) {
        return;
    }
    let target_anyv = unsafe {
        if is_static != 0 {
            CLASSES_BY_TAG_IMM[tag as usize]
        } else {
            PROTOS_BY_TAG_IMM[tag as usize]
        }
    };
    if !is_cell_imm(target_anyv) {
        return;
    }
    if unsafe { heap_type_tag(target_anyv as *const c_void) } != TAG_DYNOBJ {
        return;
    }
    unsafe {
        // Face name from a Str key only — a Symbol key's heap tag
        // distinguishes it (reflect.rs TAG_STR = 0 vs TAG_SYMBOL).
        let key_is_str = heap_type_tag(key as *const c_void) == crate::reflect::TAG_STR;
        let mint_face = |adapter: u64, prefix: &[u8], length: u64| -> *mut c_void {
            if adapter == 0 {
                return core::ptr::null_mut();
            }
            let mut full = prefix.to_vec();
            if key_is_str {
                let prop_len = (key.add(8) as *const u32).read() as usize;
                full.extend_from_slice(core::slice::from_raw_parts(key.add(16), prop_len));
            }
            let face_name = alloc_str_key(&full);
            __torajs_class_accessor_cell_new(adapter, face_name, length) as *mut c_void
        };
        let get_cell = mint_face(get_adapter, b"get ", 0);
        let set_cell = mint_face(set_adapter, b"set ", 1);
        let pair = __torajs_accessor_pair_new(get_cell, set_cell, ACC_KINDS_BOXED_BOTH);
        // Base accessor flags minus the both-faces-present bits; only
        // the face this call carries is marked present, so the define
        // kernel's redefine merge keeps the other face.
        let mut flags: u64 = (1 << 6) | (1 << 4) | (1 << 5) | (1 << 2);
        if get_adapter != 0 {
            flags |= 1 << 7;
        }
        if set_adapter != 0 {
            flags |= 1 << 8;
        }
        let mut slot = target_anyv as *mut c_void;
        __torajs_dynobj_define(&mut slot, key, ANY_HEAP as u64, pair as u64, flags);
        if is_static != 0 {
            CLASSES_BY_TAG_IMM[tag as usize] = slot as u64;
        } else {
            PROTOS_BY_TAG_IMM[tag as usize] = slot as u64;
        }
    }
}

/// 刀 3 static twin of [`__torajs_class_accessor_define`] — the
/// AccessorPair own entry lands on the CLASS object (`gOPD(C, "s")`;
/// §15.7.14 static accessors are class-object properties).
///
/// # Safety
/// `name_str` is NULL or a live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_class_static_accessor_define(
    tag: i64,
    name_str: *const u8,
    get_adapter: u64,
    set_adapter: u64,
) {
    if !in_range(tag) || name_str.is_null() || (get_adapter == 0 && set_adapter == 0) {
        return;
    }
    let class_anyv = unsafe { CLASSES_BY_TAG_IMM[tag as usize] };
    if !is_cell_imm(class_anyv) {
        return;
    }
    if unsafe { heap_type_tag(class_anyv as *const c_void) } != TAG_DYNOBJ {
        return;
    }
    unsafe {
        let prop_len = (name_str.add(8) as *const u32).read() as usize;
        let prop = core::slice::from_raw_parts(name_str.add(16), prop_len);
        let mint_face = |adapter: u64, prefix: &[u8], length: u64| -> *mut c_void {
            if adapter == 0 {
                return core::ptr::null_mut();
            }
            let mut full = Vec::with_capacity(prefix.len() + prop.len());
            full.extend_from_slice(prefix);
            full.extend_from_slice(prop);
            let face_name = alloc_str_key(&full);
            __torajs_class_accessor_cell_new(adapter, face_name, length) as *mut c_void
        };
        let get_cell = mint_face(get_adapter, b"get ", 0);
        let set_cell = mint_face(set_adapter, b"set ", 1);
        let pair = __torajs_accessor_pair_new(get_cell, set_cell, ACC_KINDS_BOXED_BOTH);
        let mut slot = class_anyv as *mut c_void;
        __torajs_dynobj_define(
            &mut slot,
            name_str,
            ANY_HEAP as u64,
            pair as u64,
            DEFINE_ACCESSOR_FLAGS,
        );
        CLASSES_BY_TAG_IMM[tag as usize] = slot as u64;
    }
}
