//! The ES §25.5.2.1 `space` argument — normalizing it into a gap and
//! spending that gap as indentation. Split out of the parent under
//! the 500-line file discipline; a child module reaches its parent's
//! private items, so the walk body it delegates to stays private.

use core::ffi::c_void;

use super::*;

/// ES §25.5.2.1 — under a non-empty `gap`, every element / property
/// sits on its own line indented by one `gap` per nesting level, and
/// the closing bracket returns to the parent's level. A no-op for

/// `JSON.stringify(value, replacer, space)` with a `space` argument.
/// ES §25.5.2.1 steps 5-8 normalize it: a Number (or Number object)
/// becomes `min(10, ToIntegerOrInfinity(space))` spaces, a String (or
/// String object) its first 10 code units, anything else no indent at
/// all — in which case the output is byte-identical to the compact
/// entry.
///
/// # Safety
/// `v` and `space` carry valid AnyValue bit patterns.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_json_stringify_spaced(
    v: AnyValue,
    space: AnyValue,
) -> *mut u8 {
    unsafe {
        let gap = gap_of(space);
        stringify_with_gap(v, &gap)
    }
}

/// The §25.5.2.1 step 5-8 normalization itself.
unsafe fn gap_of(space: AnyValue) -> Vec<u8> {
    unsafe {
        // Step 5 unwraps a Number / String wrapper before the split.
        let space = if is_cell(space) {
            let ptr = as_void_ptr(space);
            let tag = (ptr.cast::<u8>().add(4) as *const u16).read();
            if tag == Tag::NumberWrapper as u16 {
                box_double(((ptr as *const u8).add(8) as *const f64).read())
            } else if tag == Tag::StringWrapper as u16 {
                let inner = ((ptr as *const u8).add(8) as *const *const c_void).read();
                if inner.is_null() {
                    return Vec::new();
                }
                box_void_ptr(inner as *mut c_void)
            } else {
                space
            }
        } else {
            space
        };
        if is_int32(space) {
            let n = as_int32(space).clamp(0, 10) as usize;
            return vec![b' '; n];
        }
        if is_double(space) {
            let d = as_double(space);
            // ToIntegerOrInfinity truncates toward zero; NaN is 0.
            let n = if d.is_nan() { 0.0 } else { d.trunc() };
            let n = n.clamp(0.0, 10.0) as usize;
            return vec![b' '; n];
        }
        // Step 7 — a string gap keeps its first 10 code units. The
        // payload walk below counts UTF-8 code points for a Latin-1
        // cell, which is what this runtime's Str lane stores; a
        // wider string simply keeps its whole prefix under the cap.
        if is_short_str(space)
            || (is_cell(space) && {
                let ptr = as_void_ptr(space);
                (ptr.cast::<u8>().add(4) as *const u16).read() == Tag::Str as u16
            })
        {
            let cell = crate::nanbox_ffi::__torajs_anyv_to_str(space);
            let len = (cell.cast::<u8>().add(STR_LEN_OFF) as *const u32).read() as usize;
            let take = len.min(10);
            let bytes = core::slice::from_raw_parts(cell.cast::<u8>().add(STR_DATA_OFF), take);
            let out = bytes.to_vec();
            __torajs_str_drop(cell);
            return out;
        }
        Vec::new()
    }
}
