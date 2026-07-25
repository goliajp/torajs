//! The ES §25.5.2.1 `space` argument — normalizing it into a gap and
//! spending that gap as indentation. Split out of the parent under
//! the 500-line file discipline; a child module reaches its parent's
//! private items, so the walk body it delegates to stays private.

use core::ffi::c_void;

use super::*;

/// `JSON.stringify(value, replacer, space)` under an already
/// normalized gap. `depth` is the nesting level the value sits at,
/// so an any-typed member of a statically unfolded composite keeps
/// indenting from its parent's level instead of restarting at zero.
///
/// # Safety
/// `v` carries a valid AnyValue bit pattern; `gap` is a live Str
/// block (or NULL for no indent).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_json_stringify_gap(
    v: AnyValue,
    gap: *const u8,
    depth: i64,
) -> *mut u8 {
    unsafe {
        let bytes = if gap.is_null() {
            &[][..]
        } else {
            let len = (gap.add(STR_LEN_OFF) as *const u32).read() as usize;
            core::slice::from_raw_parts(gap.add(STR_DATA_OFF), len)
        };
        stringify_with_gap_at(v, bytes, depth.max(0) as u32)
    }
}

/// ES §25.5.2.1 steps 5-8 — normalize a `space` argument into a gap,
/// handed back as a Str cell. A Number (or Number object) becomes
/// `min(10, ToIntegerOrInfinity(space))` spaces, a String (or String
/// object) its first 10 code units, anything else the empty gap. The
/// static unfold cannot take a Rust slice, so it asks for the gap
/// once at the call site and threads that cell through its own
/// recursion.
/// Answers a fresh refcount=1 Str the caller drops (empty for "no
/// indent", which the caller's own compile-time gate makes
/// unreachable in practice).
///
/// # Safety
/// `space` carries a valid AnyValue bit pattern.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_json_gap_str(space: AnyValue) -> *mut u8 {
    unsafe {
        let gap = gap_of(space);
        __torajs_str_alloc(gap.as_ptr(), gap.len() as i64)
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
