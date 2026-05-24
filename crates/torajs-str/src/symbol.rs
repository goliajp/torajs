//! Symbol primitive substrate — port of `runtime_str.c` L1543-1736.
//!
//! ECMAScript `Symbol` value: 16-byte heap object with a desc Str
//! pointer. Includes:
//!
//! - Constructor / drop / refcount-aware lifecycle.
//! - `.description` / `.toString()` / print dispatch.
//! - `Symbol.for(key)` / `Symbol.keyFor(s)` global registry
//!   (linear scan, 256-slot cap matching the C port).
//! - 3 well-known singletons: `Symbol.iterator`,
//!   `Symbol.asyncIterator`, `Symbol.toPrimitive` — lazy-init on
//!   first access, lifetime-of-process refs.
//!
//! ## Layout (16 bytes)
//!
//! ```text
//!   +0..7  : universal heap header (rc u32 + type_tag=7 + flags)
//!   +8..15 : desc str ptr (`*mut Str` or NULL for `Symbol()`)
//! ```

use core::ffi::c_void;
use std::sync::Mutex;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

use crate::layout::{STR_HDR_SIZE, STR_LEN_OFF};

/// `__TORAJS_TAG_SYMBOL` — heap header tag for Symbol.
pub const TAG_SYMBOL: u16 = 7;

/// `__TORAJS_FLAG_STATIC_LITERAL` — set on static literal Strs (and
/// well-known Symbols) so drop is a no-op.
pub const FLAG_STATIC_LITERAL: u16 = 4;

pub const SYMBOL_SIZE: usize = 16;
pub const SYMBOL_DESC_OFF: usize = 8;

// Cross-tier extern decls — resolved at `tr build` link time
// against torajs-rc + runtime_str.c.
unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_rc_dec(p: *mut c_void) -> i32;
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
    fn __torajs_str_eq(a: *const u8, b: *const u8) -> i64;
    fn __torajs_panic(msg: *const u8) -> !;
    // libc — for symbol_print to match the C runtime's libc-stdout
    // buffering (Rust's stdout().lock() bypasses libc's FILE *stdout
    // buffer, causing reordering vs other prints). printf uses
    // implicit stdout — no need to import the platform-specific
    // `stdout` symbol (macOS = `__stdoutp`, Linux = `stdout`).
    fn printf(fmt: *const u8, ...) -> i32;
}

#[repr(C)]
struct HeapHeader {
    refcount: u32,
    type_tag: u16,
    flags: u16,
}

#[inline]
unsafe fn symbol_desc(p: *const c_void) -> *mut c_void {
    unsafe { *((p as *const u8).add(SYMBOL_DESC_OFF) as *const *mut c_void) }
}

#[inline]
unsafe fn symbol_flags(p: *const c_void) -> u16 {
    unsafe { (*(p as *const HeapHeader)).flags }
}

/// Allocate a fresh Symbol object with the given `desc` Str (rc'd
/// by the constructor — desc may be NULL).
///
/// # Safety
///
/// `desc` is null or a live `*Str`. Returned pointer is a `*Symbol`
/// (rc=1).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_symbol_alloc(desc: *mut c_void) -> *mut c_void {
    let p = unsafe { std::alloc::alloc(symbol_layout()) };
    if p.is_null() {
        return core::ptr::null_mut();
    }
    unsafe {
        let h = p as *mut HeapHeader;
        (*h).refcount = 1;
        (*h).type_tag = TAG_SYMBOL;
        (*h).flags = 0;
        __torajs_rc_inc(desc);
        *(p.add(SYMBOL_DESC_OFF) as *mut *mut c_void) = desc;
    }
    p as *mut c_void
}

fn symbol_layout() -> std::alloc::Layout {
    std::alloc::Layout::from_size_align(SYMBOL_SIZE, 8).unwrap()
}

/// # Safety
///
/// `p` is null or a `*Symbol`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_symbol_drop(p: *mut c_void) {
    if p.is_null() {
        return;
    }
    if unsafe { symbol_flags(p) } & FLAG_STATIC_LITERAL != 0 {
        return;
    }
    if unsafe { __torajs_rc_dec(p) } == 0 {
        return;
    }
    let desc = unsafe { symbol_desc(p) };
    if !desc.is_null() {
        unsafe { __torajs_str_drop(desc) };
    }
    unsafe { std::alloc::dealloc(p as *mut u8, symbol_layout()) };
}

/// `Symbol.prototype.toString()` → `"Symbol(<desc>)"` /
/// `"Symbol()"`. NULL receiver → `"undefined"`.
///
/// # Safety
///
/// `p` is null or a `*Symbol`. Returned pointer is a pooled Str
/// (rc=1).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_symbol_to_str(p: *const c_void) -> *mut u8 {
    if p.is_null() {
        return unsafe { alloc_str(b"undefined") };
    }
    let desc = unsafe { symbol_desc(p) };
    let desc_len = if desc.is_null() {
        0
    } else {
        unsafe { *((desc as *const u8).add(STR_LEN_OFF) as *const u64) }
    };
    let total = 8 + desc_len; // "Symbol(" + desc + ")"
    let r = unsafe { __torajs_str_alloc_pooled(total) };
    if !r.is_null() {
        unsafe {
            let dst = r.add(STR_HDR_SIZE);
            core::ptr::copy_nonoverlapping(b"Symbol(".as_ptr(), dst, 7);
            if desc_len > 0 {
                let src = (desc as *const u8).add(STR_HDR_SIZE);
                core::ptr::copy_nonoverlapping(src, dst.add(7), desc_len as usize);
            }
            *dst.add(7 + desc_len as usize) = b')';
        }
    }
    r
}

unsafe fn alloc_str(bytes: &[u8]) -> *mut u8 {
    let p = unsafe { __torajs_str_alloc_pooled(bytes.len() as u64) };
    if !p.is_null() && !bytes.is_empty() {
        unsafe {
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), p.add(STR_HDR_SIZE), bytes.len());
        }
    }
    p
}

/// `Symbol.prototype.description` — returns the rc'd desc Str, or
/// NULL for `Symbol()` (caller's `Nullable<String>` slot maps to JS
/// `undefined`).
///
/// # Safety
///
/// `p` is null or a `*Symbol`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_symbol_description(p: *const c_void) -> *mut c_void {
    if p.is_null() {
        return core::ptr::null_mut();
    }
    let desc = unsafe { symbol_desc(p) };
    if !desc.is_null() {
        unsafe { __torajs_rc_inc(desc) };
    }
    desc
}

/// `console.log(sym)` dispatch → `"Symbol(<desc>)\n"` /
/// `"Symbol()\n"`.
///
/// # Safety
///
/// `p` is null or a `*Symbol`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_symbol_print(p: *const c_void) {
    if p.is_null() {
        unsafe { printf(b"undefined\n\0".as_ptr()) };
        return;
    }
    let desc = unsafe { symbol_desc(p) };
    unsafe { printf(b"Symbol(\0".as_ptr()) };
    if !desc.is_null() {
        let len = unsafe { *((desc as *const u8).add(STR_LEN_OFF) as *const u64) } as i32;
        if len > 0 {
            // %.*s — pointer + length pair (binary-safe for NUL bytes).
            unsafe {
                printf(
                    b"%.*s\0".as_ptr(),
                    len,
                    (desc as *const u8).add(STR_HDR_SIZE),
                );
            }
        }
    }
    unsafe { printf(b")\n\0".as_ptr()) };
}

// ---- Symbol.for / Symbol.keyFor registry ----

const SYMBOL_REG_MAX: usize = 256;

static SYMBOL_REG: Mutex<Vec<usize>> = Mutex::new(Vec::new()); // sym ptrs as usize

/// `Symbol.for(key)` — lookup-or-create registered Symbol.
///
/// # Safety
///
/// `key` is null or a live `*Str`. Returned Symbol pointer is rc'd
/// for the caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_symbol_for(key: *mut c_void) -> *mut c_void {
    if key.is_null() {
        unsafe {
            __torajs_panic(b"TypeError: Symbol.for requires a string key\0".as_ptr());
        }
    }
    let mut reg = SYMBOL_REG.lock().unwrap();
    // Linear scan — same shape as C port.
    for &sym_usize in reg.iter() {
        let sym = sym_usize as *mut c_void;
        let desc = unsafe { symbol_desc(sym) };
        if !desc.is_null() && unsafe { __torajs_str_eq(desc as *const u8, key as *const u8) } != 0 {
            unsafe { __torajs_rc_inc(sym) };
            return sym;
        }
    }
    if reg.len() >= SYMBOL_REG_MAX {
        unsafe {
            __torajs_panic(b"Symbol.for registry full (>256 unique keys)\0".as_ptr());
        }
    }
    let sym = unsafe { __torajs_symbol_alloc(key) };
    unsafe { __torajs_rc_inc(sym) };
    reg.push(sym as usize);
    sym
}

/// `Symbol.keyFor(sym)` — returns the registered key Str (rc'd) or
/// NULL.
///
/// # Safety
///
/// `sym` is null or a `*Symbol`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_symbol_key_for(sym: *mut c_void) -> *mut c_void {
    if sym.is_null() {
        return core::ptr::null_mut();
    }
    let reg = SYMBOL_REG.lock().unwrap();
    for &reg_sym in reg.iter() {
        if reg_sym == sym as usize {
            let desc = unsafe { symbol_desc(sym) };
            if !desc.is_null() {
                unsafe { __torajs_rc_inc(desc) };
            }
            return desc;
        }
    }
    core::ptr::null_mut()
}

// ---- Well-known Symbol singletons ----
// Process-lifetime — lazy init via AtomicPtr CAS.

static WELL_KNOWN_ITERATOR: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
static WELL_KNOWN_ASYNC_ITERATOR: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
static WELL_KNOWN_TO_PRIMITIVE: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());

unsafe fn ensure_well_known(slot: &AtomicPtr<c_void>, desc_bytes: &[u8]) -> *mut c_void {
    let existing = slot.load(Ordering::Acquire);
    if !existing.is_null() {
        unsafe { __torajs_rc_inc(existing) };
        return existing;
    }
    let desc = unsafe { alloc_str(desc_bytes) } as *mut c_void;
    let sym = unsafe { __torajs_symbol_alloc(desc) };
    unsafe { __torajs_str_drop(desc) }; // symbol_alloc bumped rc
    // CAS — first racing init wins; loser drops its sym.
    match slot.compare_exchange(
        core::ptr::null_mut(),
        sym,
        Ordering::AcqRel,
        Ordering::Acquire,
    ) {
        Ok(_) => {
            unsafe { __torajs_rc_inc(sym) }; // caller's ref
            sym
        }
        Err(winner) => {
            unsafe { __torajs_symbol_drop(sym) };
            unsafe { __torajs_rc_inc(winner) };
            winner
        }
    }
}

/// `Symbol.iterator` — lazy-init singleton.
///
/// # Safety
///
/// Cross-tier extern allocators must be linkable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_symbol_iterator() -> *mut c_void {
    unsafe { ensure_well_known(&WELL_KNOWN_ITERATOR, b"Symbol.iterator") }
}

/// `Symbol.asyncIterator` — lazy-init singleton.
///
/// # Safety
///
/// Cross-tier extern allocators must be linkable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_symbol_async_iterator() -> *mut c_void {
    unsafe { ensure_well_known(&WELL_KNOWN_ASYNC_ITERATOR, b"Symbol.asyncIterator") }
}

/// `Symbol.toPrimitive` — lazy-init singleton.
///
/// # Safety
///
/// Cross-tier extern allocators must be linkable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_symbol_to_primitive() -> *mut c_void {
    unsafe { ensure_well_known(&WELL_KNOWN_TO_PRIMITIVE, b"Symbol.toPrimitive") }
}

// Suppress unused warning on the atomic helper since it lives only
// for the FFI compare-exchange loop.
#[allow(dead_code)]
static _SYMBOL_REG_COUNT: AtomicUsize = AtomicUsize::new(0);
