//! The struct arm's expando entries, a three-lane walk: a
//! runtime-computed instance field rides its DECLARATION row (torajs-meta `classmeta::computed_fields`
//! keeps the anchor; `__torajs_struct_expando_order` answers which
//! dict entries go before row `i`, after the last row, and the rest),
//! so `{"foo":1,"b":42,"bar":2}` comes out in definition order like
//! `Object.keys` does.
//!
//! Each entry goes through the dynobj walk's `composites::write_entry`
//! with the struct cell as the holder, so a getter `defineProperty` put
//! in the dict runs with the instance as its receiver.
//!
//! §25.5.2.4 step 6 fixes K before step 8 reads a value, so an entry a
//! getter, a toJSON hook or the replacer adds mid-walk stays out:
//! [`KeysInK`] pins the dict's keys when the walk starts, and each lane
//! emits only the entries whose key it holds.

use core::ffi::c_void;

use super::*;

unsafe extern "C" {
    /// torajs-meta — the lane's dict iteration indices.
    fn __torajs_struct_expando_order(
        cell: *const c_void,
        lane: i64,
        out: *mut u64,
        cap: u64,
    ) -> u64;
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_dynobj_set_fresh(obj: *mut *mut c_void, key: *mut c_void, tag: u64, value: u64);
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_drop(obj: *mut c_void);
}

/// The expando keys K holds: every key of the struct cell's dict when
/// the walk starts. The first lane call takes them; every walk makes
/// that call before it reads a value, and taking them there keeps the
/// work out of the struct walk itself.
///
/// They are held as the keys of a dict of their own, in the same
/// order, each one's value its position plus one: the dict retains
/// each key, so a mid-walk delete cannot free it under the comparison,
/// and finds a key by its name in one probe.
pub(super) struct KeysInK {
    /// Whether the first lane call took K.
    taken: bool,
    /// K's dict; null while untaken, or when the struct had none.
    k: *mut c_void,
}

impl KeysInK {
    /// K before the walk has looked at the dict.
    pub(super) const fn new() -> Self {
        KeysInK {
            taken: false,
            k: core::ptr::null_mut(),
        }
    }

    /// Take K from `props`, the struct's dict when the walk first
    /// looks (null when it has none then); later calls keep it.
    /// Answers whether K holds any entry.
    ///
    /// # Safety
    /// `props` is null or a live dict.
    unsafe fn take(&mut self, props: *mut c_void) -> bool {
        if !self.taken {
            self.taken = true;
            let n = unsafe { __torajs_dynobj_iter_len(props) };
            if n != 0 {
                self.k = unsafe { __torajs_dynobj_alloc() };
            }
            let mut held = 0u64;
            for i in 0..n {
                let key = unsafe { __torajs_dynobj_iter_key(props, i) };
                if !key.is_null() {
                    held += 1;
                    let at = (held as f64).to_bits();
                    unsafe { __torajs_dynobj_set_fresh(&mut self.k, key, ANY_F64, at) };
                }
            }
        }
        !self.k.is_null()
    }

    /// The key of K spelling `key`, the dict's entry `i` — the same
    /// cell, or one spelling the same name (a key deleted and set again
    /// mid-walk is still in K, §25.5.2.4 step 8 reads it then). K's
    /// cell is the one to quote: a replacer that deletes the entry
    /// frees the dict's own key cell. While no delete has made a hole
    /// before entry `i`, K holds the same cell at the same index.
    ///
    /// # Safety
    /// [`Self::take`] answered true; `key` is a live key cell.
    unsafe fn pinned(&self, i: u64, key: *mut c_void) -> Option<*mut c_void> {
        unsafe {
            if __torajs_dynobj_iter_key(self.k, i) == key {
                return Some(key);
            }
            let at = f64::from_bits(__torajs_dynobj_get_value(self.k, key)) as u64;
            (at != 0).then(|| __torajs_dynobj_iter_key(self.k, at - 1))
        }
    }
}

/// The dict value tag of a number (torajs-dynobj `ANY_F64`).
const ANY_F64: u64 = 3;

impl Drop for KeysInK {
    fn drop(&mut self) {
        // a taken K is null when the struct had no dict, which the drop
        // takes as nothing to release
        if self.taken {
            unsafe { __torajs_dynobj_drop(self.k) };
        }
    }
}

/// `lane` of [`write_struct_expandos`] — the entries before row `i`.
pub(super) fn before_row(i: u32) -> i64 {
    i64::from(i)
}
/// `lane` — the entries after the last row.
pub(super) const TAIL: i64 = -2;
/// `lane` — every other expando, insertion order.
pub(super) const REST: i64 = -1;

/// Serialize the enumerable expando entries `lane` selects whose key
/// `k` holds, as `"key":value` members after whatever `emitted` says
/// came before.
/// A pending throw (a hook's, a replacer's) ends the walk; the caller
/// checks it.
///
/// # Safety
/// `ptr` is a live `Tag::Obj` cell; `sb` a live string builder.
pub(super) unsafe fn write_struct_expandos(
    sb: *mut c_void,
    ptr: *mut c_void,
    depth: u32,
    st: &St,
    emitted: &mut bool,
    (lane, k): (i64, &mut KeysInK),
) {
    unsafe {
        let props = crate::member_get_layout::struct_props(ptr)
            .cast_mut()
            .cast::<c_void>();
        if !k.take(props) || __torajs_throw_check() != 0 {
            return;
        }
        let n_props = __torajs_dynobj_iter_len(props);
        let mut order = vec![0u64; n_props as usize];
        let n_ord = __torajs_struct_expando_order(ptr, lane, order.as_mut_ptr(), n_props);
        for &pi in order.iter().take(n_ord as usize) {
            if __torajs_dynobj_iter_flags(props, pi) & BUCKET_FLAG_ENUMERABLE == 0 {
                continue;
            }
            let key = __torajs_dynobj_iter_key(props, pi);
            if key.is_null() {
                continue;
            }
            let Some(key) = k.pinned(pi, key) else {
                continue;
            };
            // §25.5.2.2 step 3 — the holder is the STRUCT cell, not
            // its expando props dynobj: `this` inside a getter and the
            // replacer is the object the program sees.
            *emitted |=
                super::composites::write_entry(sb, (props, pi, key), ptr, depth, st, *emitted);
            if __torajs_throw_check() != 0 {
                return;
            }
        }
    }
}
