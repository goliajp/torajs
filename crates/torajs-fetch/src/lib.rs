//! `fetch(url)` HTTP client substrate for the torajs AOT
//! TypeScript runtime.
//!
//! Layer-3 substrate (P6.3, 2026-05-24) — replaces
//! `runtime_fetch.c` (189 C LOC). Wraps a single synchronous GET
//! into a tora `Response` heap object:
//!
//! ```text
//! Response (24 bytes)
//!   +0..7   : universal heap header (refcount + type_tag=9 + flags)
//!   +8      : status i64 (HTTP code; 0 on transport error)
//!   +16     : body *Str (UTF-8 bytes returned by the server)
//! ```
//!
//! SSA-side `fetch(url)` lowers to
//! `promise_alloc_fulfilled_heap(__torajs_fetch_sync(url))` giving
//! `Promise<Response>`. The user awaits it; `.text()` unwraps `body`
//! (already owned), `.status` reads the i64 field.
//!
//! ## v0.6 MVP scope
//!
//! - GET only (no POST / custom headers / body / method)
//! - Synchronous (real-suspending fetch is T-16's state-machine
//!   async/await territory)
//! - HTTPS via libcurl's bundled OpenSSL/SecureTransport
//! - Follow-redirects on (matches Bun)
//! - Body returned as a Str (UTF-8 bytes — runtime doesn't validate)
//!
//! ## Runtime gating
//!
//! Native-only. wasm32-wasi has no libcurl; per spec it should
//! route through the browser fetch API instead (T-21.b
//! follow-up). When compiled for `target_os = "wasi"`, the crate
//! exports a stub that returns an empty Response (status=0) +
//! emits no link reference to libcurl.

#[cfg(not(target_os = "wasi"))]
mod native;

// Cross-tier extern stubs for cargo unit tests — real symbols
// live in sibling staticlibs (torajs-str + torajs-rc) +
// runtime_str.c at `tr build` link time. cargo test for
// torajs-fetch doesn't link those, so panicking stubs keep the
// test binary linking clean. Same pattern as torajs-promise /
// torajs-regex / torajs-cycle test stubs.

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_alloc_pooled(_len: u64) -> *mut u8 {
    panic!(
        "torajs-fetch test stub: __torajs_str_alloc_pooled should not be called from cargo test"
    );
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_rc_dec(_p: *mut core::ffi::c_void) -> i32 {
    panic!("torajs-fetch test stub: __torajs_rc_dec should not be called from cargo test");
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_value_drop_heap(_p: *mut core::ffi::c_void) {
    panic!("torajs-fetch test stub: __torajs_value_drop_heap should not be called from cargo test");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_layout_matches_c_port() {
        // The Response heap block layout is fixed at 24 bytes:
        //   header 8 + status 8 + body_ptr 8.
        // ssa_lower reads `.status` at +8 and `.text()` body at +16,
        // so the offsets are part of the cross-tier ABI.
        assert_eq!(RESPONSE_SIZE, 24);
        assert_eq!(RESPONSE_STATUS_OFF, 8);
        assert_eq!(RESPONSE_BODY_OFF, 16);
        assert_eq!(TAG_RESPONSE, 9);
    }
}

// ---- Public constants shared with the native impl module ----

/// `__TORAJS_TAG_RESPONSE` — heap header type tag for Response.
/// Matches runtime_str.c's `value_drop_heap` dispatch on tag=9.
pub const TAG_RESPONSE: u16 = 9;

/// Total Response heap block size in bytes.
pub const RESPONSE_SIZE: usize = 24;

/// Byte offset of `status` (i64) within the Response block.
pub const RESPONSE_STATUS_OFF: usize = 8;

/// Byte offset of `body` (*Str) within the Response block.
pub const RESPONSE_BODY_OFF: usize = 16;

// wasm32-wasi target — fetch is intentionally absent (matches
// runtime_fetch.c's `#ifndef __wasi__` gate). T-21.b lands the
// browser-API routing later.
