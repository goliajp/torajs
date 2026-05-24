//! Native (non-wasm) implementation — wraps libcurl-easy for one
//! synchronous GET. Port of `runtime_fetch.c` L39-189.
//!
//! Cross-tier externs (`__torajs_str_alloc_pooled`, `__torajs_rc_dec`,
//! `__torajs_value_drop_heap`) resolve at `tr build` link time
//! against the other staticlibs + the remaining runtime_str.c block.
//!
//! libcurl ABI surface (resolved via `#[link(name = "curl")]`):
//! - `curl_easy_init / cleanup / perform / setopt / getinfo`
//! - 6 `CURLOPT_*` enum values for the options we set
//! - `CURLINFO_RESPONSE_CODE` for the status retrieval
//!
//! `curl_easy_setopt` is declared with C variadic so we can call
//! it with the per-option value type (str / i64 / fn-ptr / data-
//! ptr) — same single-arg dispatch the C side does.

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

#[link(name = "curl")]
unsafe extern "C" {
    fn curl_easy_init() -> *mut CURL;
    fn curl_easy_cleanup(handle: *mut CURL);
    fn curl_easy_perform(handle: *mut CURL) -> i32;
    fn curl_easy_setopt(handle: *mut CURL, option: i32, ...) -> i32;
    fn curl_easy_getinfo(handle: *mut CURL, info: i32, ...) -> i32;
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
    let len = unsafe { *((str_ptr as *const u8).add(8) as *const u64) } as usize;
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
    std::alloc::Layout::from_size_align(RESPONSE_SIZE, 8).unwrap()
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
    let handle = unsafe { curl_easy_init() };
    if handle.is_null() {
        return unsafe { empty_body_response(0) };
    }
    let mut buf = FetchBuf { data: Vec::new() };
    unsafe {
        curl_easy_setopt(handle, CURLOPT_URL, url_c.as_ptr() as *const c_char);
        let cb_ptr: unsafe extern "C" fn(*mut c_void, usize, usize, *mut c_void) -> usize =
            fetch_write_cb;
        curl_easy_setopt(handle, CURLOPT_WRITEFUNCTION, cb_ptr);
        curl_easy_setopt(
            handle,
            CURLOPT_WRITEDATA,
            &mut buf as *mut FetchBuf as *mut c_void,
        );
        curl_easy_setopt(handle, CURLOPT_FOLLOWLOCATION, 1 as c_long);
        // Bun-parity timeouts. 30s total + 10s connect.
        curl_easy_setopt(handle, CURLOPT_TIMEOUT, 30 as c_long);
        curl_easy_setopt(handle, CURLOPT_CONNECTTIMEOUT, 10 as c_long);
        // User-Agent matches `bun` to avoid origins gating on torajs.
        curl_easy_setopt(
            handle,
            CURLOPT_USERAGENT,
            b"torajs/0.6 (libcurl)\0".as_ptr() as *const c_char,
        );
    }
    let rc = unsafe { curl_easy_perform(handle) };
    let mut http_status: c_long = 0;
    if rc == CURLE_OK {
        unsafe {
            curl_easy_getinfo(
                handle,
                CURLINFO_RESPONSE_CODE,
                &mut http_status as *mut c_long,
            );
        }
    }
    unsafe { curl_easy_cleanup(handle) };

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
