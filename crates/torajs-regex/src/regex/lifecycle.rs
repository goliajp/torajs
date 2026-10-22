//! RegExp lifecycle externs — drop / get_source / lastIndex
//! get-set. Port of `runtime_regex.c` L1519-1552, L2130-2140.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::ffi::c_void;

use super::{
    __torajs_rc_dec, __torajs_str_alloc_pooled, RegExp, STR_HDR_SIZE, as_regex, as_regex_mut,
    str_from_bytes,
};

unsafe extern "C" {
    /// torajs-cycle — cycle-root buffer push / scrub (rationale in
    /// `torajs-cycle::buffer`). The push is gated on
    /// `has_walkable_children`, so a bagless cell pays a tag test.
    fn __torajs_cycle_buffer(p: *mut c_void);
    fn __torajs_cycle_unbuffer(p: *mut c_void);
    /// torajs-meta — scrub a dying exotic-subclass instance's
    /// identity entry (RFC 20260730 blade 0); gated on
    /// `FLAG_SUBCLASSED` so plain regexes never call out.
    fn __torajs_subclass_drop_entry(p: *mut c_void);
}

/// `torajs_rc::FLAG_SUBCLASSED` mirror (flags bit 0, RFC 20260730
/// blade 0) — exotic cell minted as a user-class instance.
const FLAG_SUBCLASSED: u16 = 1;

/// # Safety
///
/// `re_ptr` must be a pointer previously returned by
/// `__torajs_regex_compile`, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_drop(re_ptr: *mut c_void) {
    if re_ptr.is_null() {
        return;
    }
    // Refcount decrement — returns 0 when the last ref dropped (per
    // torajs-rc contract; matches the C port's `if
    // (!__torajs_rc_dec(re_ptr)) return;`).
    if unsafe { __torajs_rc_dec(re_ptr) } == 0 {
        // Still referenced. A live own-property bag makes this cell a
        // potential cycle root — the shape rotation 528 taught the
        // collector to walk, and the reason it can now be reached.
        unsafe { __torajs_cycle_buffer(re_ptr) };
        return;
    }
    // Last ref — release a boxed-form lastIndex's heap stake (any-
    // lane stores keep the assigned value verbatim, one owned rc for
    // a cell), then reclaim the Box and let Rust recursively Drop
    // the Program (Vec<Inst> + Vec<CharClass> + Vec<Box<Program>>),
    // src_bytes (Vec<u8>), capture_names (Vec<Vec<u8>>).
    unsafe {
        let boxed = as_regex(re_ptr).last_index_boxed;
        if boxed != 0 {
            super::__torajs_value_drop_heap(boxed as *mut c_void);
        }
        if as_regex(re_ptr).header.flags & FLAG_SUBCLASSED != 0 {
            __torajs_subclass_drop_entry(re_ptr);
        }
        // Own-property bag (§22.2.6 ordinary-object face) — the
        // universal dispatcher routes it to the dynobj drop.
        let props = as_regex(re_ptr).props;
        if !props.is_null() {
            (*(re_ptr as *mut RegExp)).props = core::ptr::null_mut();
            super::__torajs_value_drop_heap(props);
        }
        // Scrub from the root buffer before the memory goes away: a
        // cell buffered above that later normal-drops to zero would
        // leave a dangling candidate. No-op when never buffered.
        __torajs_cycle_unbuffer(re_ptr);
        let _ = Box::from_raw(re_ptr as *mut RegExp);
    }
}

/// `re.source` — returns the original pattern bytes as a fresh
/// pooled `Str`. NULL receiver returns `""`. Port of
/// `__torajs_regex_get_source`.
///
/// # Safety
///
/// `re_ptr` is null or a live `*RegExp`. Returned pointer is a
/// pool-Str with rc=1; caller takes ownership.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_get_source(re_ptr: *const c_void) -> *mut c_void {
    if re_ptr.is_null() {
        return unsafe { __torajs_str_alloc_pooled(0) as *mut c_void };
    }
    let re = unsafe { as_regex(re_ptr) };
    // §22.2.6.10 step 4 — an EMPTY pattern reads as `(?:)`, so that
    // the source always round-trips through the `/…/` literal form
    // (`/(?:)/` is how an empty regex is spelled; `//` is a comment).
    // Only reachable since `RegExp("")` got a path: a literal cannot
    // produce an empty pattern.
    if re.src_bytes.is_empty() {
        return unsafe { str_from_bytes(b"(?:)") as *mut c_void };
    }
    // `src_bytes` is the code-unit form (str_helpers::str_units); the
    // allocator re-encodes it into the Str layout — a raw byte copy
    // into the Latin-1 layout read every non-ASCII source as one
    // character per byte.
    unsafe { str_from_bytes(&re.src_bytes) as *mut c_void }
}

/// `re.lastIndex` numeric getter — the typed-tier lane
/// (`re.lastIndex` under the static `number` type). Boxed form
/// (a non-numeric value stored by the any lane) coerces through
/// ToNumber, matching what the typed reader can represent.
///
/// # Safety
///
/// `re_ptr` is null or a live `*RegExp`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_get_last_index(re_ptr: *const c_void) -> f64 {
    if re_ptr.is_null() {
        return 0.0;
    }
    let re = unsafe { as_regex(re_ptr) };
    if re.last_index_boxed != 0 {
        return unsafe { super::__torajs_regex_last_index_boxed_num(re.last_index_boxed) };
    }
    re.last_index
}

/// `re.lastIndex = idx` numeric setter (typed-tier lane) — resets a
/// boxed-form value back to numeric form (dropping its heap stake).
///
/// # Safety
///
/// `re_ptr` is null or a live `*RegExp`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_set_last_index(re_ptr: *mut c_void, idx: f64) {
    if re_ptr.is_null() {
        return;
    }
    if unsafe { refuse_frozen(re_ptr) } {
        return;
    }
    unsafe { as_regex_mut(re_ptr) }.set_last_index_num(idx);
}

/// A `lastIndex` frozen by `defineProperty` refuses every further
/// assignment — §10.1.9.1 step 3.b answers false, and a strict-mode
/// assignment turns that into a TypeError. Only the EXTERNAL faces
/// ask: exec / test / the sticky advance write through
/// `set_last_index_num` directly, because §22.2.7.2's stores are
/// internal-slot writes, not property assignments.
///
/// # Safety
/// `re_ptr` is a live `*RegExp`.
unsafe fn refuse_frozen(re_ptr: *mut c_void) -> bool {
    if !last_index_locked(unsafe { as_regex(re_ptr) }) {
        return false;
    }
    unsafe {
        super::__torajs_throw_type_error(
            c"Attempted to assign to readonly property.".as_ptr() as *const u8
        )
    };
    true
}

/// `torajs_rc::FLAG_FROZEN` mirror (bit 4, universal header).
const FLAG_FROZEN: u16 = 1 << 4;

/// Whether `lastIndex` is non-writable: a `defineProperty` lock, or
/// `Object.freeze` on the RegExp (§7.3.14 clears the bit on every own
/// property, and `lastIndex` is the one the cell keeps itself).
/// Before this a frozen RegExp's `lastIndex` was still writable, by
/// assignment and by a tracked exec.
pub(crate) fn last_index_locked(re: &RegExp) -> bool {
    re.last_index_frozen != 0 || re.header.flags & FLAG_FROZEN != 0
}

/// §22.2.7.2 step 12.a / 15 — a global or sticky exec writes
/// `lastIndex` through `Set(R, "lastIndex", …, true)`, which a
/// non-writable one refuses with the strict TypeError. Asked before
/// the match runs (nothing observable happens in between); answers
/// `true` with the throw recorded when the caller must stop.
pub(crate) unsafe fn tracked_store_refuses(re: &RegExp) -> bool {
    if !last_index_locked(re) {
        return false;
    }
    unsafe {
        super::__torajs_throw_type_error(
            c"Attempted to assign to readonly property.".as_ptr() as *const u8
        )
    };
    true
}

/// §22.2.6.11 step 10 for a global replace: `Set(rx, "lastIndex", 0)`.
/// Answers false with the TypeError recorded when lastIndex is not
/// writable. The walk that follows ends on a failing exec, which stores
/// 0 again, so this is also the observable post-state.
///
/// # Safety
/// Single-threaded runtime; `re` is a live RegExp.
pub(crate) unsafe fn global_replace_start(re: &mut RegExp) -> bool {
    if unsafe { tracked_store_refuses(re) } {
        return false;
    }
    re.set_last_index_num(0.0);
    true
}

/// Boxed-form `lastIndex` peek (RFC 20260722 any-slot 刀) — the raw
/// NaN-box bits a non-numeric any-lane store left, or 0 for numeric
/// form (the caller then reads the f64 getter and boxes it). BORROW
/// semantics: no rc transfer — the any-lane reader incs per the
/// boxed-value convention.
///
/// # Safety
///
/// `re_ptr` is null or a live `*RegExp`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_last_index_raw(re_ptr: *const c_void) -> u64 {
    if re_ptr.is_null() {
        return 0;
    }
    unsafe { as_regex(re_ptr) }.last_index_boxed
}

/// Boxed-form `lastIndex` store (any-lane non-numeric assignment,
/// §22.2.4.1 — the value stores verbatim; ToLength happens at the
/// exec-entry consumption). TRANSFER semantics: the caller minted /
/// inc'd one rc for a heap cell; a previously boxed value releases.
///
/// # Safety
///
/// `re_ptr` is null or a live `*RegExp`; `v` is a valid NaN-box
/// AnyValue whose heap stake (if any) transfers to the cell.
#[inline(never)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_last_index_store_boxed(re_ptr: *mut c_void, v: u64) {
    if re_ptr.is_null() {
        return;
    }
    if unsafe { refuse_frozen(re_ptr) } {
        return;
    }
    let re = unsafe { as_regex_mut(re_ptr) };
    if re.last_index_boxed != 0 {
        unsafe { super::__torajs_value_drop_heap(re.last_index_boxed as *mut c_void) };
    }
    re.last_index_boxed = v;
}
/// `re.flags` — returns the spec-ordered flag string ("g" / "im" /
/// "dgimsuy" / etc.) per ES §22.2.6.4. Order is fixed: d, g, i, m,
/// s, u, v, y (hasIndices first). NULL receiver returns an empty
/// string.
///
/// # Safety
///
/// `re_ptr` is null or a live `*RegExp`. Returned pointer is a
/// pool-Str with rc=1; caller takes ownership.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_get_flags(re_ptr: *const c_void) -> *mut c_void {
    if re_ptr.is_null() {
        return unsafe { __torajs_str_alloc_pooled(0) as *mut c_void };
    }
    let f = unsafe { as_regex(re_ptr) }.flags;
    // Build the canonical 8-byte buffer at most, ordered
    // d, g, i, m, s, u, v, y (ES §22.2.6.4 flag order).
    // Mirrors `flags::parse_flags` source of truth.
    let mut buf = [0u8; 8];
    let mut n = 0usize;
    let bits: [(u8, u8); 8] = [
        (crate::parser::RE_FLAG_D, b'd'),
        (crate::parser::RE_FLAG_G, b'g'),
        (crate::parser::RE_FLAG_I, b'i'),
        (crate::parser::RE_FLAG_M, b'm'),
        (crate::parser::RE_FLAG_S, b's'),
        (crate::parser::RE_FLAG_U, b'u'),
        (crate::parser::RE_FLAG_V, b'v'),
        (crate::parser::RE_FLAG_Y, b'y'),
    ];
    for (bit, ch) in bits {
        if f & bit != 0 {
            buf[n] = ch;
            n += 1;
        }
    }
    let s = unsafe { __torajs_str_alloc_pooled(n as u64) };
    if n > 0 {
        unsafe {
            core::ptr::copy_nonoverlapping(buf.as_ptr(), s.add(STR_HDR_SIZE), n);
        }
    }
    s as *mut c_void
}

/// `re.toString()` — per ES §22.2.6.13 returns `/` + source + `/` +
/// flags. Spec-ordered flags (d, g, i, m, s, u, v, y) match
/// `__torajs_regex_get_flags`. NULL receiver returns `/(?:)/`
/// (matches V8/JSC fallback).
///
/// # Safety
///
/// `re_ptr` is null or a live `*RegExp`. Returned pointer is a
/// pool-Str with rc=1; caller takes ownership.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_to_string(re_ptr: *const c_void) -> *mut c_void {
    if re_ptr.is_null() {
        // Spec doesn't define this; mirror engines' fallback shape.
        let fallback: &[u8] = b"/(?:)/";
        let s = unsafe { __torajs_str_alloc_pooled(fallback.len() as u64) };
        unsafe {
            core::ptr::copy_nonoverlapping(fallback.as_ptr(), s.add(STR_HDR_SIZE), fallback.len());
        }
        return s as *mut c_void;
    }
    let re = unsafe { as_regex(re_ptr) };
    // Build the canonical flag bytes (matches __torajs_regex_get_flags).
    let mut flag_buf = [0u8; 8];
    let mut nflag = 0usize;
    let bits: [(u8, u8); 8] = [
        (crate::parser::RE_FLAG_D, b'd'),
        (crate::parser::RE_FLAG_G, b'g'),
        (crate::parser::RE_FLAG_I, b'i'),
        (crate::parser::RE_FLAG_M, b'm'),
        (crate::parser::RE_FLAG_S, b's'),
        (crate::parser::RE_FLAG_U, b'u'),
        (crate::parser::RE_FLAG_V, b'v'),
        (crate::parser::RE_FLAG_Y, b'y'),
    ];
    for (bit, ch) in bits {
        if re.flags & bit != 0 {
            flag_buf[nflag] = ch;
            nflag += 1;
        }
    }
    let mut buf: Vec<u8> = Vec::with_capacity(2 + re.src_bytes.len() + nflag);
    buf.push(b'/');
    buf.extend_from_slice(&re.src_bytes);
    buf.push(b'/');
    buf.extend_from_slice(&flag_buf[..nflag]);
    let s = unsafe { str_from_bytes(&buf) };
    s as *mut c_void
}

/// `re.global` / `.ignoreCase` / `.multiline` / `.dotAll` / `.unicode`
/// / `.sticky` — boolean flag getters per ES §22.2.6.5-10. Returns 1
/// when the corresponding bit is set in `re.flags`, 0 otherwise. NULL
/// receiver returns 0 for every flag (no live RegExp = no flags).
/// `flag_bit` is the `RE_FLAG_*` byte constant the caller emits at
/// compile time so the ssa_lower side doesn't have to re-derive the
/// bit-to-name map.
///
/// # Safety
///
/// `re_ptr` is null or a live `*RegExp`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_has_flag(re_ptr: *const c_void, flag_bit: i64) -> bool {
    if re_ptr.is_null() {
        return false;
    }
    (unsafe { as_regex(re_ptr) }.flags & (flag_bit as u8)) != 0
}
