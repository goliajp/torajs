//! Ryū dtoa — shortest-roundtrip f64 → decimal string.
//!
//! **Step 15-a scaffold only.** The Ryū algorithm body ships
//! in Step 15-b — see [Adams, PLDI 2018,
//! "Ryū: Fast Float-to-String Conversion"] for the canonical
//! reference. This scaffold establishes the public extern
//! signature so call-site cutover (Step 15-d) can target the
//! final ABI without churn when the algorithm body lands.
//!
//! [Adams, PLDI 2018, "Ryū: Fast Float-to-String Conversion"]:
//!     https://dl.acm.org/doi/10.1145/3192366.3192369

/// `__torajs_fmt_dtoa(d, out_buf, out_cap) -> bytes_written | -1`.
///
/// Step 15-a scaffold: returns -2 (not-yet-implemented sentinel)
/// for every input. Step 15-b replaces with the Ryū body.
///
/// # Safety
///
/// `out_buf` must be writable for at least `out_cap` bytes.
/// Caller's `out_cap` must be ≥ 32 (every valid f64's
/// shortest-roundtrip representation fits in 24 bytes; the
/// extra 8 bytes are headroom for the algorithm's intermediate
/// scratch).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_fmt_dtoa(_d: f64, _out_buf: *mut u8, _out_cap: usize) -> i32 {
    // Step 15-a scaffold: not-yet-implemented sentinel. Step
    // 15-b ships the Ryū body. Callers must not invoke this
    // entry point until 15-b lands (no Cargo.toml dep from
    // torajs-num / torajs-arr / torajs-str yet — added in 15-d).
    -2
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sanity: scaffold returns -2 (not-yet-implemented). Step
    /// 15-b replaces this test with the ≥ 100 round-trip suite.
    #[test]
    fn scaffold_sentinel() {
        let mut buf = [0u8; 32];
        let n = unsafe { __torajs_fmt_dtoa(3.14, buf.as_mut_ptr(), buf.len()) };
        assert_eq!(
            n, -2,
            "Step 15-a scaffold must return the not-yet-implemented sentinel"
        );
    }
}
