//! Bug-327 C2.5 — uncaught-throw exit path. Extracted from
//! [`crate`] as the rotation-196 file-size sweep (parent had drifted
//! to 512 prod LOC; rotation 185 audit registered it at 512). The
//! synthesized main's throw-propagate branch calls
//! `__torajs_uncaught_exit_code`, which reads the pending throw
//! atomics, renders it (Str payload / Error `name: message` /
//! placeholder) to stderr via `__torajs_syscall_write`, and yields
//! exit code 1. Verbatim move; nothing about the rendering path
//! changes.
//!
//! `#[unsafe(no_mangle)]` on the extern symbol means the linker
//! finds it regardless of Rust module path — no re-export needed
//! for the AOT `ssa_lower_stmt_throw` / `ssa_lower_main_exit`
//! consumers.

use crate::{ANY_TAG_HEAP, STR_HDR_SIZE, STR_LEN_OFF, THROW_ACTIVE, THROW_TAG, THROW_VALUE};
use core::sync::atomic::Ordering;

unsafe extern "C" {
    /// Raw fd write — Layer-0 syscall shim (torajs-syscall).
    fn __torajs_syscall_write(fd: i32, buf: *const u8, n: usize) -> isize;
}

/// Heap type_tag discriminant for Str blocks (`torajs_rc::Tag::Str`).
/// torajs-throw is Layer-1 (no upstream crate deps) so the value is
/// mirrored, not imported — same convention as [`ANY_TAG_HEAP`].
const HEAP_TAG_STR: u16 = 0;
/// Heap type_tag discriminant for class instances (`torajs_rc::Tag::Obj`).
const HEAP_TAG_OBJ: u16 = 1;
/// Header flags field offset (`torajs_rc::HeapHeader::flags`, u16 @+6).
const HDR_FLAGS_OFF: usize = 6;
/// `torajs_rc::FLAG_ERROR` — set on Error-derived class instances by
/// ssa_lower's `__new_<C>` factory codegen.
const FLAG_ERROR: u16 = 1 << 7;
/// Obj field layout: `[header:32][field0:8][field1:8]…`. Error's
/// declaration order is `message` (field0) then `name` (field1), both
/// Str pointers — mirror of `ssa_lower::OBJ_HEADER_SIZE` (blade 1:
/// props dynobj slot @ +24 pushed field 0 to +32).
const OBJ_MESSAGE_OFF: usize = 32;
const OBJ_NAME_OFF: usize = 40;

/// Write a Str block's payload bytes to stderr. `str_ptr` points at a
/// Str heap object (`[header:8][len:8][bytes:N]`). Null / empty → no-op.
///
/// # Safety
///
/// `str_ptr` must be null or a live Str block. The uncaught reporter is
/// the last line of defense at crash time, so a partially-constructed
/// throw value (null field) is tolerated rather than dereferenced.
unsafe fn write_str_to_stderr(str_ptr: *const u8) {
    if str_ptr.is_null() {
        return;
    }
    let len = unsafe { (str_ptr.add(STR_LEN_OFF) as *const u64).read() } as usize;
    if len > 0 {
        unsafe { __torajs_syscall_write(2, str_ptr.add(STR_HDR_SIZE), len) };
    }
}

/// Report the pending throw to stderr and yield exit code 1. Called
/// by the synthesized main's throw-propagate branch (a throw that
/// escaped every user frame — `emit_throw_check` with `is_main_fn`
/// and an empty try stack). Pre-fix that branch ret'd the I32
/// sentinel 0: a crashing program exited clean and the silent-wrong
/// poisoned every exit-code consumer (test262 runner included).
///
/// Rendering: a thrown Str prints its payload; an Error-derived class
/// instance (FLAG_ERROR set by ssa_lower's factory codegen) prints
/// `name: message` from the Error layout prefix; every other shape
/// (plain objects, numbers, ...) prints a placeholder. Number rendering
/// parity with bun's report is tracked in RFC
/// 20260613-test262-bug327-root-causes.
///
/// # Safety
///
/// `extern "C"` ABI. When the pending tag says Heap the value slot
/// must hold a live heap pointer (the throw machinery's invariant).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_uncaught_exit_code() -> i32 {
    if THROW_ACTIVE.load(Ordering::Relaxed) == 0 {
        return 0;
    }
    const PREFIX: &[u8] = b"error: uncaught ";
    unsafe { __torajs_syscall_write(2, PREFIX.as_ptr(), PREFIX.len()) };
    let tag = THROW_TAG.load(Ordering::Relaxed);
    let value = THROW_VALUE.load(Ordering::Relaxed);
    let mut printed = false;
    if tag == ANY_TAG_HEAP && value != 0 {
        let p = value as *const u8;
        // type_tag lives at offset +4 in the universal heap header.
        let heap_tag = unsafe { (p.add(4) as *const u16).read() };
        if heap_tag == HEAP_TAG_STR {
            unsafe { write_str_to_stderr(p) };
            printed = true;
        } else if heap_tag == HEAP_TAG_OBJ {
            // Error-derived instances carry FLAG_ERROR (set at the
            // `__new_<C>` factory). Render `name: message` from the
            // Error layout prefix (message=field0, name=field1, both
            // Str pointers). The `: message` suffix is omitted when the
            // message is empty — matching the Error.prototype.stack /
            // bun first-line shape ("Error", not "Error: ").
            let flags = unsafe { (p.add(HDR_FLAGS_OFF) as *const u16).read() };
            if flags & FLAG_ERROR != 0 {
                let name_ptr = unsafe { (p.add(OBJ_NAME_OFF) as *const usize).read() } as *const u8;
                let msg_ptr =
                    unsafe { (p.add(OBJ_MESSAGE_OFF) as *const usize).read() } as *const u8;
                unsafe { write_str_to_stderr(name_ptr) };
                let msg_len = if msg_ptr.is_null() {
                    0usize
                } else {
                    unsafe { (msg_ptr.add(STR_LEN_OFF) as *const u64).read() as usize }
                };
                if msg_len > 0 {
                    unsafe { __torajs_syscall_write(2, b": ".as_ptr(), 2) };
                    unsafe { write_str_to_stderr(msg_ptr) };
                }
                printed = true;
            }
        }
    }
    if !printed {
        const PLACEHOLDER: &[u8] = b"exception";
        unsafe { __torajs_syscall_write(2, PLACEHOLDER.as_ptr(), PLACEHOLDER.len()) };
    }
    unsafe { __torajs_syscall_write(2, b"\n".as_ptr(), 1) };
    1
}
