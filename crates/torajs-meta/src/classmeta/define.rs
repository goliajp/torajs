//! Class-object own-entry defines — the `__torajs_class_*_define`
//! family split out of the parent registry module (file-size debt,
//! rotation 146). Verbatim move: the three entry points, their
//! attribute-set choices and their safety contracts are unchanged.
//!
//! All three resolve a class tag to its `__class_<C>` dynobj through
//! the parent's `CLASSES_BY_TAG_IMM` slot, mint the reified cell the
//! prototype entries already use, and hand it to `__torajs_dynobj_define_plain` (the assembly-path narrow kernel — every receiver here is a gated plain dynobj, RFC 20260825-inject-narrow-define 刀 1)
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
    twin: u64,
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
        // S2.38 — a static body's `this` resolves to the class object
        // at compile time (the `__sm_` adapter drops its env
        // argument); the compiler still gates the flag on a lossless
        // argument surface (all-`Any` params, no caller-side
        // defaults), so the emit side decides and this define just
        // carries the verdict.
        // RFC 20260804-fn-this-channel knife 3b — `twin` is the
        // `__smany_` receiver-polymorphic body's boxed adapter (0 =
        // this-free / mint residue). A STATIC face encodes as
        // `(tag 0, twin ≠ 0)` in the guard pair: the mono body has
        // no receiver channel at all, so a `.call/.apply` rebind
        // must ALWAYS take the twin — there is no receiver value
        // the mono path could honor (the instance guard's tag
        // compare has nothing to compare against).
        let cell = __torajs_class_method_cell_new(adapter, this_free, 0, twin);
        let mut slot = class_anyv as *mut c_void;
        __torajs_dynobj_define_plain(
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
        __torajs_dynobj_define_plain(&mut slot, name_str, vtag, vvalue, DEFINE_FIELD_FLAGS);
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
        let spelling = crate::str_wtf8::StrWtf8::of(name_str.cast());
        let mut slot = proto_anyv as *mut c_void;
        define_accessor_pair(
            &mut slot,
            name_str,
            spelling.as_bytes(),
            get_adapter,
            set_adapter,
            DEFINE_ACCESSOR_FLAGS,
        );
        PROTOS_BY_TAG_IMM[tag as usize] = slot as u64;
    }
}

/// Mint the §17-named `get <p>` / `set <p>` faces for `prop` and
/// define the pair onto `slot` under `key`. The one mint shared by
/// the instance / computed / static accessor defines and by the
/// prototype walk that places an accessor at its DECLARATION
/// position (`reify.rs`) — a face's spec length is 0 for a getter
/// and 1 for a setter, and an absent half mints nothing.
///
/// # Safety
/// `slot` holds a live dynobj; `key` is a live Str / Symbol cell.
pub(super) unsafe fn define_accessor_pair(
    slot: &mut *mut c_void,
    key: *const u8,
    prop: &[u8],
    get_adapter: u64,
    set_adapter: u64,
    flags: u64,
) {
    unsafe {
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
        __torajs_dynobj_define_plain(slot, key, ANY_HEAP as u64, pair as u64, flags);
    }
}

/// RFC 20260802-class-computed-member 刀 2 — one METHOD own entry
/// under a RUNTIME property key. `key` is the ToPropertyKey product
/// ssa_lower's `lower_key` resolved (a Str cell, or a Symbol cell
/// passed through per §7.1.19 step 2 — the define kernel's
/// key slot is polymorphic over both). The minted cell is the same
/// reified-method shape the prototype entries use; §10.2.10 method
/// attribute set. `is_static != 0` lands the entry on the class
/// object instead of the prototype.
///
/// `this_free != 0` — the S2.38 verdict ssa_lower proved for a static
/// body (never reads a runtime receiver, lossless argv face), so the
/// cell admits a detached bare call. It used to be hardcoded 0 here,
/// which is why `const f = C[Symbol.hasInstance]; f(4)` threw "class
/// method called without a receiver" while the identically shaped
/// `C.named` ran fine.
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
    this_free: u64,
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
        // tag 0 / twin 0 — this runtime-define mint site has no
        // class-tag context; the blade-3 guard stays disarmed.
        let cell = __torajs_class_method_cell_new(adapter, this_free, 0, 0);
        let mut slot = target_anyv as *mut c_void;
        __torajs_dynobj_define_plain(
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
            // 562-07 — the entry could only be appended; the rows this
            // class declares AFTER it move behind it so the own keys
            // read in element order.
            if let Some(row) = super::reify::row_of_adapter(tag, adapter) {
                super::reify::redefine_rows_after(tag, row);
            }
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
        let spelling = crate::str_wtf8::StrWtf8::of(key.cast());
        // A Symbol key names no face spelling — the prefix alone.
        let prop: &[u8] = if key_is_str { spelling.as_bytes() } else { b"" };
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
        define_accessor_pair(&mut slot, key, prop, get_adapter, set_adapter, flags);
        if is_static != 0 {
            CLASSES_BY_TAG_IMM[tag as usize] = slot as u64;
        } else {
            PROTOS_BY_TAG_IMM[tag as usize] = slot as u64;
            // 562-07 — same re-appending as the method twin. The row
            // is found by whichever face this call carries; the pair's
            // two rows are adjacent, so the second half's own
            // re-appending is a no-op over the first's.
            let face = if get_adapter != 0 {
                get_adapter
            } else {
                set_adapter
            };
            if let Some(row) = super::reify::row_of_adapter(tag, face) {
                super::reify::redefine_rows_after(tag, row);
            }
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
        let spelling = crate::str_wtf8::StrWtf8::of(name_str.cast());
        let mut slot = class_anyv as *mut c_void;
        define_accessor_pair(
            &mut slot,
            name_str,
            spelling.as_bytes(),
            get_adapter,
            set_adapter,
            DEFINE_ACCESSOR_FLAGS,
        );
        CLASSES_BY_TAG_IMM[tag as usize] = slot as u64;
    }
}

unsafe extern "C" {
    /// torajs-dynobj — re-append one own entry, carrying its key
    /// cell, value and attributes across unchanged.
    fn __torajs_dynobj_move_own_to_end(obj: *mut c_void, key: *const c_void) -> i32;
}

/// 563-03 — move the class object's own `<name>` entry behind
/// everything defined so far. §15.7.14 defines every static element
/// in ONE ordered pass, but a COMPUTED static member's key exists
/// only at the class-decl position, long after the prologue's
/// registration walk defined the plain members, and an own entry can
/// only be APPENDED. So the members declared after the computed one
/// are moved behind it — one emitted call each, ordered by the class
/// element list.
///
/// The instance side answers the same question from the class's
/// method table ([`super::reify::redefine_rows_after`]); a class
/// object has no such table, its members arrive one emitted reify at
/// a time, so the order is repaired from the emit side instead.
/// Nothing can observe the intermediate states: this runs inside the
/// class definition, before the binding exists.
///
/// # Safety
/// `name_str` is NULL or a live Str cell (caller-owned — the entry
/// keeps the key cell it already owns).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_class_static_own_move_to_end(tag: i64, name_str: *const u8) {
    if !in_range(tag) || name_str.is_null() {
        return;
    }
    let class_anyv = unsafe { CLASSES_BY_TAG_IMM[tag as usize] };
    if !is_cell_imm(class_anyv) {
        return;
    }
    unsafe {
        if heap_type_tag(class_anyv as *const c_void) != TAG_DYNOBJ {
            return;
        }
        __torajs_dynobj_move_own_to_end(class_anyv as *mut c_void, name_str.cast::<c_void>());
    }
}
