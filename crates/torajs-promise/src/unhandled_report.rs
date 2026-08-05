//! Unhandled-rejection stderr rendering — split out of
//! `unhandled.rs` when the §20.5.3.2 `name` resolution pushed it past
//! the 500-line limit (registered as a watch at 496 in rotation 307;
//! the split shape was written down there too).
//!
//! The list bookkeeping stays behind in the parent; what moves is the
//! part that turns one rejected promise into one line of stderr:
//! [`fire_unhandled_reporter`] and the two writers it owns. The
//! `torajs_throw::uncaught` twin renders the same `name: message`
//! shape for a throw nobody caught.
//!
//! `reason_is_cell_like` deliberately did NOT come along — it has
//! callers elsewhere in the crate.

use core::sync::atomic::Ordering;

use super::unhandled::{
    FLAG_ERROR, OBJ_MESSAGE_OFF, STR_LEN_OFF, TAG_OBJ, TAG_STR, UNHANDLED_REJECTION_OCCURRED,
    reason_is_cell_like,
};
use crate::layout::{HeapHeader, Promise};

unsafe extern "C" {
    fn __torajs_syscall_write(fd: i32, buf: *const u8, n: usize) -> isize;
    /// Newline-terminated / newline-free Str writers (torajs-str).
    fn __torajs_str_print_err(s: *const u8);
    fn __torajs_str_write_err(s: *const u8);
    /// torajs-anyvalue — an error instance's `name` resolved through
    /// its own slot and then the class prototype chain (§20.5.3.2);
    /// the `torajs_throw::uncaught` twin declares the same symbol.
    fn __torajs_error_name_get(obj: *const u8) -> *const u8;
    /// torajs-str — own-absence sentinel identity probe.
    fn __torajs_str_is_undef(p: *const u8) -> i64;
}

/// Default unhandled-rejection reporter. Writes one line to stderr
/// and sets the process-global flag.
///
/// The line is `<label>: <detail>`, where the label is the literal
/// `error` for every reason except an Error instance — there the
/// Error's own `name` IS the label. That is bun's shape: it reports
/// `Promise.reject("hi")` as `error: hi` but
/// `Promise.reject(new TypeError("mine"))` as `TypeError: mine`,
/// never stacking one label on the other.
///
/// Reason rendering:
///   - heap Str (real heap pointer, type_tag == 0) → reuse
///     `__torajs_str_print_err`, prefixed.
///   - Error-derived instance (type_tag == Obj + FLAG_ERROR) →
///     `name: message\n` read from the Error layout prefix, with
///     `: message` omitted when the message is empty. Verbatim
///     twin of `torajs_throw::__torajs_uncaught_exit_code`'s
///     rendering, so tr's two error-report paths agree.
///   - other real heap (Closure / RegExp / Date / Symbol / plain
///     object / etc.) → `error: <object>\n` placeholder.
///   - NaN-box immediate (a Type::Any reason routed through the
///     `_heap` alloc path) → `error: <any>\n` placeholder.
///   - primitive (`value_is_heap == 0`) non-zero → `error: <value>\n`
///     with the i64 written as decimal via the local int formatter.
///   - primitive zero → `error: null\n` (bun-parity for the
///     0-arg `Promise.reject()` sentinel).
///
/// v0.5 narrow MVP renderer; A4 lands user-side `process.on(
/// 'unhandledRejection', cb)` which gets the raw reason in
/// NaN-box form and lets user code stringify with full
/// anyvalue dispatch.
pub(crate) unsafe fn fire_unhandled_reporter(pp: *mut Promise) {
    const PREFIX: &[u8] = b"error: ";
    const OBJ_PLACEHOLDER: &[u8] = b"error: <object>\n";
    const ANY_PLACEHOLDER: &[u8] = b"error: <any>\n";
    const NULL_PLACEHOLDER: &[u8] = b"error: null\n";

    UNHANDLED_REJECTION_OCCURRED.store(1, Ordering::Relaxed);

    let reason = unsafe { (*pp).value };
    let is_heap = unsafe { (*pp).value_is_heap };

    if is_heap != 0 && reason != 0 {
        // Type::Any reasons walk through `alloc_rejected_heap` but
        // carry a NaN-box immediate, not a real heap pointer. The
        // cell-like gate isolates real pointers; anything else is
        // surfaced via the `<any>` placeholder so dereferencing the
        // (non-)header can't SIGSEGV.
        if !reason_is_cell_like(reason) {
            unsafe {
                __torajs_syscall_write(2, ANY_PLACEHOLDER.as_ptr(), ANY_PLACEHOLDER.len());
            }
            return;
        }
        let header = reason as *const HeapHeader;
        let type_tag = unsafe { (*header).type_tag };
        if type_tag == TAG_STR {
            unsafe { __torajs_syscall_write(2, PREFIX.as_ptr(), PREFIX.len()) };
            // `str_print_err` already appends a newline.
            unsafe { __torajs_str_print_err(reason as *const u8) };
            return;
        }
        if type_tag == TAG_OBJ && unsafe { (*header).flags } & FLAG_ERROR != 0 {
            unsafe { report_error_instance(reason as *const u8) };
            return;
        }
        unsafe {
            __torajs_syscall_write(2, OBJ_PLACEHOLDER.as_ptr(), OBJ_PLACEHOLDER.len());
        }
        return;
    }

    if reason == 0 && is_heap == 0 {
        // i64 0 + non-heap — spec-wise this is "rejected with the
        // integer 0", but in practice this is the default-reject
        // sentinel for null / unknown reasons (Promise.reject(),
        // Promise.all([pending]), thenable absorption of pending).
        // Bun prints `null` for these, so we match.
        unsafe { __torajs_syscall_write(2, NULL_PLACEHOLDER.as_ptr(), NULL_PLACEHOLDER.len()) };
        return;
    }

    // Primitive non-zero — format reason as a signed decimal.
    unsafe {
        __torajs_syscall_write(2, PREFIX.as_ptr(), PREFIX.len());
        write_i64_decimal_stderr(reason);
        __torajs_syscall_write(2, b"\n".as_ptr(), 1);
    }
}

/// Report an Error-derived instance as `name: message\n`, the
/// Error's own `name` standing in for the `error: ` label every
/// other reason shape carries.
///
/// `: message` is omitted when the message is empty, matching the
/// `Error.prototype.stack` first-line shape (`"Error"`, not
/// `"Error: "`) and the uncaught-throw twin in
/// `torajs_throw::uncaught`.
///
/// # Safety
///
/// `p` must point at a live `Tag::Obj` instance carrying
/// [`FLAG_ERROR`]. A null `name` / `message` slot is tolerated
/// rather than dereferenced: this reporter runs at process exit on
/// a value nobody handled, so a partially-constructed Error must
/// still yield a line rather than a fault.
unsafe fn report_error_instance(p: *const u8) {
    // §20.5.3.2 — see the `torajs_throw::uncaught` twin: the `name`
    // slot normally holds the own-absence sentinel, so it is resolved
    // rather than read at its offset.
    let name_ptr = unsafe { __torajs_error_name_get(p) };
    let msg_ptr = unsafe { (p.add(OBJ_MESSAGE_OFF) as *const usize).read() } as *const u8;

    unsafe { __torajs_str_write_err(name_ptr) };

    let msg_len = if msg_ptr.is_null() || unsafe { __torajs_str_is_undef(msg_ptr) } != 0 {
        0
    } else {
        unsafe { (msg_ptr.add(STR_LEN_OFF) as *const u32).read() as usize }
    };
    if msg_len > 0 {
        unsafe { __torajs_syscall_write(2, b": ".as_ptr(), 2) };
        unsafe { __torajs_str_write_err(msg_ptr) };
    }
    unsafe { __torajs_syscall_write(2, b"\n".as_ptr(), 1) };
}

/// Write `n` as a signed decimal to stderr. Stack buffer is sized
/// for `i64::MIN` (`-9223372036854775808`, 20 chars) + sign. Single
/// `write(2)` keeps the line atomic.
unsafe fn write_i64_decimal_stderr(n: i64) {
    let mut buf = [0u8; 21];
    let mut idx = buf.len();
    let (mut abs, negative) = if n < 0 {
        // Use wrapping_neg so i64::MIN doesn't trap; its abs as u64
        // is exactly 1 << 63 which fits unsigned.
        (n.wrapping_neg() as u64, true)
    } else {
        (n as u64, false)
    };
    if abs == 0 {
        idx -= 1;
        buf[idx] = b'0';
    } else {
        while abs > 0 {
            idx -= 1;
            buf[idx] = b'0' + (abs % 10) as u8;
            abs /= 10;
        }
    }
    if negative {
        idx -= 1;
        buf[idx] = b'-';
    }
    let n_bytes = buf.len() - idx;
    unsafe {
        __torajs_syscall_write(2, buf.as_ptr().add(idx), n_bytes);
    }
}
