//! `<key> in <obj>` for `Type::Any` rhs operands.
//!
//! `in` is HasProperty (ES §13.10.1 → §7.3.12): own descriptor
//! first, then the receiver's prototype chain. ssa_lower's static
//! `in`-op path (ssa_lower.rs T-45) dispatches on the rhs operand's
//! SSA type and routes the `Type::Any` arm here, picked by the
//! **key**'s static SSA type:
//!
//!   `__torajs_in_op_any_num(v, key_i64)` — key is Number-typed.
//!     `Tag::Arr` keeps an inline bounds + hole probe (no string
//!     alloc on the hot indexed shape); every other receiver mints
//!     the canonical decimal string once and takes the full
//!     string-keyed face below.
//!
//!   `__torajs_in_op_any_str(v, key_str_ptr)` — key is String-typed.
//!     • own face — `__torajs_any_prop_has`, the SAME own-property
//!       predicate `hasOwnProperty` / `propertyIsEnumerable` answer
//!       from (torajs-anyvalue `prop_has`): dynobj entries + proto-
//!       singleton interned methods, Arr indices/holes/length/side
//!       props, closure name/length tombstones + expandos, struct
//!       fields + accessor slots, wrapper faces. One predicate, not
//!       a second copy of the walk (rotation 148: a support table
//!       drifting from its dispatcher is the recorded failure mode).
//!     • chain face — `__torajs_proto_chain_key_owned` against the
//!       receiver's builtin family (`proto_family_of`): the
//!       `<Ctor>.prototype` link, then the `Object.prototype` root.
//!       `"map" in xs` / `"call" in fn` / `"toString" in {}` are
//!       chain answers, not own answers.
//!
//! Both kernels share `require_object_rhs` (§13.10.1 step 5): a
//! non-Object rhs is a TypeError, not a `false` answer — immediates
//! fail the ANY_HEAP tag gate, the three heap-resident primitives
//! (string, symbol, bigint) are rejected on `type_tag`.
//!
//! A `Tag::Obj` receiver gets one extra link before the Object
//! root: `__torajs_struct_proto_member_has`, the class-prototype
//! face (methods + accessor halves — prototype properties per
//! class semantics, which is why they are chain answers while the
//! own face keeps saying false to `hasOwnProperty`).
//!
//! Recorded boundary: after `delete fn.name`, `"name" in fn`
//! answers false where bun walks to `Function.prototype`'s own
//! `name` (same boundary as the member_set readonly walk).

use core::ffi::c_void;

#[cfg(not(test))]
unsafe extern "C" {
    fn __torajs_anyv_unbox_tag(v: i64) -> i64;
    fn __torajs_anyv_unbox_value(v: i64) -> i64;
    fn __torajs_dynobj_has(obj: *const c_void, key: *const u8) -> i32;
    // torajs-dynobj — hole-tombstone probe (an elision / deleted
    // index reads undefined but is NOT an own property).
    fn __torajs_dynobj_entry_is_hole(obj: *const c_void, key: *const u8) -> i32;
    // torajs-num runtime symbol — resolved at `tr build` link time;
    // torajs-rc keeps 0 Cargo deps (vision §2). Same pattern as
    // dynobj_has / anyv_unbox above.
    fn __torajs_num_to_string_radix_i(n: i64, radix: i64) -> *mut u8;
    // torajs-str — full Str release (dec + free at rc 0); the bare
    // `__torajs_rc_dec` only decrements and answers DropPolicy.
    // Link-time-resolved, zero-Cargo-dep pattern (same as above).
    #[cfg(not(test))]
    fn __torajs_str_drop(s: *mut c_void);
    // torajs-anyvalue — the shared own-property predicate (see
    // module doc). Link-time-resolved, zero-Cargo-dep pattern.
    fn __torajs_any_prop_has(recv: u64, key: *const c_void) -> i64;
    // torajs-anyvalue — §7.3.11 HasProperty. Asked only for a SYMBOL
    // key: `member_get_symbol` owns that key domain end to end, and
    // routing here is what keeps `sym in o` and `o[sym]` answering
    // the same chain.
    fn __torajs_any_has_property(recv: u64, key: *const c_void) -> i64;
    // torajs-anyvalue — borrowed user-[[Prototype]] cell of a DynObj
    // (NULL = null-proto / implicit chain / non-cell).
    fn __torajs_dynobj_user_proto(dynobj: *const c_void) -> *mut c_void;
    // torajs-anyvalue — prototype-chain membership against the
    // interned family tables (`<Ctor>.prototype` link + the
    // `Object.prototype` root; delete tombstones consulted inside).
    fn __torajs_proto_chain_key_owned(family_tag: i64, key: *const c_void) -> i64;
    // torajs-anyvalue — class-prototype membership (methods +
    // accessor halves) for a `Tag::Obj` receiver, the chain link
    // between the instance's own face and the Object root.
    fn __torajs_struct_proto_member_has(ptr: *const c_void, key: *const c_void) -> i64;
    // torajs-anyvalue — the buffer family's name-level prototype
    // face (accessor names + interned methods; never a getter call).
    fn __torajs_buffer_family_proto_key(recv: *const c_void, tag: i64, key: *const c_void) -> i64;
    // torajs-throw — arms a pending TypeError and returns; the
    // caller must return on its own (see the C→Rust port playbook
    // B-2: a `-> !` signature here would let LLVM DCE the resume
    // path).
    fn __torajs_throw_type_error(msg: *const u8);
}

// Offset of the i64 `len` slot inside the Array heap block — matches
// `ARR_LEN_OFF = 8` in ssa_lower.rs (8 bytes after the universal
// HeapHeader).
const ARR_LEN_OFF: usize = 8;

// Offset of the inline props-dynobj slot inside the Array heap block
// (`torajs_arr::layout::ARR_PROPS_OFF`) — NULL until the array takes
// its first non-index property.
const ARR_PROPS_OFF: usize = 24;

// Byte offset of `cap` (u32) — the materialized-extent bound while
// `FLAG_ARR_SPARSE_TAIL` is up (torajs-arr `layout::ARR_CAP_OFF`).
const ARR_CAP_OFF: usize = 16;

// Tag value ssa_lower emits for NaN-boxed heap pointers (mirrors
// `ANY_TAG_HEAP = 4` from torajs-anyvalue / ssa_lower).
const ANY_TAG_HEAP: i64 = 4;

// DynObj header-flags bit for a `Object.create(null)` receiver
// (mirror of `torajs-anyvalue::member_get_own::
// DYNOBJ_HDR_FLAG_NULL_PROTO`; header flags half-word at offset 6).
const DYNOBJ_HDR_FLAG_NULL_PROTO: u16 = 1 << 6;

// `torajs_rc::Tag` numeric values (stable ABI per assert_eq! suite
// in `lib.rs`):
const TAG_ARR: u16 = 2;
// The three primitives that live in heap cells. They carry a value,
// not an object, so `in` rejects them the same way it rejects the
// immediate primitives (see `require_object_rhs`).
const TAG_STR: u16 = 0;
const TAG_SYMBOL: u16 = 7;
const TAG_BIGINT: u16 = 10;

/// True when a property-KEY cell is a Symbol rather than a Str — the
/// §6.1.7 key-domain split, read off the cell's own header `type_tag`.
/// (Distinct from [`TAG_SYMBOL`]'s use above, which classifies a
/// RECEIVER cell.)
///
/// # Safety
/// `key` must be non-NULL and point at a live key cell.
#[inline]
unsafe fn key_cell_is_symbol(key: *const u8) -> bool {
    unsafe { *(key.add(4) as *const u16) == TAG_SYMBOL }
}

/// ES §13.10.1 step 5 — `in` demands an Object on the right; every
/// other rhs is a TypeError, not a `false` answer.
///
/// Returns the object cell and its `type_tag@+4` on success. On a
/// non-Object rhs it arms a pending TypeError and returns `None`,
/// leaving the caller to return (playbook B-2 — the throw helper is
/// void, so the caller owns the control flow).
///
/// Non-Objects come in two shapes: the immediates (undefined, null,
/// number, boolean, short string), which fail the ANY_HEAP tag gate,
/// and the three heap-resident primitives (string, symbol, bigint),
/// which pass the tag gate and have to be rejected on `type_tag`.
/// Everything else that reaches a heap cell — dynobj, array, struct,
/// closure, the primitive wrappers — is an object and gets through.
///
/// # Safety
/// `v` is an unconstrained i64; defensive on every step.
/// `pub` for the `#x in o` brand-check kernel (torajs-anyvalue
/// `member_get_private`) — same step-5 contract.
pub unsafe fn require_object_rhs(v: i64) -> Option<(*const c_void, u16)> {
    let reject = || {
        unsafe {
            __torajs_throw_type_error(b"Right hand side of 'in' should be an object\0".as_ptr());
        }
        None
    };
    if unsafe { __torajs_anyv_unbox_tag(v) } != ANY_TAG_HEAP {
        return reject();
    }
    // A ShortStr reports Heap too, and unbox_value would MATERIALIZE
    // an rc=1 Str the reject path then abandons (546-02 M1 family) —
    // a string primitive rhs is the same §13.10.1 TypeError, decided
    // on the bit test with no materialization.
    if !crate::ffi::nan_box_is_cell_like(v as u64 as *mut c_void) {
        return reject();
    }
    let ptr = unsafe { __torajs_anyv_unbox_value(v) } as *const c_void;
    if ptr.is_null() {
        return reject();
    }
    let type_tag = unsafe { *((ptr as *const u8).add(4) as *const u16) };
    if matches!(type_tag, TAG_STR | TAG_SYMBOL | TAG_BIGINT) {
        return reject();
    }
    Some((ptr, type_tag))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_in_op_any_num(v: i64, key: i64) -> bool {
    let Some((ptr, type_tag)) = (unsafe { require_object_rhs(v) }) else {
        return false;
    };
    if type_tag == TAG_ARR {
        let len = unsafe { *((ptr as *const u8).add(ARR_LEN_OFF) as *const i64) };
        // Sparse tail (RFC 20260810-arr-sparse-grow) — `[extent,
        // len)` is implicit holes: skip the fast own-answer and fall
        // to the string face, whose chain walk consults the
        // prototype digit keys (same continuation as an explicit
        // hole).
        let sparse_tail = unsafe { *((ptr as *const u8).add(6) as *const u16) }
            & crate::FLAG_ARR_SPARSE_TAIL
            != 0
            && key >= 0
            && {
                let cap = unsafe { *((ptr as *const u8).add(ARR_CAP_OFF) as *const u32) } as i64;
                key >= cap
            };
        if key >= 0 && key < len && !sparse_tail {
            // §13.10.1 HasProperty — a hole (elision / delete /
            // length-grow) is absent as an OWN property, but the walk
            // continues along the chain (刀 5 G3): a holed index falls
            // to the string face below, whose chain walk consults the
            // Array.prototype / %Object.prototype% digit keys.
            // Exotic-index header bit gates the probe.
            let mut hole = false;
            if unsafe { *((ptr as *const u8).add(6) as *const u16) } & crate::FLAG_ARR_EXOTIC_INDEX
                != 0
            {
                let key_str = unsafe { __torajs_num_to_string_radix_i(key, 10) };
                if !key_str.is_null() {
                    let props =
                        unsafe { *((ptr as *const u8).add(ARR_PROPS_OFF) as *const *const c_void) };
                    hole = !props.is_null()
                        && unsafe { __torajs_dynobj_has(props, key_str) } != 0
                        && unsafe { __torajs_dynobj_entry_is_hole(props, key_str) } != 0;
                    unsafe { rc_dec_temp_str(key_str) };
                }
            }
            if !hole {
                return true;
            }
        }
        // Out-of-bounds (and holes) fall through: `arr[9] = x` on a
        // len-3 array lands a side-props entry the string face still
        // owns, and `"constructor" in arr`-style chain names never
        // take the numeric path anyway (a decimal key interns no mid).
    }
    // Every other receiver (and the Arr out-of-bounds tail): mint the
    // canonical decimal string once and take the full string-keyed
    // face — own + chain, one code path (spec: the key is ToString'd,
    // `0 in obj` is true when `obj["0"]` exists).
    let key_str = unsafe { __torajs_num_to_string_radix_i(key, 10) };
    if key_str.is_null() {
        return false;
    }
    let r = unsafe { __torajs_in_op_any_str(v, key_str) };
    unsafe { rc_dec_temp_str(key_str) };
    r
}

#[cfg(not(test))]
unsafe fn rc_dec_temp_str(p: *mut u8) {
    // Full Str release, not a bare `__torajs_rc_dec` — that helper
    // only decrements and ANSWERS DropPolicy; swallowing the answer
    // leaked every minted decimal key (刀 5 G3 churn probe, ~32B per
    // numeric `in` miss).
    unsafe {
        __torajs_str_drop(p as *mut c_void);
    }
}

#[cfg(test)]
unsafe fn rc_dec_temp_str(_p: *mut u8) {
    // Tests construct mock str blocks on the stack — they're not
    // refcount-managed and the rc_dec path would underflow into the
    // drop dispatch. No-op here mirrors the test stubs below.
}

/// `<key:string> in <v:any>` — String-keyed dispatch: own face, then
/// the prototype chain (module doc).
///
/// # Safety
/// `v` is an unconstrained i64; `key` must be a torajs-str heap block
/// pointer (the same shape ssa_lower already passes to
/// `__torajs_dynobj_has`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_in_op_any_str(v: i64, key: *const u8) -> bool {
    let Some((ptr, type_tag)) = (unsafe { require_object_rhs(v) }) else {
        return false;
    };
    // §6.1.7 — a symbol key is a wholly separate key domain, and every
    // face below this line is name-keyed (index decode, class-proto
    // members, interned family methods, buffer names). torajs-anyvalue
    // owns the symbol chain for the READ face; asking it here is what
    // makes `sym in o` and `o[sym]` agree. This used to be a `return
    // false` further down, past the user-[[Prototype]] walk, so
    // `Symbol.iterator in [1]` was false while `[1][Symbol.iterator]`
    // was a function, and a `Object.defineProperty(Array.prototype,
    // sym, …)` patch was invisible to `in`.
    if unsafe { key_cell_is_symbol(key) } {
        return unsafe { __torajs_any_has_property(v as u64, key as *const c_void) } != 0;
    }
    if unsafe { __torajs_any_prop_has(v as u64, key as *const c_void) } != 0 {
        return true;
    }
    // §7.3.11 HasProperty walks the user [[Prototype]] chain — an
    // `Object.create(parent)` receiver answers its inherited keys
    // (RFC 20260721 刀 5 R-F). Recursion covers grandparents and
    // ends at the parent's own builtin-family chain face.
    if type_tag == crate::Tag::DynObj as u16 {
        let parent = unsafe { __torajs_dynobj_user_proto(ptr) };
        if !parent.is_null() {
            return unsafe { __torajs_in_op_any_str(parent as i64, key) };
        }
        // A NULL parent is ambiguous: `Object.create(null)` has no
        // chain AT ALL (its face check must stop here — the expando
        // probe below would wrongly surface Object.prototype's
        // `__proto__` accessor), while an implicit-chain receiver
        // falls to the family face. Header flag disambiguates
        // (`member_get_own::DYNOBJ_HDR_FLAG_NULL_PROTO` mirror).
        if unsafe { *((ptr as *const u8).add(6) as *const u16) } & DYNOBJ_HDR_FLAG_NULL_PROTO != 0 {
            return false;
        }
    }
    // 405-01 substrate — a re-parented FUNCTION value carries its
    // user [[Prototype]] link on the lazy expando dynobj at +24 (the
    // same `\x00proto` simulation entry), so `"s" in D` after
    // `Object.setPrototypeOf(D, P)` walks up like the dynobj arm.
    // An explicit null ends the chain before the builtin face.
    if type_tag == crate::Tag::Closure as u16 {
        let props = unsafe { *((ptr as *const u8).add(24) as *const u64) } as *mut c_void;
        if !props.is_null() {
            let parent = unsafe { __torajs_dynobj_user_proto(props) };
            if !parent.is_null() {
                return unsafe { __torajs_in_op_any_str(parent as i64, key) };
            }
            if unsafe { *((props as *const u8).add(6) as *const u16) } & DYNOBJ_HDR_FLAG_NULL_PROTO
                != 0
            {
                return false;
            }
        }
    }
    // Class-prototype link — a struct receiver's methods / accessor
    // halves live on its class prototype, not on the instance
    // (`hasOwnProperty` answers false, `in` answers true), one hop
    // before the Object root.
    if type_tag == crate::Tag::Obj as u16
        && (unsafe { __torajs_struct_proto_member_has(ptr, key as *const c_void) }) != 0
    {
        return true;
    }
    // §25.1 / §23.2 / §25.3 — the buffer family has no reified
    // prototype singleton yet; its prototype face is the name-level
    // predicate torajs-anyvalue owns, then the Object root like
    // every other chain.
    if type_tag == crate::Tag::ArrayBuffer as u16
        || type_tag == crate::Tag::TypedArray as u16
        || type_tag == crate::Tag::DataView as u16
    {
        if unsafe {
            __torajs_buffer_family_proto_key(
                ptr as *const c_void,
                type_tag as i64,
                key as *const c_void,
            )
        } != 0
        {
            return true;
        }
        let root = crate::builtin_proto::OBJECT_PROTO_TAG as i64;
        return (unsafe { __torajs_proto_chain_key_owned(root, key as *const c_void) }) != 0;
    }
    match crate::in_op_family::proto_family_of(ptr, type_tag) {
        Some(family) => {
            (unsafe { __torajs_proto_chain_key_owned(family, key as *const c_void) }) != 0
        }
        None => false,
    }
}

// Cargo-test stubs for the NaN-box / prop_has / chain externs. Real
// symbols live in torajs-anyvalue / torajs-dynobj / torajs-num; unit
// tests here hand-roll a NaN-box + result thread-locals so the
// kernel's dispatch (rhs gate → own face → chain face) verifies in
// isolation. Own-face and chain-face INTERNALS are covered by their
// home crates' tests + the conformance fixtures.
#[cfg(test)]
unsafe fn __torajs_anyv_unbox_tag(v: i64) -> i64 {
    // Faithful to the real encoding for the cell shape: a heap box
    // IS the raw pointer (top16 zero), which is what lets the
    // kernel's `nan_box_is_cell_like` gate run against test values.
    // Non-cell mock shapes keep the legacy `tag << 48` spelling.
    if crate::ffi::nan_box_is_cell_like(v as u64 as *mut c_void) {
        return ANY_TAG_HEAP;
    }
    ((v as u64 >> 48) & 0xFFFF) as i64
}

#[cfg(test)]
unsafe fn __torajs_anyv_unbox_value(v: i64) -> i64 {
    if crate::ffi::nan_box_is_cell_like(v as u64 as *mut c_void) {
        return v;
    }
    (v as u64 & 0x0000_FFFF_FFFF_FFFF) as i64
}

#[cfg(test)]
unsafe fn __torajs_dynobj_has(_obj: *const c_void, _key: *const u8) -> i32 {
    // The num kernel's Arr hole probe is exotic-index-gated; tests
    // never set the flag, so this stays unreached there.
    0
}

#[cfg(test)]
unsafe fn __torajs_dynobj_entry_is_hole(_obj: *const c_void, _key: *const u8) -> i32 {
    0
}

#[cfg(test)]
unsafe fn __torajs_any_prop_has(_recv: u64, _key: *const c_void) -> i64 {
    PROP_HAS_RESULT.with(|r| r.get())
}

#[cfg(test)]
unsafe fn __torajs_any_has_property(_recv: u64, _key: *const c_void) -> i64 {
    PROP_HAS_RESULT.with(|r| r.get())
}

#[cfg(test)]
unsafe fn __torajs_dynobj_user_proto(_dynobj: *const c_void) -> *mut c_void {
    // Unit tests never build a user [[Prototype]] chain — the chain
    // hop is covered by the conformance fixtures.
    core::ptr::null_mut()
}

#[cfg(test)]
unsafe fn __torajs_proto_chain_key_owned(family_tag: i64, _key: *const c_void) -> i64 {
    CHAIN_SEEN_FAMILY.with(|f| f.set(family_tag));
    CHAIN_RESULT.with(|r| r.get())
}

#[cfg(test)]
unsafe fn __torajs_struct_proto_member_has(_ptr: *const c_void, _key: *const c_void) -> i64 {
    STRUCT_PROTO_RESULT.with(|r| r.get())
}

#[cfg(test)]
unsafe fn __torajs_buffer_family_proto_key(
    _recv: *const c_void,
    _tag: i64,
    _key: *const c_void,
) -> i64 {
    // Unit tests never build a buffer-family receiver.
    0
}

#[cfg(test)]
unsafe fn __torajs_throw_type_error(_msg: *const u8) {
    // The real symbol records a pending throw in TLS for ssa_lower's
    // emit_throw_check to propagate; here we only need to observe
    // that the rhs gate fired, since the return value alone cannot
    // tell "not an own property" from "not an object".
    THROWN.with(|c| c.set(c.get() + 1));
}

#[cfg(test)]
thread_local! {
    static THROWN: core::cell::Cell<u32> = const { core::cell::Cell::new(0) };
    static PROP_HAS_RESULT: core::cell::Cell<i64> = const { core::cell::Cell::new(0) };
    static CHAIN_RESULT: core::cell::Cell<i64> = const { core::cell::Cell::new(0) };
    static CHAIN_SEEN_FAMILY: core::cell::Cell<i64> = const { core::cell::Cell::new(-1) };
    static STRUCT_PROTO_RESULT: core::cell::Cell<i64> = const { core::cell::Cell::new(0) };
}

#[cfg(test)]
fn thrown_by(f: impl FnOnce() -> bool) -> (bool, bool) {
    THROWN.with(|c| c.set(0));
    let r = f();
    (r, THROWN.with(|c| c.get()) > 0)
}

// Test stub for the cross-crate num→str alloc. Returns a static-
// buffer pointer the stubs above never deref-read. Pairs with
// rc_dec_temp_str's cfg(test) no-op so the mock pointer never enters
// the drop-dispatch path.
#[cfg(test)]
static TEST_NUM_TO_STR_BUF: [u8; 16] = [0u8; 16];

#[cfg(test)]
unsafe fn __torajs_num_to_string_radix_i(_n: i64, _radix: i64) -> *mut u8 {
    TEST_NUM_TO_STR_BUF.as_ptr() as *mut u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Tag;

    fn make_heap_block(type_tag: u16) -> Vec<u8> {
        // 32 bytes: 4B refcount + 2B type_tag + 2B flags + payload.
        // A Tag::Closure receiver's chain probe reads the props slot
        // at +24 (405-01), so the mock must be at least that wide; a
        // zero slot there means "no expando" and skips the walk.
        let mut block = vec![0u8; 32];
        block[4..6].copy_from_slice(&type_tag.to_ne_bytes());
        block
    }

    fn make_arr_heap_block(len: i64) -> Vec<u8> {
        let mut block = make_heap_block(TAG_ARR);
        block[ARR_LEN_OFF..ARR_LEN_OFF + 8].copy_from_slice(&len.to_ne_bytes());
        block
    }

    fn nan_box(tag: i64, value: i64) -> i64 {
        (tag << 48) | (value & 0x0000_FFFF_FFFF_FFFF)
    }

    fn boxed_cell(block: &[u8]) -> i64 {
        // Real encoding: a heap box IS the raw pointer (the mock's
        // stack address has top16 zero and 8-alignment on every
        // supported target), so the cell-likeness gate passes.
        block.as_ptr() as i64
    }

    fn set_faces(own: i64, chain: i64) {
        PROP_HAS_RESULT.with(|r| r.set(own));
        CHAIN_RESULT.with(|r| r.set(chain));
        CHAIN_SEEN_FAMILY.with(|f| f.set(-1));
        STRUCT_PROTO_RESULT.with(|r| r.set(0));
    }

    #[test]
    fn str_struct_class_proto_link_answers_between_own_and_root() {
        set_faces(0, 0);
        STRUCT_PROTO_RESULT.with(|r| r.set(1));
        let block = make_heap_block(Tag::Obj as u16);
        let boxed = boxed_cell(&block);
        assert!(unsafe { __torajs_in_op_any_str(boxed, b"m\0".as_ptr()) });
        // The class link answered before the Object-root probe ran.
        assert_eq!(CHAIN_SEEN_FAMILY.with(|f| f.get()), -1);
    }

    #[test]
    fn str_class_proto_link_is_struct_only() {
        // A primed struct-proto stub must not leak into non-Obj
        // receivers — a dynobj miss still walks to the Object root.
        set_faces(0, 0);
        STRUCT_PROTO_RESULT.with(|r| r.set(1));
        let block = make_heap_block(Tag::DynObj as u16);
        let boxed = boxed_cell(&block);
        assert!(!unsafe { __torajs_in_op_any_str(boxed, b"m\0".as_ptr()) });
    }

    #[test]
    fn num_arr_key_in_bounds_returns_true() {
        set_faces(0, 0);
        let block = make_arr_heap_block(3);
        let boxed = boxed_cell(&block);
        assert!(unsafe { __torajs_in_op_any_num(boxed, 0) });
        assert!(unsafe { __torajs_in_op_any_num(boxed, 2) });
    }

    #[test]
    fn num_arr_key_out_of_bounds_falls_to_string_face() {
        // OOB numeric key on an Arr delegates to the string face —
        // both faces miss here, so the answer is false (a side-props
        // own entry would flip PROP_HAS_RESULT in real linkage).
        set_faces(0, 0);
        let block = make_arr_heap_block(3);
        let boxed = boxed_cell(&block);
        assert!(!unsafe { __torajs_in_op_any_num(boxed, 3) });
        assert!(!unsafe { __torajs_in_op_any_num(boxed, -1) });
        set_faces(1, 0);
        assert!(unsafe { __torajs_in_op_any_num(boxed, 9) });
    }

    #[test]
    fn num_non_arr_receiver_delegates_to_string_face() {
        set_faces(1, 0);
        let block = make_heap_block(Tag::DynObj as u16);
        let boxed = boxed_cell(&block);
        assert!(unsafe { __torajs_in_op_any_num(boxed, 0) });
        set_faces(0, 0);
        assert!(!unsafe { __torajs_in_op_any_num(boxed, 0) });
    }

    #[test]
    fn num_non_heap_tag_throws() {
        // §13.10.1 step 5 — a number / boolean / undefined rhs is a
        // TypeError, not a false answer.
        let boxed = nan_box(2, 0x1234_5678);
        assert_eq!(
            thrown_by(|| unsafe { __torajs_in_op_any_num(boxed, 0) }),
            (false, true)
        );
    }

    #[test]
    fn num_null_ptr_throws() {
        // A null cell is the `null` rhs, which §13.10.1 rejects too.
        let boxed = nan_box(ANY_TAG_HEAP, 0);
        assert_eq!(
            thrown_by(|| unsafe { __torajs_in_op_any_num(boxed, 0) }),
            (false, true)
        );
    }

    #[test]
    fn str_own_face_hit_returns_true() {
        set_faces(1, 0);
        let block = make_heap_block(Tag::DynObj as u16);
        let boxed = boxed_cell(&block);
        assert!(unsafe { __torajs_in_op_any_str(boxed, b"foo\0".as_ptr()) });
    }

    #[test]
    fn str_chain_face_answers_after_own_miss() {
        set_faces(0, 1);
        let block = make_heap_block(Tag::Closure as u16);
        let boxed = boxed_cell(&block);
        assert!(unsafe { __torajs_in_op_any_str(boxed, b"call\0".as_ptr()) });
        // The closure routed to the Function family.
        assert_eq!(
            CHAIN_SEEN_FAMILY.with(|f| f.get()),
            crate::builtin_proto::FUNCTION_PROTO_TAG as i64
        );
    }

    #[test]
    fn str_both_faces_miss_returns_false() {
        set_faces(0, 0);
        let block = make_heap_block(Tag::DynObj as u16);
        let boxed = boxed_cell(&block);
        assert!(!unsafe { __torajs_in_op_any_str(boxed, b"foo\0".as_ptr()) });
    }

    #[test]
    fn str_no_family_receiver_skips_chain() {
        // A shape with no row in the family table has no builtin
        // prototype on its chain face — even a chain stub primed to
        // answer true must not be asked. `Tag::Response` is the
        // stand-in: a `fetch` result is an object, and the prototype
        // it should answer from is a recorded gap. (WeakRef used to
        // sit here and now HAS a row — `"deref" in new WeakRef({})`
        // is true.)
        set_faces(0, 1);
        let block = make_heap_block(Tag::Response as u16);
        let boxed = boxed_cell(&block);
        assert!(!unsafe { __torajs_in_op_any_str(boxed, b"foo\0".as_ptr()) });
        assert_eq!(CHAIN_SEEN_FAMILY.with(|f| f.get()), -1);
    }

    #[test]
    fn str_family_routing_covers_builtin_receivers() {
        // (heap tag, expected builtin-proto family tag) pairs.
        let cases: &[(u16, i64)] = &[
            (Tag::Obj as u16, 1),
            (Tag::DynObj as u16, 1),
            (TAG_ARR, 2),
            (Tag::Closure as u16, 13),
            (Tag::RegExp as u16, 7),
            (Tag::Date as u16, 8),
            (Tag::Promise as u16, 10),
            (Tag::Map as u16, 11),
            (Tag::Set as u16, 12),
            (Tag::NumberWrapper as u16, 0),
            (Tag::StringWrapper as u16, 3),
            (Tag::BooleanWrapper as u16, 4),
        ];
        for &(heap_tag, family) in cases {
            set_faces(0, 0);
            let block = make_heap_block(heap_tag);
            let boxed = boxed_cell(&block);
            assert!(!unsafe { __torajs_in_op_any_str(boxed, b"x\0".as_ptr()) });
            assert_eq!(
                CHAIN_SEEN_FAMILY.with(|f| f.get()),
                family,
                "heap tag {heap_tag}"
            );
        }
    }

    #[test]
    fn str_non_heap_tag_throws() {
        let boxed = nan_box(2, 0x1234_5678);
        assert_eq!(
            thrown_by(|| unsafe { __torajs_in_op_any_str(boxed, b"foo\0".as_ptr()) }),
            (false, true)
        );
    }

    #[test]
    fn str_null_ptr_throws() {
        let boxed = nan_box(ANY_TAG_HEAP, 0);
        assert_eq!(
            thrown_by(|| unsafe { __torajs_in_op_any_str(boxed, b"foo\0".as_ptr()) }),
            (false, true)
        );
    }

    #[test]
    fn str_heap_primitive_cell_throws() {
        // A string / symbol / bigint passes the ANY_HEAP tag gate but
        // is still a primitive, so the rejection has to key off
        // type_tag rather than the NaN-box tag alone.
        for tag in [TAG_STR, TAG_SYMBOL, TAG_BIGINT] {
            set_faces(1, 1);
            let block = make_heap_block(tag);
            let boxed = boxed_cell(&block);
            assert_eq!(
                thrown_by(|| unsafe { __torajs_in_op_any_str(boxed, b"foo\0".as_ptr()) }),
                (false, true),
                "type_tag {tag}"
            );
        }
    }

    #[test]
    fn str_object_rhs_does_not_throw() {
        // The object path must stay clean — a plain "absent property"
        // answer is false with no pending TypeError.
        set_faces(0, 0);
        let block = make_heap_block(Tag::DynObj as u16);
        let boxed = boxed_cell(&block);
        assert_eq!(
            thrown_by(|| unsafe { __torajs_in_op_any_str(boxed, b"foo\0".as_ptr()) }),
            (false, false)
        );
    }
}
