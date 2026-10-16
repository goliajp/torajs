//! The call-form bodies for the three families that are not pure
//! coercions — Symbol, BigInt and RegExp. Split from the parent under
//! the 500-line file rule; the parent's table names which family
//! answers what, these answer how.
//!
//! The seam is the parent's own: rows like Number and Boolean are one
//! expression each and read fine inline, while these three each run a
//! spec algorithm with its own throw shapes.

use super::*;

/// §20.4.1.1 — `Symbol(description)`. Step 2's ToString runs only
/// when the argument is present and not undefined; a Symbol
/// description is allowed to be absent, and "undefined" is a
/// different description from none.
pub(super) unsafe fn ctor_symbol_call(argc: i64, arg: u64) -> u64 {
    use crate::nanbox::{VALUE_UNDEFINED, box_void_ptr, is_undefined};
    unsafe {
        let desc = if argc == 0 || is_undefined(arg) {
            core::ptr::null_mut()
        } else {
            let s = crate::nanbox_ffi::__torajs_anyv_to_str(arg);
            if __torajs_throw_check() != 0 {
                return VALUE_UNDEFINED;
            }
            s
        };
        let sym = __torajs_symbol_alloc(desc);
        if !desc.is_null() {
            // `symbol_alloc` takes its own stake in the description.
            __torajs_str_drop(desc);
        }
        box_void_ptr(sym)
    }
}

/// §21.2.1.1 — `BigInt(value)`: ToPrimitive with the number hint,
/// then NumberToBigInt or ToBigInt. `any_to_bigint` is that whole
/// step, including the RangeError a non-integral Number raises.
pub(super) unsafe fn ctor_bigint_call(argc: i64, arg: u64) -> u64 {
    use crate::nanbox::{VALUE_UNDEFINED, box_void_ptr};
    if argc == 0 {
        unsafe {
            __torajs_throw_type_error(c"BigInt() requires an argument".as_ptr());
        }
        return VALUE_UNDEFINED;
    }
    match unsafe { crate::to_bigint::bigint_of_value(arg) } {
        Some(cell) => box_void_ptr(cell as *mut c_void),
        None => VALUE_UNDEFINED,
    }
}

/// §22.2.4.1 — `RegExp(pattern, flags)` with NewTarget undefined.
///
/// Step 1.b's identity shortcut is taken on the shape tr can see: a
/// RegExp cell with no flags argument answers itself. The spec also
/// reads `pattern.constructor` and compares it to %RegExp% first, so
/// a RegExp whose `constructor` was reassigned should be rebuilt
/// rather than returned — that read is not made here, and the
/// difference is observable only after such a reassignment.
pub(super) unsafe fn ctor_regexp_call(argv: *const u64, argc: i64) -> u64 {
    use crate::nanbox::{VALUE_UNDEFINED, is_cell, is_undefined};
    unsafe {
        let pattern = if argc > 0 {
            argv.read()
        } else {
            VALUE_UNDEFINED
        };
        let flags = if argc > 1 {
            argv.add(1).read()
        } else {
            VALUE_UNDEFINED
        };
        if is_undefined(flags)
            && is_cell(pattern)
            && (crate::nanbox::as_void_ptr(pattern).cast::<u8>().add(4) as *const u16).read()
                == torajs_rc::Tag::RegExp as u16
        {
            crate::nanbox_ffi::__torajs_anyv_rc_inc(pattern);
            return pattern;
        }
        crate::dispatch_seam::__torajs_ctor_regexp_build(pattern, flags)
    }
}
