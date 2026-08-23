//! Native (non-wasm) implementation — wraps libcurl-easy for one
//! synchronous GET. Port of `runtime_fetch.c` L39-189.
//!
//! Cross-tier externs (`__torajs_str_alloc_pooled`, `__torajs_rc_dec`,
//! `__torajs_value_drop_heap`) resolve at `tr build` link time
//! against the other staticlibs + the remaining runtime_str.c block.
//!
//! libcurl ABI surface (resolved lazily via `dlopen`/`dlsym` on the
//! first `fetch` call — S2 startup knife, 2026-08-24):
//! - `curl_easy_init / cleanup / perform / setopt / getinfo`
//! - 6 `CURLOPT_*` enum values for the options we set
//! - `CURLINFO_RESPONSE_CODE` for the status retrieval
//!
//! Pre-knife this block carried `#[link(name = "curl")]`, which put
//! an `LC_LOAD_DYLIB /usr/lib/libcurl.4.dylib` into EVERY AOT
//! binary (the fetch staticlib is always baked): a C-hello A/B
//! measured that eager load at **+0.76 ms wall / +0.53 ms user** —
//! the bulk of the tr-vs-rust startup gap (2.3 vs 1.3 ms). With the
//! lazy table nothing references libcurl at link time, the linker's
//! ordinal-2 accounting emits no LC (`archive_emit_lc_meta.rs`
//! `has_libcurl_lc`), and a program that does call `fetch` pays one
//! `dlopen` (~0.5 ms) folded into its first multi-ms network RTT.
//! `dlopen`/`dlsym` are libSystem exports, SD-1 resolved.
//!
//! `curl_easy_setopt` keeps its C-variadic type through the fn
//! pointer — on aarch64-darwin variadic args travel on the stack,
//! so a fixed-arity pointer cast would mis-place every argument.

use core::ffi::{c_char, c_long, c_void};

use crate::{RESPONSE_BODY_OFF, RESPONSE_SIZE, RESPONSE_STATUS_OFF, TAG_RESPONSE};

// ---- libcurl ABI ----
// Mirrors the constants we use from <curl/curl.h>. Values from
// libcurl 7.x stable ABI — match curl headers shipped with macOS
// + linux distros.

const CURLE_OK: i32 = 0;

const CURLOPT_URL: i32 = 10002;
const CURLOPT_WRITEFUNCTION: i32 = 20011;
const CURLOPT_WRITEDATA: i32 = 10001;
const CURLOPT_FOLLOWLOCATION: i32 = 52;
const CURLOPT_TIMEOUT: i32 = 13;
const CURLOPT_CONNECTTIMEOUT: i32 = 78;
const CURLOPT_USERAGENT: i32 = 10018;

const CURLINFO_RESPONSE_CODE: i32 = 0x200002;

#[repr(C)]
struct CURL {
    _opaque: [u8; 0],
}

// ---- lazy libcurl table (S2 startup knife — see module doc) ----

unsafe extern "C" {
    fn dlopen(path: *const c_char, mode: i32) -> *mut c_void;
    fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
}

/// `RTLD_LAZY` on darwin.
const RTLD_LAZY: i32 = 0x1;

type CurlInit = unsafe extern "C" fn() -> *mut CURL;
type CurlCleanup = unsafe extern "C" fn(*mut CURL);
type CurlPerform = unsafe extern "C" fn(*mut CURL) -> i32;
type CurlSetopt = unsafe extern "C" fn(*mut CURL, i32, ...) -> i32;
type CurlGetinfo = unsafe extern "C" fn(*mut CURL, i32, ...) -> i32;

struct CurlApi {
    init: CurlInit,
    cleanup: CurlCleanup,
    perform: CurlPerform,
    setopt: CurlSetopt,
    getinfo: CurlGetinfo,
}

/// Resolved-once libcurl entry points. AtomicPtr keeps the shape
/// multi-thread-ready (§6.2): a racing second resolver would build
/// an identical table and drop it on the CAS loss.
static CURL_API: core::sync::atomic::AtomicPtr<CurlApi> =
    core::sync::atomic::AtomicPtr::new(core::ptr::null_mut());

/// dlopen + dlsym the five `curl_easy_*` entry points on first use.
/// `None` = libcurl unavailable (or a symbol missing) — the caller
/// answers the same status-0 Response as a failed
/// `curl_easy_init`.
fn curl_api() -> Option<&'static CurlApi> {
    use core::sync::atomic::Ordering;
    let cached = CURL_API.load(Ordering::Acquire);
    if !cached.is_null() {
        return Some(unsafe { &*cached });
    }
    let handle = unsafe { dlopen(c"/usr/lib/libcurl.4.dylib".as_ptr(), RTLD_LAZY) };
    if handle.is_null() {
        return None;
    }
    let init = unsafe { dlsym(handle, c"curl_easy_init".as_ptr()) };
    let cleanup = unsafe { dlsym(handle, c"curl_easy_cleanup".as_ptr()) };
    let perform = unsafe { dlsym(handle, c"curl_easy_perform".as_ptr()) };
    let setopt = unsafe { dlsym(handle, c"curl_easy_setopt".as_ptr()) };
    let getinfo = unsafe { dlsym(handle, c"curl_easy_getinfo".as_ptr()) };
    if init.is_null()
        || cleanup.is_null()
        || perform.is_null()
        || setopt.is_null()
        || getinfo.is_null()
    {
        return None;
    }
    // SAFETY: the five pointers are non-null exports of libcurl's
    // stable 7.x ABI; the transmutes re-type them to the matching
    // C signatures.
    let table = Box::new(unsafe {
        CurlApi {
            init: core::mem::transmute::<*mut c_void, CurlInit>(init),
            cleanup: core::mem::transmute::<*mut c_void, CurlCleanup>(cleanup),
            perform: core::mem::transmute::<*mut c_void, CurlPerform>(perform),
            setopt: core::mem::transmute::<*mut c_void, CurlSetopt>(setopt),
            getinfo: core::mem::transmute::<*mut c_void, CurlGetinfo>(getinfo),
        }
    });
    let raw = Box::into_raw(table);
    match CURL_API.compare_exchange(
        core::ptr::null_mut(),
        raw,
        Ordering::AcqRel,
        Ordering::Acquire,
    ) {
        Ok(_) => Some(unsafe { &*raw }),
        Err(winner) => {
            drop(unsafe { Box::from_raw(raw) });
            Some(unsafe { &*winner })
        }
    }
}

// ---- Cross-tier extern (runtime_str.c + torajs-str + torajs-rc) ----

#[cfg(not(test))]
unsafe extern "C" {
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
    fn __torajs_rc_dec(p: *mut c_void) -> i32;
    fn __torajs_value_drop_heap(p: *mut c_void);
}

#[cfg(test)]
unsafe extern "C" {
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
    fn __torajs_rc_dec(p: *mut c_void) -> i32;
    fn __torajs_value_drop_heap(p: *mut c_void);
}

// Universal heap header — matches runtime_str.c's
// `__torajs_heap_header_t` byte-for-byte (#[repr(C)] keeps the
// fields in source order with no padding before refcount).
#[repr(C)]
struct HeapHeader {
    refcount: u32,
    type_tag: u16,
    flags: u16,
}

const STR_HDR_SIZE: usize = 16;

// ---- libcurl write callback ----
// Accumulates response body bytes into a Vec; the final body Str
// is built from the slice once curl_easy_perform returns.

struct FetchBuf {
    data: Vec<u8>,
}

unsafe extern "C" fn fetch_write_cb(
    src: *mut c_void,
    size: usize,
    nmemb: usize,
    user: *mut c_void,
) -> usize {
    let add = size.saturating_mul(nmemb);
    if add == 0 {
        return 0;
    }
    let buf = unsafe { &mut *(user as *mut FetchBuf) };
    let slice = unsafe { core::slice::from_raw_parts(src as *const u8, add) };
    buf.data.extend_from_slice(slice);
    add
}

// ---- Helpers ----

/// Read a tora Str's payload into a heap NUL-terminated `Vec<u8>`
/// (suitable for passing to libcurl as CURLOPT_URL).
unsafe fn str_to_cstring(str_ptr: *const c_void) -> Vec<u8> {
    if str_ptr.is_null() {
        return vec![0u8];
    }
    let len = unsafe { *((str_ptr as *const u8).add(8) as *const u32) } as usize;
    let mut out = Vec::with_capacity(len + 1);
    let data_ptr = unsafe { (str_ptr as *const u8).add(STR_HDR_SIZE) };
    unsafe {
        out.extend_from_slice(core::slice::from_raw_parts(data_ptr, len));
    }
    out.push(0);
    out
}

/// Layout of the Response heap block (24 bytes, alignment of u64).
pub(crate) fn response_layout() -> std::alloc::Layout {
    // SAFETY: compile-time-const size (24) + align (8) satisfy Layout
    // invariants. Unchecked ctor avoids pulling Rust's Layout::Err
    // formatting path into the user binary (polish A3).
    unsafe { std::alloc::Layout::from_size_align_unchecked(RESPONSE_SIZE, 8) }
}

/// Initialize a freshly-allocated Response block at `block` with
/// `status` and `body_str_ptr`. Sets header to refcount=1 +
/// type_tag=TAG_RESPONSE.
///
/// # Safety
///
/// `block` must point at a writable RESPONSE_SIZE-byte allocation
/// with at least 8-byte alignment.
pub(crate) unsafe fn init_response(block: *mut u8, status: i64, body_str_ptr: *mut c_void) {
    unsafe {
        let h = block as *mut HeapHeader;
        (*h).refcount = 1;
        (*h).type_tag = TAG_RESPONSE;
        (*h).flags = 0;
        *(block.add(RESPONSE_STATUS_OFF) as *mut i64) = status;
        *(block.add(RESPONSE_BODY_OFF) as *mut *mut c_void) = body_str_ptr;
    }
}

unsafe fn alloc_response(status: i64, body_str_ptr: *mut c_void) -> *mut c_void {
    let layout = response_layout();
    let block = unsafe { std::alloc::alloc(layout) };
    if block.is_null() {
        return core::ptr::null_mut();
    }
    unsafe { init_response(block, status, body_str_ptr) };
    block as *mut c_void
}

unsafe fn empty_body_response(status: i64) -> *mut c_void {
    let body = unsafe { __torajs_str_alloc_pooled(0) };
    unsafe { alloc_response(status, body as *mut c_void) }
}

// ---- Public extern API ----

/// `fetch(url)` runtime entrypoint. `url_str_ptr` is a tora `*Str`.
/// Returns a heap `Response*` (rc=1; caller transfers via
/// `Promise.value`). Transport error (DNS / TLS / connection
/// refused / ...) yields `status=0` + empty body, surfaced as a
/// clearly-abnormal Response without a separate "throw" path.
///
/// # Safety
///
/// `url_str_ptr` is null or a live `*Str`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_fetch_sync(url_str_ptr: *mut c_void) -> *mut c_void {
    let url_c = unsafe { str_to_cstring(url_str_ptr) };
    // Lazy-resolve libcurl; unavailable ≙ failed curl_easy_init
    // (status-0 transport-error Response, no separate throw path).
    let Some(curl) = curl_api() else {
        return unsafe { empty_body_response(0) };
    };
    let handle = unsafe { (curl.init)() };
    if handle.is_null() {
        return unsafe { empty_body_response(0) };
    }
    let mut buf = FetchBuf { data: Vec::new() };
    unsafe {
        (curl.setopt)(handle, CURLOPT_URL, url_c.as_ptr() as *const c_char);
        let cb_ptr: unsafe extern "C" fn(*mut c_void, usize, usize, *mut c_void) -> usize =
            fetch_write_cb;
        (curl.setopt)(handle, CURLOPT_WRITEFUNCTION, cb_ptr);
        (curl.setopt)(
            handle,
            CURLOPT_WRITEDATA,
            &mut buf as *mut FetchBuf as *mut c_void,
        );
        (curl.setopt)(handle, CURLOPT_FOLLOWLOCATION, 1 as c_long);
        // Bun-parity timeouts. 30s total + 10s connect.
        (curl.setopt)(handle, CURLOPT_TIMEOUT, 30 as c_long);
        (curl.setopt)(handle, CURLOPT_CONNECTTIMEOUT, 10 as c_long);
        // User-Agent matches `bun` to avoid origins gating on torajs.
        (curl.setopt)(
            handle,
            CURLOPT_USERAGENT,
            b"torajs/0.6 (libcurl)\0".as_ptr() as *const c_char,
        );
    }
    let rc = unsafe { (curl.perform)(handle) };
    let mut http_status: c_long = 0;
    if rc == CURLE_OK {
        unsafe {
            (curl.getinfo)(
                handle,
                CURLINFO_RESPONSE_CODE,
                &mut http_status as *mut c_long,
            );
        }
    }
    unsafe { (curl.cleanup)(handle) };

    // Build the body Str regardless of rc — on transport error
    // buf.data is empty, yielding an empty Str.
    let body = unsafe { __torajs_str_alloc_pooled(buf.data.len() as u64) };
    if !body.is_null() && !buf.data.is_empty() {
        unsafe {
            core::ptr::copy_nonoverlapping(
                buf.data.as_ptr(),
                body.add(STR_HDR_SIZE),
                buf.data.len(),
            );
        }
    }
    unsafe { alloc_response(http_status as i64, body as *mut c_void) }
}

/// Drop hook — called from runtime_str.c's
/// `__torajs_value_drop_heap` via the `TAG_RESPONSE` case. Releases
/// the body Str (via the generic value_drop_heap path so substrings
/// + interned strings drop correctly) then deallocates the Response
/// block itself.
///
/// # Safety
///
/// `p` is null or a Response pointer previously returned by
/// `__torajs_fetch_sync`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_response_drop(p: *mut c_void) {
    if p.is_null() {
        return;
    }
    if unsafe { __torajs_rc_dec(p) } == 0 {
        return;
    }
    let body = unsafe { *((p as *mut u8).add(RESPONSE_BODY_OFF) as *mut *mut c_void) };
    if !body.is_null() {
        unsafe { __torajs_value_drop_heap(body) };
    }
    unsafe { std::alloc::dealloc(p as *mut u8, response_layout()) };
}
