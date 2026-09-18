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

/// The entries that land on the class object itself.
mod statics;

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
        let layout = __torajs_struct_layout_lookup(tag as u32);
        define_accessor_pair(
            &mut slot,
            name_str,
            spelling.as_bytes(),
            super::reify::face_half(layout, tag, get_adapter),
            super::reify::face_half(layout, tag, set_adapter),
            DEFINE_ACCESSOR_FLAGS,
            0,
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
/// One half of a class accessor as its reified face carries it
/// (629-02): the boxed adapter, plus the receiver guard the invoke
/// reads — the owning class's tag (0 = every receiver rides the
/// twin) and the `__cmany_` twin's adapter (0 = none). [`Self::mono`]
/// is the guard-less spelling the runtime-key and static defines use
/// (a static body binds no receiver; a computed member's twin is not
/// resolvable by row here).
#[derive(Clone, Copy)]
pub(super) struct FaceHalf {
    pub adapter: u64,
    pub tag: u64,
    pub twin: u64,
}

impl FaceHalf {
    pub(super) fn mono(adapter: u64) -> Self {
        Self {
            adapter,
            tag: 0,
            twin: 0,
        }
    }
}

pub(super) unsafe fn define_accessor_pair(
    slot: &mut *mut c_void,
    key: *const u8,
    prop: &[u8],
    get: FaceHalf,
    set: FaceHalf,
    flags: u64,
    unit: u64,
) {
    unsafe {
        let mint_face = |half: FaceHalf, prefix: &[u8], length: u64| -> *mut c_void {
            if half.adapter == 0 {
                return core::ptr::null_mut();
            }
            let mut full = Vec::with_capacity(prefix.len() + prop.len());
            full.extend_from_slice(prefix);
            full.extend_from_slice(prop);
            let face_name = alloc_str_key(&full);
            // K2b — one evaluation's face is mortal and holds the
            // evaluation's class object; the shared by-tag prototype
            // (unit 0) keeps the immortal face.
            if unit != 0 {
                return __torajs_class_accessor_cell_new_unit(
                    half.adapter,
                    face_name,
                    length,
                    half.tag,
                    half.twin,
                    unit,
                ) as *mut c_void;
            }
            __torajs_class_accessor_cell_new(half.adapter, face_name, length, half.tag, half.twin)
                as *mut c_void
        };
        let get_cell = mint_face(get, b"get ", 0);
        let set_cell = mint_face(set, b"set ", 1);
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
        // 564-01 — the face's §10.2.9 name is the key it is being
        // defined under, not the `__ccm_<n>__` sentinel the fn-name
        // registry holds for its body; the fresh Str transfers.
        let cell = __torajs_class_method_cell_new(adapter, this_free, 0, 0, fn_name_of_key(key));
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

/// §10.2.9 SetFunctionName's spelling of the property key a
/// computed member is being defined under: a Str key IS the name
/// (a numeric key already arrived as its Str spelling), and a
/// Symbol key reads `"[<description>]"` — empty when it has none.
/// The answer is a fresh Str the caller transfers into the face.
///
/// # Safety
/// `key` is a live Str / Symbol cell.
unsafe fn fn_name_of_key(key: *const u8) -> *mut u8 {
    if unsafe { heap_type_tag(key as *const c_void) } == crate::reflect::TAG_STR {
        unsafe { __torajs_rc_inc(key as *mut c_void) };
        return key as *mut u8;
    }
    unsafe { __torajs_symbol_fn_name(key as *const c_void) }
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
        define_accessor_pair(
            &mut slot,
            key,
            prop,
            FaceHalf::mono(get_adapter),
            FaceHalf::mono(set_adapter),
            flags,
            0,
        );
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

unsafe extern "C" {
    /// torajs-dynobj — re-append one own entry, carrying its key
    /// cell, value and attributes across unchanged.
    fn __torajs_dynobj_move_own_to_end(obj: *mut c_void, key: *const c_void) -> i32;
    /// torajs-str — §10.2.9's `"[<description>]"` spelling of a
    /// Symbol property key as a function name (564-01); fresh Str.
    fn __torajs_symbol_fn_name(p: *const c_void) -> *mut u8;
}
