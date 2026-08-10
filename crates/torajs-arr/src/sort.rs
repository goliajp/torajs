//! Array `.sort(cmp)` O(n log n) runtime helper — Perf Round 5
//! attack #1 (RFC 20260703-perf-arr-sort-nlogn).
//!
//! Replaces the SSA-emitted inline insertion sort (O(n²) — 243,901
//! comparator calls for a 1000-element random array vs JSC's merge
//! sort at 8,686, measured 2026-07-03) with a stable merge sort +
//! insertion base driven from native code, calling the user
//! comparator back through the closure ABI.
//!
//! Comparator ABI: the SSA lowering passes the raw fn pointer, the
//! env pointer (closure receiver; NULL for plain FnSig values) and a
//! `mode` bitset selecting one of 8 concrete C signatures — element
//! and return registers differ between i64 (GPR) and f64 (FPR) on
//! AArch64, so the call must go through the exact matching fn type:
//!
//! - bit 0: element slots are f64 (else i64 / pointer bits)
//! - bit 1: comparator returns f64 (else i64)
//! - bit 2: env pointer present (Closure) vs absent (FnSig)
//! - bit 3: element slots are Str pointers — every compare runs the
//!   §23.1.3.30.2 steps 5-8 undefined pre-probe (sentinel sorts last
//!   WITHOUT calling the comparator). Str-only: an i64/f64 element's
//!   raw bits could collide with the sentinel address.
//!
//! Ordering predicate is `is_gt` = "comparator said strictly greater
//! than zero"; an f64 NaN return compares false which matches the ES
//! §23.1.3.30 ToNumber-NaN → +0 rule and the pre-existing inline
//! `FCmp Ogt` semantics. Merge takes the left element unless
//! `is_gt(left, right)`, which keeps the sort stable.
//!
//! Throw safety: the comparator may record a pending throw (TLS flag
//! via torajs-throw). After every callback we poll
//! `__torajs_throw_check`; on a pending throw the sort unwinds to a
//! *complete permutation* of the original elements (no slot lost or
//! duplicated — refcount balance is preserved) and returns early so
//! the caller's SSA `emit_throw_check` propagates the throw.

use core::ffi::c_void;

/// Offset of the `len` u64 within an array heap block.
const ARR_HDR_LEN_OFF: usize = 8;

/// Offset of the `head_offset` u32 within an array heap block
/// (T-13.5 deque packed cap + head).
const ARR_HDR_HEAD_OFF: usize = 20;

use crate::layout::arr_data;

unsafe extern "C" {
    /// Cross-tier — provided by torajs-throw at `tr build` link time.
    /// Non-zero iff a throw is pending in the TLS throw slot.
    fn __torajs_throw_check() -> i64;

    /// Cross-tier — torajs-str. §23.1.3.30.2 steps 5-8 pre-probe:
    /// `1`/`-1`/`0` when either side is the undefined sentinel
    /// (SortCompare answer, comparator skipped), `2` otherwise.
    fn __torajs_str_sort_undef_pre(a: *const u8, b: *const u8) -> i64;

    /// Cross-tier — torajs-anyvalue NaN-box protocol + coercions
    /// (the any-receiver sort modes, backfill chunk 4).
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_to_number(v: u64) -> f64;
    fn __torajs_anyv_to_str(v: u64) -> *mut c_void;
    /// Cross-tier — torajs-str / universal dropper (temp release).
    fn __torajs_str_drop(s: *mut c_void);
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// Cross-tier — torajs-str. Default SortCompare: undefined-last
    /// + UTF-16 ToString order.
    fn __torajs_str_sort_cmp(a: *const u8, b: *const u8) -> i64;

    /// torajs-mmalloc libc-compat malloc / free — merge scratch buffer.
    #[link_name = "__torajs_libc_malloc"]
    fn malloc(n: usize) -> *mut c_void;
    #[link_name = "__torajs_libc_free"]
    fn free(p: *mut c_void);
}

/// `mode` bit 0 — element slots carry f64 bits (comparator params in FPRs).
const MODE_ELEM_F64: i64 = 1;
/// `mode` bit 1 — comparator returns f64 (FPR return).
const MODE_RET_F64: i64 = 2;
/// `mode` bit 2 — comparator is a Closure (env pointer prepended).
const MODE_HAS_ENV: i64 = 4;
/// `mode` bit 3 — element slots are Str pointers (undefined
/// sentinel pre-probe runs before every comparator call).
const MODE_ELEM_STR: i64 = 8;

/// `Cmp.rebox_kind` = ARR_KIND_UNSET means the slots already ARE
/// AnyValues (FLAG_ARR_ANY receiver); any other kind reboxes the
/// raw slot bits per the recorded elem kind before comparing (typed
/// block behind an `any` view — the slots keep their raw layout,
/// only the compare sees boxes).
#[inline]
unsafe fn rebox_slot(kind: u16, raw: u64) -> u64 {
    unsafe {
        match kind {
            torajs_rc::ARR_KIND_I64 => __torajs_anyv_box_from_pair(2, raw as i64),
            torajs_rc::ARR_KIND_F64 => __torajs_anyv_box_from_pair(3, raw as i64),
            torajs_rc::ARR_KIND_BOOL => __torajs_anyv_box_from_pair(1, raw as i64),
            torajs_rc::ARR_KIND_HEAP if raw == 0 => __torajs_anyv_box_from_pair(5, 0),
            torajs_rc::ARR_KIND_HEAP => raw, // a heap cell's box IS its pointer
            _ => raw,
        }
    }
}

/// Runs at or below this length sort via binary-free insertion —
/// same base-case cutoff family as JSC / std sorts.
const INSERTION_RUN: usize = 32;

/// Comparator polymorphism seam — the sort kernels (insertion /
/// merge / sort_range) are generic over it and monomorphize per
/// implementation, so the typed-tier comparator keeps its
/// pre-ANY-modes codegen (bench regression 12115353: folding the ANY
/// arms into one `is_gt` grew the fn past the inline threshold and
/// taxed every typed compare).
trait SortCmp {
    /// Compare raw slot bits `a`, `b` — `true` iff a sorts after b.
    unsafe fn is_gt(&self, a: u64, b: u64) -> bool;
}

/// Comparator callback bundle. `is_gt(a, b)` dispatches to the exact
/// extern "C" signature selected by `mode` and reports whether the
/// user comparator returned > 0.
struct Cmp {
    fn_ptr: *const u8,
    env: *mut u8,
    mode: i64,
}

impl SortCmp for Cmp {
    /// Call the user comparator with raw slot bits `a`, `b`.
    /// Returns `true` iff the comparator result is strictly greater
    /// than zero (f64 NaN → false, i.e. treated as 0). Str elements
    /// run the §23.1.3.30.2 undefined pre-probe first — an undefined
    /// side sorts last and the comparator is never called.
    #[inline]
    unsafe fn is_gt(&self, a: u64, b: u64) -> bool {
        unsafe {
            if self.mode & MODE_ELEM_STR != 0 {
                let pre = __torajs_str_sort_undef_pre(a as *const u8, b as *const u8);
                if pre != 2 {
                    return pre > 0;
                }
            }
            let f = self.fn_ptr;
            let e = self.env;
            let elem_f64 = self.mode & MODE_ELEM_F64 != 0;
            let ret_f64 = self.mode & MODE_RET_F64 != 0;
            let has_env = self.mode & MODE_HAS_ENV != 0;
            // S1 (RFC 20260810-indirect-argc-abi) — the env-first
            // shapes carry the hidden i64 argc after the env; a
            // comparator receives two arguments. FnSig shapes (no
            // env) are named fns and carry no argc slot.
            match (has_env, elem_f64, ret_f64) {
                (true, false, false) => {
                    let c: unsafe extern "C" fn(*mut u8, i64, i64, i64) -> i64 =
                        core::mem::transmute(f);
                    c(e, 2, a as i64, b as i64) > 0
                }
                (true, false, true) => {
                    let c: unsafe extern "C" fn(*mut u8, i64, i64, i64) -> f64 =
                        core::mem::transmute(f);
                    c(e, 2, a as i64, b as i64) > 0.0
                }
                (true, true, false) => {
                    let c: unsafe extern "C" fn(*mut u8, i64, f64, f64) -> i64 =
                        core::mem::transmute(f);
                    c(e, 2, f64::from_bits(a), f64::from_bits(b)) > 0
                }
                (true, true, true) => {
                    let c: unsafe extern "C" fn(*mut u8, i64, f64, f64) -> f64 =
                        core::mem::transmute(f);
                    c(e, 2, f64::from_bits(a), f64::from_bits(b)) > 0.0
                }
                (false, false, false) => {
                    let c: unsafe extern "C" fn(i64, i64) -> i64 = core::mem::transmute(f);
                    c(a as i64, b as i64) > 0
                }
                (false, false, true) => {
                    let c: unsafe extern "C" fn(i64, i64) -> f64 = core::mem::transmute(f);
                    c(a as i64, b as i64) > 0.0
                }
                (false, true, false) => {
                    let c: unsafe extern "C" fn(f64, f64) -> i64 = core::mem::transmute(f);
                    c(f64::from_bits(a), f64::from_bits(b)) > 0
                }
                (false, true, true) => {
                    let c: unsafe extern "C" fn(f64, f64) -> f64 = core::mem::transmute(f);
                    c(f64::from_bits(a), f64::from_bits(b)) > 0.0
                }
            }
        }
    }
}

/// Any-receiver comparator (backfill chunk 4) — slots are NaN-box
/// AnyValues (or typed raw slots reboxed per `rebox_kind`); a user
/// comparator rides the boxed dual-entry ABI, its absence is the
/// §23.1.3.30.2 step-10 default (undefined-last + ToString UTF-16
/// order).
struct AnyCmp {
    fn_ptr: *const u8,
    env: *mut u8,
    /// `true` = step-10 default compare (`fn_ptr` unused).
    default_mode: bool,
    /// Comparator declares the receiver-first channel (RFC
    /// 20260717-objlit-anylane-recv knife 2e) — argv shifts one slot
    /// so `__this` binds `undefined` (§23.1.3.30 no-thisArg).
    recv_first: bool,
    /// Elem-kind rebox for typed-behind-any receivers;
    /// ARR_KIND_UNSET = slots already NaN-boxed.
    rebox_kind: u16,
}

impl SortCmp for AnyCmp {
    unsafe fn is_gt(&self, a: u64, b: u64) -> bool {
        unsafe {
            let a = rebox_slot(self.rebox_kind, a);
            let b = rebox_slot(self.rebox_kind, b);
            // §23.1.3.30.2 steps 5-8 — undefined sorts last, the
            // comparator never sees one; both-undefined is +0.
            let a_undef = __torajs_anyv_unbox_tag(a) == 5;
            let b_undef = __torajs_anyv_unbox_tag(b) == 5;
            if a_undef || b_undef {
                return a_undef && !b_undef;
            }
            if self.default_mode {
                // Step 10 — ToString both sides, UTF-16 order.
                let sa = __torajs_anyv_to_str(a);
                let sb = __torajs_anyv_to_str(b);
                let r = __torajs_str_sort_cmp(sa as *const u8, sb as *const u8);
                __torajs_str_drop(sa);
                __torajs_str_drop(sb);
                return r > 0;
            }
            // Boxed dual-entry comparator — argv is borrowed,
            // the owned return releases after ToNumber.
            let cb: unsafe extern "C" fn(*mut c_void, *const u64, i64) -> u64 =
                core::mem::transmute(self.fn_ptr);
            let argv = [__torajs_anyv_box_from_pair(5, 0), a, b];
            let (window, n) = if self.recv_first {
                (argv.as_ptr(), 3)
            } else {
                (argv[1..].as_ptr(), 2)
            };
            let r = cb(self.env as *mut c_void, window, n);
            let n = __torajs_anyv_to_number(r);
            __torajs_value_drop_heap(r as *mut c_void);
            n > 0.0
        }
    }
}

/// Pending-throw poll — comparator side effects land in the TLS
/// throw slot; the sort must stop permuting and hand control back.
#[inline]
unsafe fn aborted() -> bool {
    unsafe { __torajs_throw_check() != 0 }
}

/// Stable insertion sort over `n` slots at `s`. Returns `false` when
/// a comparator throw aborted the pass; the slots then hold a
/// complete permutation (the in-flight element is written back into
/// the current hole before returning).
unsafe fn insertion<C: SortCmp>(s: *mut u64, n: usize, cmp: &C) -> bool {
    unsafe {
        for i in 1..n {
            let cur = *s.add(i);
            let mut j = i;
            while j > 0 {
                let prev = *s.add(j - 1);
                let gt = cmp.is_gt(prev, cur);
                if aborted() {
                    // slots[j] is the hole (its value already shifted
                    // right or copied out as `cur`) — plug it.
                    *s.add(j) = cur;
                    return false;
                }
                if !gt {
                    break;
                }
                *s.add(j) = prev;
                j -= 1;
            }
            *s.add(j) = cur;
        }
        true
    }
}

/// Stable merge of `s[lo..mid]` and `s[mid..hi]` using `scratch`
/// (holds the left run, `mid - lo` slots). Returns `false` on
/// comparator throw; remaining scratch elements are flushed back so
/// `s[lo..hi]` stays a complete permutation.
unsafe fn merge<C: SortCmp>(
    s: *mut u64,
    scratch: *mut u64,
    lo: usize,
    mid: usize,
    hi: usize,
    cmp: &C,
) -> bool {
    unsafe {
        let ln = mid - lo;
        core::ptr::copy_nonoverlapping(s.add(lo), scratch, ln);
        let mut i = 0usize;
        let mut j = mid;
        let mut k = lo;
        while i < ln && j < hi {
            let l = *scratch.add(i);
            let r = *s.add(j);
            let gt = cmp.is_gt(l, r);
            if aborted() {
                // invariant: j - k == ln - i, so the unconsumed left
                // run exactly fills the gap before the right run.
                core::ptr::copy_nonoverlapping(scratch.add(i), s.add(k), ln - i);
                return false;
            }
            if gt {
                *s.add(k) = r;
                j += 1;
            } else {
                *s.add(k) = l;
                i += 1;
            }
            k += 1;
        }
        core::ptr::copy_nonoverlapping(scratch.add(i), s.add(k), ln - i);
        true
    }
}

/// Recursive top-down merge sort of `s[lo..hi]`; insertion base at
/// [`INSERTION_RUN`]. Depth is log2(n) (~10 frames for 1000 slots).
unsafe fn sort_range<C: SortCmp>(
    s: *mut u64,
    scratch: *mut u64,
    lo: usize,
    hi: usize,
    cmp: &C,
) -> bool {
    unsafe {
        let n = hi - lo;
        if n <= INSERTION_RUN {
            return insertion(s.add(lo), n, cmp);
        }
        let mid = lo + n / 2;
        if !sort_range(s, scratch, lo, mid, cmp) {
            return false;
        }
        if !sort_range(s, scratch, mid, hi, cmp) {
            return false;
        }
        merge(s, scratch, lo, mid, hi, cmp)
    }
}

/// Sort `arr` in place with a user comparator — stable O(n log n)
/// merge sort. `fn_ptr` + `env` + `mode` describe the comparator
/// callback (see module doc for the bit layout).
///
/// Pure slot permutation: element bits move between slots of the
/// same array, so refcounts are untouched. On a comparator throw the
/// array is left as a complete (partially sorted) permutation and
/// the pending throw propagates via the caller's `emit_throw_check`.
///
/// # Safety
/// `extern "C"` ABI. `arr` must be a live `Array<T>` heap block with
/// 8-byte slots (`Array<Any>`'s 16-byte tagged slots never reach
/// this helper — the SSA lowering gates on the element layout).
/// `fn_ptr` must be a live comparator matching `mode`'s signature.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_sort_cb(
    arr: *mut u8,
    fn_ptr: *const u8,
    env: *mut u8,
    mode: i64,
) {
    unsafe {
        let len = *(arr.add(ARR_HDR_LEN_OFF) as *const u64) as usize;
        if len < 2 {
            return;
        }
        let head = *(arr.add(ARR_HDR_HEAD_OFF) as *const u32) as usize;
        let slots = arr_data(arr).add(head * 8) as *mut u64;
        let cmp = Cmp { fn_ptr, env, mode };
        if len <= INSERTION_RUN {
            insertion(slots, len, &cmp);
            return;
        }
        // scratch holds the larger left run of the top split:
        // ceil(len / 2) slots covers every recursive merge.
        let scratch = malloc(len.div_ceil(2) * 8) as *mut u64;
        sort_range(slots, scratch, 0, len, &cmp);
        free(scratch as *mut c_void);
    }
}

/// `xs.sort(cmp?)` where the receiver arrived through `any`
/// (backfill chunk 4) — the same stable merge sort over 8-byte
/// slots, comparing through the ANY modes: a user comparator rides
/// the boxed dual-entry ABI (`has_cb != 0`), its absence is the
/// §23.1.3.30.2 step-10 default (undefined-last + ToString UTF-16
/// order). Pure slot permutation — refcounts are untouched; a
/// typed block behind the `any` view keeps its raw slot layout
/// (compares rebox per the recorded elem kind). Answers the same
/// pointer for chaining.
///
/// # Safety
/// `arr` is a valid `Tag::Arr` heap pointer; when `has_cb != 0`,
/// `(cb_env, cb_entry)` is a live closure cell + its non-zero boxed
/// dual entry.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_any_sort(
    arr: *mut u8,
    cb_env: *mut c_void,
    cb_entry: u64,
    has_cb: i64,
) -> *mut u8 {
    unsafe {
        // RFC 20260810 刀 D — the sort staging walk crosses the
        // unmaterialized tail; loud reject.
        if crate::sparse_gate::sparse_tail_rejects(
            arr as *const core::ffi::c_void,
            b"sparse array tail is not yet supported in Array.prototype.sort\0".as_ptr(),
        ) {
            return arr;
        }
        let len = *(arr.add(ARR_HDR_LEN_OFF) as *const u64) as usize;
        if len < 2 {
            return arr;
        }
        let header = &*(arr as *const torajs_rc::HeapHeader);
        let rebox_kind = if header.flags & torajs_rc::FLAG_ARR_ANY != 0 {
            torajs_rc::ARR_KIND_UNSET
        } else {
            header.arr_elem_kind()
        };
        let head = *(arr.add(ARR_HDR_HEAD_OFF) as *const u32) as usize;
        let slots = arr_data(arr).add(head * 8) as *mut u64;
        let cmp = AnyCmp {
            fn_ptr: cb_entry as usize as *const u8,
            env: cb_env as *mut u8,
            default_mode: has_cb == 0,
            recv_first: has_cb != 0
                && !cb_env.is_null()
                && crate::method_any_hof::recv_first_shift(cb_env as *mut c_void) != 0,
            rebox_kind,
        };
        if len <= INSERTION_RUN {
            insertion(slots, len, &cmp);
            return arr;
        }
        let scratch = malloc(len.div_ceil(2) * 8) as *mut u64;
        sort_range(slots, scratch, 0, len, &cmp);
        free(scratch as *mut c_void);
        arr
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::{AtomicI64, Ordering};

    /// Test-controlled stand-in for the torajs-throw TLS flag —
    /// cargo test binaries don't link libtorajs_throw.a. nextest
    /// runs each test in its own process, so no cross-test bleed.
    static THROW_FLAG: AtomicI64 = AtomicI64::new(0);

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn __torajs_throw_check() -> i64 {
        THROW_FLAG.load(Ordering::Relaxed)
    }

    // The ANY-mode extern stubs (anyv_* / str_sort_cmp / str_drop)
    // live in lib.rs's crate-level `#[cfg(test)]` stub block beside
    // `__torajs_value_drop_heap` — single home per symbol.

    /// Build a minimal Array<T> heap block: [header 8][len 8]
    /// [cap+head 8][props 8][slots]. Returns the backing Vec (keep
    /// alive) and the block pointer.
    fn make_arr(slots: &[u64], head: u32) -> (Vec<u8>, *mut u8) {
        let total = crate::layout::ARR_CELL_SIZE + (head as usize + slots.len()) * 8;
        let mut buf = vec![0u8; total];
        let p = buf.as_mut_ptr();
        unsafe {
            *(p.add(ARR_HDR_LEN_OFF) as *mut u64) = slots.len() as u64;
            *(p.add(16) as *mut u32) = (head as usize + slots.len()) as u32; // cap
            *(p.add(ARR_HDR_HEAD_OFF) as *mut u32) = head;
            *(p.add(crate::layout::ARR_DATA_PTR_OFF) as *mut *mut u8) =
                p.add(crate::layout::ARR_CELL_SIZE);
            let data = p.add(crate::layout::ARR_CELL_SIZE + head as usize * 8) as *mut u64;
            for (i, &v) in slots.iter().enumerate() {
                *data.add(i) = v;
            }
        }
        (buf, p)
    }

    fn read_slots(p: *mut u8, n: usize, head: u32) -> Vec<u64> {
        let mut out = Vec::with_capacity(n);
        unsafe {
            let data = arr_data(p).add(head as usize * 8) as *const u64;
            for i in 0..n {
                out.push(*data.add(i));
            }
        }
        out
    }

    unsafe extern "C" fn cmp_i64_asc(a: i64, b: i64) -> i64 {
        a - b
    }

    unsafe extern "C" fn cmp_f64_asc(a: f64, b: f64) -> f64 {
        a - b
    }

    // S1 (RFC 20260810-indirect-argc-abi) — env-first shape carries
    // the hidden argc between the env and the elements.
    unsafe extern "C" fn cmp_env_count(env: *mut u8, argc: i64, a: i64, b: i64) -> i64 {
        assert_eq!(argc, 2, "comparator receives two arguments");
        unsafe { *(env as *mut i64) += 1 };
        a - b
    }

    /// Stability probe: key in the high 32 bits, original sequence
    /// number in the low 32; comparator orders by key only.
    unsafe extern "C" fn cmp_key_only(a: i64, b: i64) -> i64 {
        (a >> 32) - (b >> 32)
    }

    /// Sets the fake throw flag after 5 calls, then keeps comparing.
    unsafe extern "C" fn cmp_throw_after_5(a: i64, b: i64) -> i64 {
        static CALLS: AtomicI64 = AtomicI64::new(0);
        if CALLS.fetch_add(1, Ordering::Relaxed) + 1 == 5 {
            THROW_FLAG.store(1, Ordering::Relaxed);
        }
        a - b
    }

    fn lcg_data(n: usize) -> Vec<u64> {
        let mut s: i64 = 1;
        (0..n)
            .map(|_| {
                s = ((s.wrapping_mul(48271)) as i32 & 0x7fffffff) as i64;
                if s == 0 {
                    s = 1;
                }
                s as u64
            })
            .collect()
    }

    #[test]
    fn sorts_random_i64_large() {
        let data = lcg_data(1000);
        let (_buf, p) = make_arr(&data, 0);
        unsafe { __torajs_arr_sort_cb(p, cmp_i64_asc as *const u8, core::ptr::null_mut(), 0) };
        let got = read_slots(p, 1000, 0);
        let mut want = data;
        want.sort();
        assert_eq!(got, want);
    }

    #[test]
    fn sorts_small_run_and_presorted() {
        for data in [vec![3u64, 1, 2], (0..32).collect(), (0..32).rev().collect()] {
            let (_buf, p) = make_arr(&data, 0);
            unsafe { __torajs_arr_sort_cb(p, cmp_i64_asc as *const u8, core::ptr::null_mut(), 0) };
            let got = read_slots(p, data.len(), 0);
            let mut want = data;
            want.sort();
            assert_eq!(got, want);
        }
    }

    #[test]
    fn sorts_with_deque_head_offset() {
        let data = lcg_data(100);
        let (_buf, p) = make_arr(&data, 7);
        unsafe { __torajs_arr_sort_cb(p, cmp_i64_asc as *const u8, core::ptr::null_mut(), 0) };
        let got = read_slots(p, 100, 7);
        let mut want = data;
        want.sort();
        assert_eq!(got, want);
    }

    #[test]
    fn sorts_f64_elems_f64_ret() {
        let data: Vec<u64> = [3.5f64, -1.25, 0.0, 99.0, 2.5, -7.75]
            .iter()
            .map(|f| f.to_bits())
            .collect();
        let (_buf, p) = make_arr(&data, 0);
        unsafe {
            __torajs_arr_sort_cb(
                p,
                cmp_f64_asc as *const u8,
                core::ptr::null_mut(),
                MODE_ELEM_F64 | MODE_RET_F64,
            )
        };
        let got: Vec<f64> = read_slots(p, data.len(), 0)
            .iter()
            .map(|&b| f64::from_bits(b))
            .collect();
        assert_eq!(got, vec![-7.75, -1.25, 0.0, 2.5, 3.5, 99.0]);
    }

    #[test]
    fn env_comparator_receives_env() {
        let data = lcg_data(50);
        let mut count: i64 = 0;
        let (_buf, p) = make_arr(&data, 0);
        unsafe {
            __torajs_arr_sort_cb(
                p,
                cmp_env_count as *const u8,
                &mut count as *mut i64 as *mut u8,
                MODE_HAS_ENV,
            )
        };
        let got = read_slots(p, 50, 0);
        let mut want = data;
        want.sort();
        assert_eq!(got, want);
        assert!(count > 0, "env comparator was never called");
    }

    #[test]
    fn merge_sort_is_stable() {
        // 200 entries, 10 distinct keys, sequence number in low bits.
        let data: Vec<u64> = (0..200u64).map(|i| ((i % 10) << 32) | i).collect();
        let mut shuffled = data.clone();
        // deterministic shuffle
        let mut s: u64 = 12345;
        for i in (1..shuffled.len()).rev() {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1);
            let j = (s >> 33) as usize % (i + 1);
            shuffled.swap(i, j);
        }
        let (_buf, p) = make_arr(&shuffled, 0);
        unsafe { __torajs_arr_sort_cb(p, cmp_key_only as *const u8, core::ptr::null_mut(), 0) };
        let got = read_slots(p, 200, 0);
        // stable: within each key group, sequence numbers must appear
        // in the same relative order as in `shuffled`.
        for key in 0..10u64 {
            let want_seq: Vec<u64> = shuffled
                .iter()
                .filter(|&&v| v >> 32 == key)
                .map(|&v| v & 0xffff_ffff)
                .collect();
            let got_seq: Vec<u64> = got
                .iter()
                .filter(|&&v| v >> 32 == key)
                .map(|&v| v & 0xffff_ffff)
                .collect();
            assert_eq!(
                got_seq, want_seq,
                "key {key} group reordered — sort unstable"
            );
        }
    }

    #[test]
    fn throw_abort_leaves_complete_permutation() {
        THROW_FLAG.store(0, Ordering::Relaxed);
        let data = lcg_data(500);
        let (_buf, p) = make_arr(&data, 0);
        unsafe {
            __torajs_arr_sort_cb(p, cmp_throw_after_5 as *const u8, core::ptr::null_mut(), 0)
        };
        assert_eq!(THROW_FLAG.load(Ordering::Relaxed), 1, "probe never threw");
        let mut got = read_slots(p, 500, 0);
        let mut want = data;
        got.sort();
        want.sort();
        assert_eq!(got, want, "abort lost or duplicated elements");
        THROW_FLAG.store(0, Ordering::Relaxed);
    }

    #[test]
    fn len_0_and_1_are_noops() {
        for data in [vec![], vec![42u64]] {
            let (_buf, p) = make_arr(&data, 0);
            unsafe { __torajs_arr_sort_cb(p, cmp_i64_asc as *const u8, core::ptr::null_mut(), 0) };
            assert_eq!(read_slots(p, data.len(), 0), data);
        }
    }
}
