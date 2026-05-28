//! Eisel-Lemire atod — string → f64 parsing.
//!
//! **Step 15-a scaffold only.** The Eisel-Lemire algorithm body
//! ships in Step 15-c — see [Lemire, SP&E 2021, "Number Parsing
//! at a Gigabyte per Second"] for the canonical reference. This
//! scaffold establishes the public extern signature so call-
//! site cutover (Step 15-e) can target the final ABI without
//! churn when the algorithm body lands.
//!
//! [Lemire, SP&E 2021, "Number Parsing at a Gigabyte per Second"]:
//!     https://onlinelibrary.wiley.com/doi/10.1002/spe.2984

/// `__torajs_fmt_atod(s, len, endp) -> f64`.
///
/// Step 15-a scaffold: returns f64::NAN for every input. Step
/// 15-c replaces with the Eisel-Lemire body.
///
/// # Safety
///
/// `s` must point at ≥ `len` readable bytes. `endp` is null or
/// a writable `*mut usize` slot. Caller is responsible for the
/// slice's validity.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_fmt_atod(_s: *const u8, _len: usize, _endp: *mut usize) -> f64 {
    // Step 15-a scaffold: NaN sentinel. Step 15-c ships the
    // Eisel-Lemire body. Callers must not invoke this entry
    // point until 15-c lands.
    f64::NAN
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sanity: scaffold returns NaN. Step 15-c replaces this
    /// test with the ≥ 100 round-trip suite.
    #[test]
    fn scaffold_sentinel() {
        let buf = b"3.14";
        let r = unsafe { __torajs_fmt_atod(buf.as_ptr(), buf.len(), core::ptr::null_mut()) };
        assert!(r.is_nan(), "Step 15-a scaffold must return NaN sentinel");
    }
}
