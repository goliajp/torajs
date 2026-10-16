//! §22.2.3.1 RegExpInitialize behind the ctor build seam — the body
//! `R(p, f)` and `new R(p, f)` share when `R` is the RegExp constructor
//! held as a value (§22.2.4.1 step 2 hands both forms here).

use core::ffi::c_void;

use torajs_anyvalue::nanbox::{AnyValue, VALUE_UNDEFINED, box_void_ptr, is_undefined};
use torajs_anyvalue::regexp_support as av;

use crate::ops::{Owned, get};

unsafe extern "C" {
    fn __torajs_throw_check() -> i64;
    /// torajs-regex — §22.2.3.1 over two boxed operands: a RegExp
    /// pattern contributes its source (and its flags when `flags` is
    /// undefined), anything else is ToString'd; a rejected pattern is
    /// a pending SyntaxError. Always a live cell (rc 1).
    fn __torajs_regex_compile_any(pattern: u64, flags: u64) -> *mut c_void;
}

/// RegExpAlloc + RegExpInitialize over two AnyValues. An absent /
/// undefined operand is the empty string, which is what makes
/// `new RegExp()` the empty pattern.
///
/// # Safety
/// `pattern` and `flags` are live AnyValues.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_ctor_regexp_build(
    pattern: AnyValue,
    flags: AnyValue,
) -> AnyValue {
    unsafe {
        // Step 1 — IsRegExp runs first; its `@@match` Get is
        // observable (a getter may even recompile the pattern).
        let pattern_is_regexp = av::is_regexp(pattern) != 0;
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        // Step 4 — a RegExp cell is read through its
        // [[OriginalSource]] (`new RegExp(/b/, "y")` is `/b/y`; the
        // stringified `/b/` would match slashes). Step 5 — any other
        // regexp-like object answers `source` / `flags` by Get.
        let mut owned: [Option<Owned>; 2] = [None, None];
        if pattern_is_regexp && av::regexp_cell(pattern).is_none() {
            let Some(src) = get(pattern, b"source") else {
                return VALUE_UNDEFINED;
            };
            if is_undefined(flags) {
                let Some(f) = get(pattern, b"flags") else {
                    return VALUE_UNDEFINED;
                };
                owned[1] = Some(f);
            }
            owned[0] = Some(src);
        }
        let p = owned[0].as_ref().map_or(pattern, Owned::get);
        let f = owned[1].as_ref().map_or(flags, Owned::get);
        let re = box_void_ptr(__torajs_regex_compile_any(p, f));
        if __torajs_throw_check() != 0 {
            av::__torajs_anyv_rc_dec(re);
            return VALUE_UNDEFINED;
        }
        re
    }
}
