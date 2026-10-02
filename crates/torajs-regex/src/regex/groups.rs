//! The `groups` object of a match — §22.2.7.8 steps 25-27: one
//! property per named capture, holding what the group matched.

use alloc::vec::Vec;
use core::ffi::c_void;

use super::str_helpers::subject::Subject;
use super::{
    __torajs_dynobj_alloc, __torajs_dynobj_mark_null_proto, __torajs_dynobj_set, __torajs_str_drop,
    ANY_HEAP, ANY_UNDEF, RegExp, str_from_bytes,
};
use crate::node::REGEX_MAX_CAPTURES;
use crate::vm::save_slot;

/// Build the dict from the named captures recorded on `re` and the
/// just-finished match's saves[]; `None` for a regex that names no
/// capture, whose `groups` is `undefined`. The caller owns the dict.
///
/// # Safety
///
/// Calls cross-tier extern allocators. `re` and `subject` must
/// outlive the call.
pub unsafe fn build_groups(re: &RegExp, subject: &Subject, saves: &[i64]) -> Option<*mut c_void> {
    if re.n_named_captures == 0 || re.capture_names.is_empty() {
        return None;
    }
    // Spec: the groups object is created with a null prototype
    // (OrdinaryObjectCreate(null)); the flag drives the
    // `[Object: null prototype] ` print prefix.
    let mut groups = unsafe { __torajs_dynobj_alloc() };
    unsafe { __torajs_dynobj_mark_null_proto(groups) };
    let n_cap_lim = (re.n_captures as usize).min(REGEX_MAX_CAPTURES - 1);
    // Duplicate named groups (`(?:(?<z>c)|(?<z>d))`) — §22.2.7.8:
    // the groups object carries the PARTICIPATING twin's value, but
    // key order follows the FIRST occurrence in source order. So a
    // non-participating slot still writes its undefined placeholder
    // (a later participating twin's dynobj set updates in place,
    // keeping the slot), unless a participating twin already wrote
    // a defined value — that must not be clobbered back.
    let mut defined_names: Vec<&[u8]> = Vec::new();
    for i in 1..=n_cap_lim {
        let name = match re.capture_names.get(i) {
            Some(n) if !n.is_empty() => n,
            _ => continue,
        };
        let name_key = unsafe { str_from_bytes(name) };
        let gs = save_slot(saves, 2 * i);
        let ge = save_slot(saves, 2 * i + 1);
        if gs < 0 || ge < 0 {
            if defined_names.contains(&name.as_slice()) {
                unsafe { __torajs_str_drop(name_key as *mut c_void) };
                continue;
            }
            // Non-participating named group → undefined.
            unsafe {
                __torajs_dynobj_set(&mut groups, name_key as *mut c_void, ANY_UNDEF, 0);
            }
        } else {
            defined_names.push(name.as_slice());
            let val_str = unsafe { subject.span(gs, ge) };
            unsafe {
                __torajs_dynobj_set(
                    &mut groups,
                    name_key as *mut c_void,
                    ANY_HEAP,
                    val_str as u64,
                );
            }
        }
        unsafe { __torajs_str_drop(name_key as *mut c_void) };
    }
    Some(groups)
}
