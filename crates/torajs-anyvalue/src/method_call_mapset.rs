//! `Tag::Map` / `Tag::Set` arm of `__torajs_any_method_call`
//! (Any-method-call RFC 20260704 C4) — split out of `method_call.rs`
//! by the 500-line file discipline.
//!
//! - get / set / has / delete / add / clear (C4-1) go straight onto
//!   the torajs-collections pair-ABI kernels; heap keys / values are
//!   CONSUMED by the kernels so borrowed argv payloads rc-bump
//!   first. `set` / `add` return `this` (+1, boxed-value
//!   convention). Methods of the other collection kind (`get` on a
//!   Set, `add` on a Map) fall through to the catchable TypeError
//!   like bun.
//! - forEach (C4-2) walks the entries with the caller-managed-cursor
//!   kernel `__torajs_map_iter_next` and invokes the callback
//!   through its boxed dual entry (`(env, argv, argc) -> AnyValue`,
//!   the C3a adapters — arr `method_any_hof` is the precedent). Per
//!   ES §24.1.3.5 the Map callback receives `(value, key, map)`; per
//!   §24.2.3.7 the Set callback receives `(value, value, set)`. The
//!   cursor re-reads `n_used` every step, so entries the callback
//!   appends mid-walk are still visited and tombstoned ones are
//!   skipped (ES forEach live-iteration semantics).
//!
//! forEach ledger: the iter kernel hands out BORROWED entry pairs —
//! the loop rc-bumps them into an owned frame before the call
//! (the callback can `delete`/`clear` the entry out from under its
//! own argv) and releases after; the callback's return is +1-owned
//! and released (forEach discards it). A pending throw after any
//! callback aborts the walk and returns `undefined` for the
//! SSA-side throw check to propagate.

use core::ffi::c_void;

use torajs_rc::{
    ANY_METHOD_ADD, ANY_METHOD_CLEAR, ANY_METHOD_DELETE, ANY_METHOD_DIFFERENCE, ANY_METHOD_ENTRIES,
    ANY_METHOD_FOR_EACH, ANY_METHOD_GET, ANY_METHOD_GET_OR_INSERT,
    ANY_METHOD_GET_OR_INSERT_COMPUTED, ANY_METHOD_GET_SIZE, ANY_METHOD_HAS,
    ANY_METHOD_INTERSECTION, ANY_METHOD_IS_DISJOINT_FROM, ANY_METHOD_IS_SUBSET_OF,
    ANY_METHOD_IS_SUPERSET_OF, ANY_METHOD_KEYS, ANY_METHOD_NEXT, ANY_METHOD_SET,
    ANY_METHOD_SYMMETRIC_DIFFERENCE, ANY_METHOD_UNION, ANY_METHOD_VALUES, Tag,
};

use crate::method_call::{MAX_BOXED_ARGS, closure_boxed_entry, method_no_such, not_callable};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED};
use crate::nanbox_encode::__torajs_anyv_box_from_pair;

unsafe extern "C" {
    /// torajs-collections — Map/Set kernels (pair ABI; heap keys /
    /// values are consumed — the caller rc-bumps before the call).
    fn __torajs_map_get_or_insert(
        p: *mut c_void,
        key_tag: i64,
        key_payload: i64,
        default_tag: i64,
        default_payload: i64,
        out_tag: *mut i64,
        out_payload: *mut i64,
    );
    fn __torajs_map_get(
        p: *const c_void,
        key_tag: i64,
        key_payload: i64,
        out_tag: *mut i64,
        out_payload: *mut i64,
    );
    fn __torajs_map_set(
        p: *mut c_void,
        key_tag: i64,
        key_payload: i64,
        value_tag: i64,
        value_payload: i64,
    );
    fn __torajs_map_has(p: *const c_void, key_tag: i64, key_payload: i64) -> i64;
    fn __torajs_map_delete(p: *mut c_void, key_tag: i64, key_payload: i64) -> i64;
    fn __torajs_map_clear(p: *mut c_void);
    fn __torajs_map_size(p: *const c_void) -> i64;
    /// torajs-collections — ES2025 set methods (§24.2.5). The four
    /// combiners answer a fresh rc=1 Set (ownership transfers out);
    /// the three predicates answer 0/1.
    fn __torajs_set_union(this: *const c_void, other: *const c_void) -> *mut c_void;
    fn __torajs_set_intersection(this: *const c_void, other: *const c_void) -> *mut c_void;
    fn __torajs_set_difference(this: *const c_void, other: *const c_void) -> *mut c_void;
    fn __torajs_set_symmetric_difference(this: *const c_void, other: *const c_void) -> *mut c_void;
    fn __torajs_set_is_subset_of(this: *const c_void, other: *const c_void) -> i64;
    fn __torajs_set_is_superset_of(this: *const c_void, other: *const c_void) -> i64;
    fn __torajs_set_is_disjoint_from(this: *const c_void, other: *const c_void) -> i64;
    /// Cross-tier — torajs-throw (records a pending TypeError).
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// torajs-collections — caller-managed-cursor entry walk
    /// (`*cursor = -1` = first call; out pairs are borrows).
    fn __torajs_map_iter_next(
        p: *const c_void,
        cursor: *mut i64,
        out_k_tag: *mut i64,
        out_k_payload: *mut i64,
        out_v_tag: *mut i64,
        out_v_payload: *mut i64,
    ) -> i64;
    /// torajs-collections — iterator-object mints (each rc-incs the
    /// source Map/Set and answers a fresh rc=1 MapIter cell) + the
    /// cursor advance (out pair is a borrow; the ENTRIES pair array
    /// comes back pre-decremented to 0 so the caller's owning inc
    /// lands it at exactly 1).
    fn __torajs_map_iter_create_keys(p: *mut c_void) -> *mut c_void;
    fn __torajs_map_iter_create_values(p: *mut c_void) -> *mut c_void;
    fn __torajs_map_iter_create_entries(p: *mut c_void) -> *mut c_void;
    fn __torajs_map_iter_create_set_entries(p: *mut c_void) -> *mut c_void;
    fn __torajs_map_iter_step(p: *mut c_void, out_tag: *mut i64, out_payload: *mut i64) -> i64;
    fn __torajs_arr_iter_step(p: *mut c_void, out_tag: *mut i64, out_payload: *mut i64) -> i64;
    /// torajs-dynobj — the IteratorResult `{ value, done }` shell.
    /// `set` rc-incs the key (caller keeps its own ref) and CONSUMES
    /// the heap value; the obj slot rides by reference (resize may
    /// relocate).
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_dynobj_set(obj_slot: *mut *mut c_void, key: *mut c_void, tag: u64, value: u64);
    /// torajs-str — key literals for the IteratorResult shell.
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
    /// torajs-rc — NaN-box-safe refcount bump (the consume-contract
    /// pre-inc for borrowed argv payloads + the `set`/`add` return
    /// of `this`).
    fn __torajs_rc_inc(p: *mut c_void);
    /// Cross-tier — universal NaN-box-safe heap dropper.
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// Cross-tier — torajs-throw. Non-zero iff a throw is pending.
    fn __torajs_throw_check() -> i64;
}

/// The boxed dual-entry ABI (torajs-core `ssa_lower_boxed_entry`).
type BoxedFn = unsafe extern "C" fn(*mut c_void, *const u64, i64) -> u64;

/// `Tag::Map` / `Tag::Set` arm — id-switch onto the
/// torajs-collections kernels (see module doc).
pub(crate) unsafe fn map_set_method(
    m: *mut c_void,
    is_set: bool,
    mid: i64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    let arg_at = |i: i64| -> u64 {
        if i < argc {
            unsafe { *argv.add(i as usize) }
        } else {
            VALUE_UNDEFINED
        }
    };
    // Decode a borrowed AnyValue into the kernels' (tag, payload)
    // pair, handing the kernel exactly one owned stake per the
    // consume contract: `unbox_value_owned` rc-bumps a heap cell
    // (the caller's box keeps its own ref) and hands a ShortStr
    // through as a freshly-materialized rc=1 Str — already the one
    // stake. Pre-fix the manual `unbox_value` + inc double-staked
    // the ShortStr materialization (unbox materializes owned, the
    // inc made it rc=2, the kernel consumed 1) and leaked one
    // 32-byte Str cell per get/set/add/has/delete carrying a short
    // string literal.
    let pair_consumed = |av: u64| -> (i64, i64) {
        let tag = crate::nanbox_encode::__torajs_anyv_unbox_tag(av);
        let payload = crate::nanbox_encode::__torajs_anyv_unbox_value_owned(av);
        (tag, payload)
    };
    unsafe {
        match mid {
            m2 if m2 == ANY_METHOD_GET && !is_set => {
                let (kt, kp) = pair_consumed(arg_at(0));
                let (mut vt, mut vp): (i64, i64) = (5, 0);
                __torajs_map_get(m, kt, kp, &mut vt, &mut vp);
                // The kernel rc-bumped a heap value for us — the box
                // transfers that ownership out.
                __torajs_anyv_box_from_pair(vt, vp)
            }
            m2 if m2 == ANY_METHOD_GET_OR_INSERT && !is_set => {
                // Stage-3 upsert (RFC 20260721 刀 6) — the kernel
                // owns both stakes and rc-bumps the answered value.
                let (kt, kp) = pair_consumed(arg_at(0));
                let (dt, dp) = pair_consumed(arg_at(1));
                let (mut vt, mut vp): (i64, i64) = (5, 0);
                __torajs_map_get_or_insert(m, kt, kp, dt, dp, &mut vt, &mut vp);
                __torajs_anyv_box_from_pair(vt, vp)
            }
            m2 if m2 == ANY_METHOD_GET_OR_INSERT_COMPUTED && !is_set => {
                // 383-04 — the callback-carrying upsert; the key
                // stake transfers to the core, the callback box is
                // borrowed argv, the answer comes back +1-owned.
                let (kt, kp) = pair_consumed(arg_at(0));
                crate::method_call_upsert::map_get_or_insert_computed(m, kt, kp, arg_at(1))
            }
            m2 if m2 == ANY_METHOD_SET && !is_set => {
                let (kt, kp) = pair_consumed(arg_at(0));
                let (vt, vp) = pair_consumed(arg_at(1));
                __torajs_map_set(m, kt, kp, vt, vp);
                // ES §24.1.3.9 — returns `this`.
                __torajs_rc_inc(m);
                m as u64
            }
            m2 if m2 == ANY_METHOD_ADD && is_set => {
                let (kt, kp) = pair_consumed(arg_at(0));
                __torajs_map_set(m, kt, kp, 5, 0);
                // ES §24.2.3.1 — returns `this`.
                __torajs_rc_inc(m);
                m as u64
            }
            m2 if m2 == ANY_METHOD_HAS => {
                let (kt, kp) = pair_consumed(arg_at(0));
                __torajs_anyv_box_from_pair(1, __torajs_map_has(m, kt, kp))
            }
            m2 if m2 == ANY_METHOD_DELETE => {
                let (kt, kp) = pair_consumed(arg_at(0));
                __torajs_anyv_box_from_pair(1, __torajs_map_delete(m, kt, kp))
            }
            m2 if m2 == ANY_METHOD_CLEAR => {
                __torajs_map_clear(m);
                VALUE_UNDEFINED
            }
            m2 if m2 == ANY_METHOD_GET_SIZE => {
                // The reified `get size` getter invoked through
                // `.call(recv)` — the id is only reachable via the
                // carried-mid re-dispatch (it never interns).
                __torajs_anyv_box_from_pair(2, __torajs_map_size(m))
            }
            m2 if m2 == ANY_METHOD_FOR_EACH => {
                let Some((cb_env, cb_entry)) = closure_boxed_entry(arg_at(0)) else {
                    return not_callable();
                };
                map_set_for_each(m, is_set, cb_env, cb_entry, arg_at(1))
            }
            m2 if is_set
                && matches!(
                    m2,
                    ANY_METHOD_UNION
                        | ANY_METHOD_INTERSECTION
                        | ANY_METHOD_DIFFERENCE
                        | ANY_METHOD_SYMMETRIC_DIFFERENCE
                        | ANY_METHOD_IS_SUBSET_OF
                        | ANY_METHOD_IS_SUPERSET_OF
                        | ANY_METHOD_IS_DISJOINT_FROM
                ) =>
            {
                // §24.2.1.2 GetSetRecord — a real-Set argument keeps
                // the Set×Set fast kernels; anything else (a Map, a
                // user set-like, a refusing primitive) walks the
                // observable size/has/keys protocol in `set_like`.
                let other_av = arg_at(0);
                let other = {
                    // Borrow-shaped cell read — a ShortStr / immediate
                    // answers NULL and routes to the protocol (whose
                    // step-1 object gate throws for it).
                    let p = crate::nanbox_encode::__torajs_anyv_cell_ptr(other_av) as *mut c_void;
                    if p.is_null()
                        || (p.cast::<u8>().add(4) as *const u16).read() != Tag::Set as u16
                    {
                        let out = crate::set_like_ops::setlike_method(m, m2, other_av);
                        if __torajs_throw_check() != 0 {
                            return VALUE_UNDEFINED;
                        }
                        return out;
                    }
                    p as *const c_void
                };
                match m2 {
                    ANY_METHOD_UNION => __torajs_set_union(m, other) as u64,
                    ANY_METHOD_INTERSECTION => __torajs_set_intersection(m, other) as u64,
                    ANY_METHOD_DIFFERENCE => __torajs_set_difference(m, other) as u64,
                    ANY_METHOD_SYMMETRIC_DIFFERENCE => {
                        __torajs_set_symmetric_difference(m, other) as u64
                    }
                    ANY_METHOD_IS_SUBSET_OF => {
                        __torajs_anyv_box_from_pair(1, __torajs_set_is_subset_of(m, other))
                    }
                    ANY_METHOD_IS_SUPERSET_OF => {
                        __torajs_anyv_box_from_pair(1, __torajs_set_is_superset_of(m, other))
                    }
                    _ => __torajs_anyv_box_from_pair(1, __torajs_set_is_disjoint_from(m, other)),
                }
            }
            m2 if m2 == ANY_METHOD_KEYS || m2 == ANY_METHOD_VALUES || m2 == ANY_METHOD_ENTRIES => {
                // Iterator mints — fresh rc=1 MapIter cell transfers
                // out as the box. Set keys/values both yield elements
                // (§24.2.3.8 `keys` is the `values` alias); Set
                // entries yields `[e, e]` pairs (§24.2.3.6).
                let it = if m2 == ANY_METHOD_ENTRIES {
                    if is_set {
                        __torajs_map_iter_create_set_entries(m)
                    } else {
                        __torajs_map_iter_create_entries(m)
                    }
                } else if is_set || m2 == ANY_METHOD_KEYS {
                    __torajs_map_iter_create_keys(m)
                } else {
                    __torajs_map_iter_create_values(m)
                };
                it as u64
            }
            _ => method_no_such(),
        }
    }
}

/// `Tag::MapIter` arm — the iterator-protocol surface for the mints
/// above. `next()` answers a fresh IteratorResult `{ value, done }`
/// dynobj (ES §27.1.3): the step kernel's borrowed payload rc-bumps
/// into an owned ref (the ENTRIES pair array arrives pre-decremented
/// to 0 exactly so this inc lands it at 1) and transfers into the
/// dynobj value slot; exhaustion answers `{ value: undefined,
/// done: true }` forever after.
pub(crate) unsafe fn map_iter_method(it: *mut c_void, mid: i64) -> AnyValue {
    if mid != ANY_METHOD_NEXT {
        return unsafe { method_no_such() };
    }
    unsafe {
        let (mut tag, mut payload): (i64, i64) = (5, 0);
        let hit = __torajs_map_iter_step(it, &mut tag, &mut payload);
        if hit != 0 {
            crate::payload_rc_inc(tag, payload);
        }
        let mut obj = __torajs_dynobj_alloc();
        let k_value = __torajs_str_alloc(c"value".as_ptr() as *const u8, 5);
        __torajs_dynobj_set(&mut obj, k_value as *mut c_void, tag as u64, payload as u64);
        __torajs_str_drop(k_value as *mut c_void);
        let k_done = __torajs_str_alloc(c"done".as_ptr() as *const u8, 4);
        __torajs_dynobj_set(&mut obj, k_done as *mut c_void, 1, (hit == 0) as u64);
        __torajs_str_drop(k_done as *mut c_void);
        obj as u64
    }
}

/// `Tag::ArrIter` arm — mirror of `map_iter_method` for
/// `Array.prototype.{keys,values,entries}` iterators reached through
/// the any-lane dispatch. Same `{ value, done }` IteratorResult shape;
/// `__torajs_arr_iter_step` returns borrowed payloads that need
/// `payload_rc_inc` before entering the dynobj slot (ENTRIES pair
/// arrives pre-decremented to land at 1 after the inc — same contract
/// as MapIter's ENTRIES).
pub(crate) unsafe fn arr_iter_method(it: *mut c_void, mid: i64) -> AnyValue {
    if mid != ANY_METHOD_NEXT {
        return unsafe { method_no_such() };
    }
    unsafe {
        let (mut tag, mut payload): (i64, i64) = (5, 0);
        let hit = __torajs_arr_iter_step(it, &mut tag, &mut payload);
        if hit != 0 {
            crate::payload_rc_inc(tag, payload);
        }
        let mut obj = __torajs_dynobj_alloc();
        let k_value = __torajs_str_alloc(c"value".as_ptr() as *const u8, 5);
        __torajs_dynobj_set(&mut obj, k_value as *mut c_void, tag as u64, payload as u64);
        __torajs_str_drop(k_value as *mut c_void);
        let k_done = __torajs_str_alloc(c"done".as_ptr() as *const u8, 4);
        __torajs_dynobj_set(&mut obj, k_done as *mut c_void, 1, (hit == 0) as u64);
        __torajs_str_drop(k_done as *mut c_void);
        obj as u64
    }
}

/// forEach walk (see module doc for the ledger).
unsafe fn map_set_for_each(
    m: *mut c_void,
    is_set: bool,
    cb_env: *mut c_void,
    cb_entry: u64,
    this_arg: u64,
) -> AnyValue {
    unsafe {
        let cb: BoxedFn = core::mem::transmute(cb_entry as usize);
        // Recv-first callback (RFC 20260717-objlit-anylane-recv
        // knife 2e) — shift `(v, k, m)` up one slot so `__this`
        // reads the buffer's `undefined` (§24.1.3.5 / §24.2.3.7
        // no-thisArg forEach binds `this = undefined`).
        let s = crate::method_call::recv_first_shift(cb_env);
        // The receiver rides as the callback's third argument — a
        // heap cell's NaN-box encoding is its pointer bits (borrow).
        let m_boxed = m as u64;
        let mut cursor: i64 = -1;
        let (mut kt, mut kp, mut vt, mut vp): (i64, i64, i64, i64) = (5, 0, 5, 0);
        while __torajs_map_iter_next(m, &mut cursor, &mut kt, &mut kp, &mut vt, &mut vp) != 0 {
            // Own the entry pair across the callback — it can
            // delete/clear the entry out from under its own argv.
            crate::payload_rc_inc(kt, kp);
            let k_boxed = __torajs_anyv_box_from_pair(kt, kp);
            let v_boxed = if is_set {
                // ES §24.2.3.7 — the Set callback's first two args
                // are both the element; the entry value slot (always
                // undefined) is unused. One owned ref covers both
                // argv slots.
                k_boxed
            } else {
                crate::payload_rc_inc(vt, vp);
                __torajs_anyv_box_from_pair(vt, vp)
            };
            let mut argv = [VALUE_UNDEFINED; MAX_BOXED_ARGS];
            if s == 1 {
                // knife 4 — the thisArg (or undefined) rides argv[0]
                // for a receiver-first callback (§24.1.3.5 step 5).
                argv[0] = this_arg;
            }
            argv[s] = v_boxed;
            argv[s + 1] = k_boxed;
            argv[s + 2] = m_boxed;
            let r = cb(cb_env, argv.as_ptr(), (3 + s) as i64);
            let threw = __torajs_throw_check() != 0;
            // forEach discards the +1-owned return; release the
            // owned frame pair (one ref for the Set alias).
            __torajs_value_drop_heap(r as *mut c_void);
            __torajs_value_drop_heap(k_boxed as *mut c_void);
            if !is_set {
                __torajs_value_drop_heap(v_boxed as *mut c_void);
            }
            if threw {
                return VALUE_UNDEFINED;
            }
        }
        VALUE_UNDEFINED
    }
}
