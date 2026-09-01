//! ArrIter — stateful iterator returned by `arr.keys() / .values() /
//! .entries()` for `Array<Any>` sources.
//!
//! Historically this lived in `runtime_map.c` (the C-side was added
//! when MapIter graduated to its own substrate and ArrIter
//! piggy-backed off the same file). P4.1 closed without lifting it;
//! P4.3-g (2026-05-24) finally puts it in the right crate.
//!
//! Same shape as `torajs-collections::iter::MapIter` — distinct
//! `TAG_ARR_ITER = 17` so `value_drop_heap` routes correctly.
//!
//! Currently restricted to `Array<Any>` (16B slot stride). Typed
//! `Array<T>` for non-Any `T` needs an elem-tag field + per-tag step
//! path (P5.4 follow-up). Source array layout (mirror of
//! `runtime_str.c`):
//! ```text
//! offset 0  : universal heap header (8B)
//! offset 8  : len (u64)
//! offset 16 : cap (u32) + head_offset (u32)
//! offset 24 : slots[cap] — 16B each (tag u64 + payload u64)
//! ```

use core::ffi::c_void;

use torajs_rc::HeapHeader;

/// `type_tag` for ArrIter heap blocks (matches `torajs_rc::Tag::ArrIter`
/// = 17).
pub const TAG_ARR_ITER: u16 = 17;

/// Iteration kind.
pub const ARR_ITER_KEYS: u32 = 0;
pub const ARR_ITER_VALUES: u32 = 1;
pub const ARR_ITER_ENTRIES: u32 = 2;

/// ANY-slot tags (mirror of `torajs_rc::AnySlotTag`; duplicated here
/// because torajs-arr currently doesn't import all of them and an
/// extra crate-wide use would over-couple just for iter's needs).
const ANY_I64: u8 = 2;
const ANY_HEAP: u8 = 4;
const ANY_UNDEF: u8 = 5;

/// Which spec mint produced this cell, and so which prototype it
/// answers. §22.1.5.1 CreateStringIterator and §23.1.5.1
/// CreateArrayIterator give their objects DIFFERENT [[Prototype]]s,
/// and tr materializes a string iteration as an ArrIter over a
/// character array — so the source cannot tell them apart and the
/// cell has to say so itself. Lives in the word that used to be
/// padding, which keeps the block 32 bytes.
pub const ARR_ITER_FAMILY_ARRAY: u32 = 0;
pub const ARR_ITER_FAMILY_STRING: u32 = 1;

/// ArrIter heap block — 40 bytes, ABI-shared with the C-side
/// definition we just deleted.
#[repr(C)]
struct ArrIter {
    header: HeapHeader,
    arr: *mut c_void,
    cursor: i64,
    kind: u32,
    family: u32,
    /// Lazy own-property bag — §23.1.5.1 mints an ORDINARY object,
    /// so `it.zz = 1` is an ordinary own property and the cursor /
    /// kind / family above are internal state, not properties. NULL
    /// until the first such write; same shape Promise / Map / Date /
    /// the wrappers carry (see [`ARR_ITER_PROPS_OFF`]).
    props: *mut c_void,
}

/// Byte offset of `ArrIter::props` — mirrored by torajs-anyvalue
/// (`member_get_layout::ITER_PROPS_OFF`) and torajs-meta, the same
/// narrow-ABI constant replication the tag constants use.
pub const ARR_ITER_PROPS_OFF: usize = 32;

unsafe extern "C" {
    /// torajs-mmalloc libc-compat — v0.7-A2 step 6b cutover.
    #[link_name = "__torajs_libc_malloc"]
    fn malloc(n: usize) -> *mut c_void;
    #[link_name = "__torajs_libc_free"]
    fn free(p: *mut c_void);
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_rc_dec(p: *mut c_void) -> i32;
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// torajs-cycle — cycle-root buffer push / scrub (rationale in
    /// `torajs-cycle::buffer`). The push is gated on
    /// `has_walkable_children`, so a bagless cell pays a tag test.
    fn __torajs_cycle_buffer(p: *mut c_void);
    fn __torajs_cycle_unbuffer(p: *mut c_void);
    /// Cross-tier — same crate, but the IR emission uses an `extern
    /// "C"` call so we keep the boundary explicit for consistency
    /// with the rest of the iter externs.
    fn __torajs_arr_alloc_any(cap: u64) -> *mut c_void;
    fn __torajs_arr_push_any(arr: *mut c_void, tag: u64, value: u64) -> *mut c_void;
    /// Step 7e-A NaN-box decoders — read the per-slot AnyValue
    /// back into the legacy (tag, value) pair shape this fn was
    /// originally designed against.
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    /// torajs-buffer §23.2.4.4 — the view's length right now, or -1
    /// with a pending throw. Re-asked every step; see
    /// [`typedarray_source_len`].
    fn __torajs_typedarray_validate(av: u64) -> i64;
    /// torajs-buffer §10.4.5 — the element, OWNED (a BigInt kind
    /// mints a fresh cell).
    fn __torajs_typedarray_index_get(av: u64, index: f64) -> u64;
}

/// `type_tag` for TypedArray heap blocks (mirror of
/// `torajs_rc::Tag::TypedArray` = 28).
const TAG_TYPEDARRAY: u16 = 28;

/// §23.2.5.1 — `%TypedArray%.prototype.values` returns the SAME
/// Array Iterator arrays get, and CreateArrayIterator's own closure
/// branches on whether the source has a `[[TypedArrayName]]`. This
/// is that branch: the length comes from ValidateTypedArray rather
/// than the array header, and it is asked FRESH on every step —
/// which is what makes detaching mid-iteration a throw here and
/// nothing at all over there.
///
/// `None` = the validate threw; the caller stops and leaves the
/// pending throw for its own caller.
///
/// # Safety
/// `arr` is a live heap cell.
unsafe fn typedarray_source_len(arr: *mut c_void) -> Option<u64> {
    let len = unsafe { __torajs_typedarray_validate(arr as u64) };
    if len < 0 { None } else { Some(len as u64) }
}

/// True when the iterator's source is an integer-indexed exotic
/// view rather than an `Array<Any>`.
///
/// # Safety
/// `arr` is a live heap cell.
unsafe fn source_is_typedarray(arr: *mut c_void) -> bool {
    unsafe { (arr.cast::<u8>().add(4) as *const u16).read() == TAG_TYPEDARRAY }
}

/// Internal: alloc + init a fresh ArrIter struct. rc_inc the source
/// array so iteration stays valid past caller-side binding drop.
unsafe fn create_with_kind(arr_p: *mut c_void, kind: u32) -> *mut c_void {
    let it = unsafe { malloc(core::mem::size_of::<ArrIter>()) } as *mut ArrIter;
    unsafe {
        (*it).header = HeapHeader {
            refcount: 1,
            type_tag: TAG_ARR_ITER,
            flags: 0,
        };
        (*it).arr = arr_p;
        (*it).cursor = 0;
        (*it).kind = kind;
        (*it).family = ARR_ITER_FAMILY_ARRAY;
        // No own property written yet — the bag is minted by the
        // first `it.zz = 1`, never here.
        (*it).props = core::ptr::null_mut();
        if !arr_p.is_null() {
            __torajs_rc_inc(arr_p);
        }
    }
    it as *mut c_void
}

/// `__torajs_arr_iter_create_keys(arr)` — KEYS-kind iterator.
///
/// # Safety
/// `arr_p` is null or a live Array<Any> heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_iter_create_keys(arr_p: *mut c_void) -> *mut c_void {
    unsafe { create_with_kind(arr_p, ARR_ITER_KEYS) }
}

/// `__torajs_arr_iter_create_values(arr)` — VALUES-kind iterator.
///
/// # Safety
/// Same as keys.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_iter_create_values(arr_p: *mut c_void) -> *mut c_void {
    unsafe { create_with_kind(arr_p, ARR_ITER_VALUES) }
}

/// `__torajs_arr_iter_create_entries(arr)` — ENTRIES `[index, value]`
/// iterator.
///
/// # Safety
/// Same as keys.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_iter_create_entries(arr_p: *mut c_void) -> *mut c_void {
    unsafe { create_with_kind(arr_p, ARR_ITER_ENTRIES) }
}

/// `__torajs_arr_iter_create_values_string(arr)` — the §22.1.5.1
/// mint. The same VALUES cell over the same character array; it
/// differs only in naming %StringIteratorPrototype% when asked what
/// it inherits from, which is what makes the badge "[object String
/// Iterator]".
///
/// # Safety
/// Same as keys.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_iter_create_values_string(arr_p: *mut c_void) -> *mut c_void {
    let it = unsafe { create_with_kind(arr_p, ARR_ITER_VALUES) };
    unsafe { (*(it as *mut ArrIter)).family = ARR_ITER_FAMILY_STRING };
    it
}

/// `__torajs_arr_iter_family(iter)` — [`ARR_ITER_FAMILY_ARRAY`] or
/// [`ARR_ITER_FAMILY_STRING`], for the consumers that have to name
/// the cell's prototype (`getPrototypeOf`, the `@@toStringTag` walk).
///
/// # Safety
/// `iter_p` is a live ArrIter heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_iter_family(iter_p: *const c_void) -> u32 {
    unsafe { (*(iter_p as *const ArrIter)).family }
}

/// `__torajs_arr_iter_step(iter, *out_tag, *out_payload)` — advance
/// the cursor + fill out-params per kind. Returns 1 on hit, 0 when
/// cursor has run past `arr.length`.
///
/// The `.value` box the caller builds (`__torajs_anyv_box_from_pair`)
/// TRANSFERS the reference it is handed — its heap arm is a bare
/// `box_void_ptr` with no rc traffic. So each kind hands out exactly
/// what that box may own: ENTRIES mints a fresh `[index, value]`
/// Array<Any> at refcount 1, KEYS yields a primitive.
///
/// VALUES `+1`s the slot payload it forwards, because the slot read
/// above is an explicit borrow and the box would otherwise adopt the
/// array's own reference.
///
/// # Safety
/// `iter_p` is null or a live ArrIter. `out_*` are valid writable
/// pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_iter_step(
    iter_p: *mut c_void,
    out_tag: *mut i64,
    out_payload: *mut i64,
) -> i64 {
    if iter_p.is_null() {
        unsafe {
            *out_tag = ANY_UNDEF as i64;
            *out_payload = 0;
        }
        return 0;
    }
    let it = iter_p as *mut ArrIter;
    let arr = unsafe { (*it).arr };
    if arr.is_null() {
        unsafe {
            *out_tag = ANY_UNDEF as i64;
            *out_payload = 0;
        }
        return 0;
    }
    let typed = unsafe { source_is_typedarray(arr) };
    let len = if typed {
        match unsafe { typedarray_source_len(arr) } {
            Some(n) => n,
            None => {
                unsafe {
                    *out_tag = ANY_UNDEF as i64;
                    *out_payload = 0;
                }
                return 0;
            }
        }
    } else {
        unsafe { *((arr as *const u8).add(8) as *const u64) }
    };
    let i = unsafe { (*it).cursor } as u32;
    if i as u64 >= len {
        // §23.1.5.2.1 — exhaustion latches: [[IteratedObject]] is set
        // to undefined, so a later `push` never revives the iterator.
        // Dropping the strong ref here doubles as the latch; the
        // arr.is_null() arm above answers every subsequent step.
        unsafe {
            (*it).arr = core::ptr::null_mut();
            __torajs_value_drop_heap(arr);
            *out_tag = ANY_UNDEF as i64;
            *out_payload = 0;
        }
        return 0;
    }
    // Kind-aware borrowed whole-box read (backfill chunk 4) — a
    // typed block behind an `any` view reboxes per its recorded elem
    // kind; FLAG_ARR_ANY blocks keep the raw NaN-box slot read.
    // The typed read is OWNED where the array read is borrowed, so
    // the +1 the VALUES / ENTRIES arms below apply would be one too
    // many — `owned_read` tracks which it was.
    //
    // KEYS yields the index alone and reads no element at all
    // (§23.2.5.1's `key` kind does not either): a typed read MINTS
    // (a BigInt element is a fresh cell), and even the array borrow
    // feeds the unconditional unbox below, which materializes a
    // ShortStr slot into an owned Str no arm would consume
    // (rotation 546 — one leaked Str per `keys()` step).
    let kind = unsafe { (*it).kind };
    let read = kind != ARR_ITER_KEYS;
    let (slot_av, owned_read) = if !read {
        (0u64, false)
    } else if typed {
        (
            unsafe { __torajs_typedarray_index_get(arr as u64, i as f64) },
            true,
        )
    } else {
        (
            unsafe { crate::any::__torajs_arr_get_any_boxed(arr, i as u64) },
            false,
        )
    };
    // Nothing was read on the skipped path, so nothing is decoded
    // either — `slot_av` there is a placeholder no arm looks at.
    let (slot_tag, slot_val) = if read {
        unsafe {
            (
                __torajs_anyv_unbox_tag(slot_av) as u64,
                __torajs_anyv_unbox_value(slot_av) as u64,
            )
        }
    } else {
        (ANY_UNDEF as u64, 0)
    };
    unsafe { (*it).cursor = (i + 1) as i64 };

    let (tag, payload) = match unsafe { (*it).kind } {
        k if k == ARR_ITER_KEYS => (ANY_I64 as i64, i as i64),
        k if k == ARR_ITER_VALUES => {
            // The `.value` box adopts what it is handed, and
            // `__torajs_arr_get_any_boxed` above is an explicit borrow
            // (the slot keeps its reference), so the +1 has to happen
            // here. Without it `arr.values().next().value` handed out
            // the element's only stake and the element died under the
            // array — `arr[0][0]` answered undefined after five steps
            // (rotation 323). The inc gates on cell-likeness, not the
            // tag: a ShortStr reports Heap but `slot_val` is then the
            // materialization, whose fresh rc=1 stake transfers as-is
            // (rotation 546 — inc'ing it double-staked and leaked).
            if !owned_read
                && (slot_tag & 0xff) == ANY_HEAP as u64
                && torajs_rc::ffi::nan_box_is_cell_like(slot_av as *mut c_void)
            {
                unsafe { __torajs_rc_inc(slot_val as *mut c_void) };
            }
            (slot_tag as i64, slot_val as i64)
        }
        k if k == ARR_ITER_ENTRIES => {
            // Yield `[index, value]` Array<Any> at refcount 1 — that
            // one reference is what the caller transfers into the
            // `.value` box. Mirrors MapIter's `make_pair_arr`, which
            // carried (and has lost) the same pre-dec-to-0 idiom.
            unsafe {
                let mut out_arr = __torajs_arr_alloc_any(2);
                // Index — primitive i64, no rc_inc.
                out_arr = __torajs_arr_push_any(out_arr, ANY_I64 as u64, i as u64);
                // Value — a borrowed CELL payload needs rc_inc before
                // the push adopts it; a ShortStr materialization's
                // fresh stake transfers as-is (same gate as VALUES).
                if !owned_read
                    && (slot_tag & 0xff) == ANY_HEAP as u64
                    && torajs_rc::ffi::nan_box_is_cell_like(slot_av as *mut c_void)
                {
                    __torajs_rc_inc(slot_val as *mut c_void);
                }
                out_arr = __torajs_arr_push_any(out_arr, slot_tag, slot_val);
                (ANY_HEAP as i64, out_arr as i64)
            }
        }
        _ => (ANY_UNDEF as i64, 0),
    };

    unsafe {
        *out_tag = tag;
        *out_payload = payload;
    }
    1
}

/// `__torajs_arr_iter_drop(iter)` — rc-aware drop. Releases strong
/// ref on source array + frees iter struct.
///
/// # Safety
/// `iter_p` is null or a live ArrIter.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_iter_drop(iter_p: *mut c_void) {
    if iter_p.is_null() {
        return;
    }
    if unsafe { __torajs_rc_dec(iter_p) } == 0 {
        // Still referenced. A live own-property bag makes this cell a
        // potential cycle root — the shape rotation 528 taught the
        // collector to walk, and the reason it can now be reached.
        unsafe { __torajs_cycle_buffer(iter_p) };
        return;
    }
    let it = iter_p as *mut ArrIter;
    unsafe {
        let arr = (*it).arr;
        if !arr.is_null() {
            __torajs_value_drop_heap(arr);
        }
        // Own-property bag — the universal dispatcher routes it to
        // the dynobj drop.
        let props = (*it).props;
        if !props.is_null() {
            (*it).props = core::ptr::null_mut();
            __torajs_value_drop_heap(props);
        }
        // Scrub from the root buffer before the memory goes away: a
        // cell buffered above that later normal-drops to zero would
        // leave a dangling candidate. No-op when never buffered.
        __torajs_cycle_unbuffer(iter_p);
        free(iter_p);
    }
}
