//! The comparator seam of [`crate::sort`] — the `mode` bit layout the
//! lowerer's `__torajs_arr_sort_cb` call carries, the slot rebox for a
//! typed block behind an `any` view, and the two `SortCmp`
//! implementations the sort kernels monomorphize over: the typed
//! comparator (`Cmp`, one exact extern "C" signature per mode) and the
//! any-receiver comparator (`AnyCmp`, boxed dual-entry ABI or the
//! §23.1.3.30.2 step-10 default). The kernels themselves (insertion /
//! merge / sort_range) and the two entry points stay in `sort.rs`.
//! Pure displacement (rotation 649): `sort.rs` had reached the file
//! limit and the receiver-first comparator needed room.

use core::ffi::c_void;

unsafe extern "C" {
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
}

/// `mode` bit 0 — element slots carry f64 bits (comparator params in FPRs).
pub(crate) const MODE_ELEM_F64: i64 = 1;
/// `mode` bit 1 — comparator returns f64 (FPR return).
pub(crate) const MODE_RET_F64: i64 = 2;
/// `mode` bit 2 — comparator is a Closure (env pointer prepended).
pub(crate) const MODE_HAS_ENV: i64 = 4;
/// `mode` bit 3 — element slots are Str pointers (undefined
/// sentinel pre-probe runs before every comparator call).
pub(crate) const MODE_ELEM_STR: i64 = 8;
/// The comparator cell is stamped `FLAG_CLOSURE_RECV_FIRST`: its
/// native entry takes a boxed receiver ahead of `(a, b)`. Set at
/// entry off the cell header (once per sort), never read per compare
/// — the lowerer's `mode` never carries it.
pub(crate) const MODE_RECV_FIRST: i64 = 16;

/// `Cmp.rebox_kind` = ARR_KIND_UNSET means the slots already ARE
/// AnyValues (FLAG_ARR_ANY receiver); any other kind reboxes the
/// raw slot bits per the recorded elem kind before comparing (typed
/// block behind an `any` view — the slots keep their raw layout,
/// only the compare sees boxes).
#[inline]
pub(crate) unsafe fn rebox_slot(kind: u16, raw: u64) -> u64 {
    unsafe {
        match kind {
            torajs_rc::ARR_KIND_I64 => __torajs_anyv_box_from_pair(2, raw as i64),
            // A number[] slot holding the undefined sentinel is
            // `undefined` (§23.1.3.30.2: it sorts last, uncompared).
            torajs_rc::ARR_KIND_F64 if raw == torajs_rc::F64_UNDEF_SENTINEL_BITS => {
                __torajs_anyv_box_from_pair(5, 0)
            }
            torajs_rc::ARR_KIND_F64 => __torajs_anyv_box_from_pair(3, raw as i64),
            torajs_rc::ARR_KIND_BOOL => __torajs_anyv_box_from_pair(1, raw as i64),
            torajs_rc::ARR_KIND_HEAP if raw == 0 => __torajs_anyv_box_from_pair(5, 0),
            torajs_rc::ARR_KIND_HEAP => raw, // a heap cell's box IS its pointer
            _ => raw,
        }
    }
}

/// Comparator polymorphism seam — the sort kernels (insertion /
/// merge / sort_range) are generic over it and monomorphize per
/// implementation, so the typed-tier comparator keeps its
/// pre-ANY-modes codegen (bench regression 12115353: folding the ANY
/// arms into one `is_gt` grew the fn past the inline threshold and
/// taxed every typed compare).
pub(crate) trait SortCmp {
    /// Compare raw slot bits `a`, `b` — `true` iff a sorts after b.
    unsafe fn is_gt(&self, a: u64, b: u64) -> bool;
}

/// RFC 20260905 residue 1 — the receiver-first comparator (§23.1.3.30
/// calls the comparator with an undefined `this`): the same four
/// env-first shapes with the undefined box between the argc and the
/// pair. Out of line so the plain comparator's `is_gt` keeps its
/// inline budget (bench regression 12115353).
#[inline(never)]
unsafe fn is_gt_recv_first(
    f: *const u8,
    e: *mut u8,
    a: u64,
    b: u64,
    elem_f64: bool,
    ret_f64: bool,
) -> bool {
    unsafe {
        let this = __torajs_anyv_box_from_pair(5, 0) as i64;
        match (elem_f64, ret_f64) {
            (false, false) => {
                let c: unsafe extern "C" fn(*mut u8, i64, i64, i64, i64) -> i64 =
                    core::mem::transmute(f);
                c(e, 2, this, a as i64, b as i64) > 0
            }
            (false, true) => {
                let c: unsafe extern "C" fn(*mut u8, i64, i64, i64, i64) -> f64 =
                    core::mem::transmute(f);
                c(e, 2, this, a as i64, b as i64) > 0.0
            }
            (true, false) => {
                let c: unsafe extern "C" fn(*mut u8, i64, i64, f64, f64) -> i64 =
                    core::mem::transmute(f);
                c(e, 2, this, f64::from_bits(a), f64::from_bits(b)) > 0
            }
            (true, true) => {
                let c: unsafe extern "C" fn(*mut u8, i64, i64, f64, f64) -> f64 =
                    core::mem::transmute(f);
                c(e, 2, this, f64::from_bits(a), f64::from_bits(b)) > 0.0
            }
        }
    }
}

/// Comparator callback bundle. `is_gt(a, b)` dispatches to the exact
/// extern "C" signature selected by `mode` and reports whether the
/// user comparator returned > 0.
pub(crate) struct Cmp {
    pub(crate) fn_ptr: *const u8,
    pub(crate) env: *mut u8,
    pub(crate) mode: i64,
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
            if self.mode & MODE_RECV_FIRST != 0 {
                return is_gt_recv_first(f, e, a, b, elem_f64, ret_f64);
            }
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
pub(crate) struct AnyCmp {
    pub(crate) fn_ptr: *const u8,
    pub(crate) env: *mut u8,
    /// `true` = step-10 default compare (`fn_ptr` unused).
    pub(crate) default_mode: bool,
    /// Comparator declares the receiver-first channel (RFC
    /// 20260717-objlit-anylane-recv knife 2e) — argv shifts one slot
    /// so `__this` binds `undefined` (§23.1.3.30 no-thisArg).
    pub(crate) recv_first: bool,
    /// Elem-kind rebox for typed-behind-any receivers;
    /// ARR_KIND_UNSET = slots already NaN-boxed.
    pub(crate) rebox_kind: u16,
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
