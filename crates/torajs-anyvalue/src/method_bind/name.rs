//! A bound function's `name` (§20.2.3.2 steps 12-14): resolved when the
//! cell is minted, and spelled out when it is read, so a chain of binds
//! over an unrenamed bound target shares one name cell.

use core::ffi::c_void;

use super::*;

/// §20.2.3.2 steps 8-14 — `"bound "` prepended to `Get(Target,
/// "name")`, a non-String target name counting as `""`. Answers an
/// owned Str cell and the count of further `bound ` prefixes it is
/// read with (see [`BOUND_NAME_PREFIXES_OFF`]).
///
/// # Safety
/// As [`bind_time_length`].
pub(super) unsafe fn bind_time_name(kind: u64, target: u64) -> (*mut u8, u64) {
    unsafe {
        // A bound target whose `name` reads its own minted slot runs
        // no user code on step 13's Get, so sharing the slot is the
        // same answer without the copy.
        if kind == 1
            && let Some(shared) = shared_bound_name(target)
        {
            return shared;
        }
        let tname: *mut u8 = if kind == 0 {
            let (mid, _) = unpack_builtin_target(target);
            let name = torajs_rc::any_method_meta(mid).map_or("", |(n, _)| n);
            __torajs_str_alloc(name.as_ptr(), name.len() as i64)
        } else {
            crate::name_get::bound_target_name_str(target)
        };
        let prefix = __torajs_str_alloc(c"bound ".as_ptr() as *const u8, 6);
        let joined = crate::__torajs_str_concat(prefix as *const u8, tname as *const u8);
        __torajs_str_drop(prefix as *mut c_void);
        // Fresh target names drop; an interned method-name cell
        // no-ops on the static flag.
        __torajs_str_drop(tname as *mut c_void);
        (joined as *mut u8, 0)
    }
}

/// `(name cell +1, prefixes)` for a new cell bound over `target`,
/// when `target` is a bound cell whose `Get(target, "name")` is its
/// minted name: no own-property bag (no define / expando shadow) and
/// no deleted-name tombstone.
///
/// # Safety
/// `target` is a live kind-1 target pointer.
unsafe fn shared_bound_name(target: u64) -> Option<(*mut u8, u64)> {
    unsafe {
        let cell = bound_target_cell(target)?;
        let name = *(cell.add(BOUND_NAME_OFF) as *const u64) as *mut u8;
        if name.is_null()
            || *(cell.add(CLOSURE_PROPS_OFF) as *const u64) != 0
            || crate::member_get::header_flag(
                cell as *const c_void,
                torajs_rc::FLAG_FN_NAME_DELETED,
            )
        {
            return None;
        }
        torajs_rc::__torajs_rc_inc(name as *mut c_void);
        let prefixes = *(cell.add(BOUND_NAME_PREFIXES_OFF) as *const u64);
        Some((name, prefixes + 1))
    }
}

/// # Safety
/// As [`bound_cell_length`]. The answer is a fresh stake.
pub(crate) unsafe fn bound_cell_name(ptr: *mut c_void) -> *mut u8 {
    unsafe {
        let s = *((ptr as *const u8).add(BOUND_NAME_OFF) as *const u64) as *mut u8;
        let prefixes = *((ptr as *const u8).add(BOUND_NAME_PREFIXES_OFF) as *const u64) as usize;
        if prefixes == 0 {
            torajs_rc::__torajs_rc_inc(s as *mut c_void);
            return s;
        }
        let bytes = prefixes * 6;
        if bytes > STR_MAX_LEN {
            return __torajs_str_too_long();
        }
        let head = crate::__torajs_str_alloc_pooled(bytes as u64);
        for i in 0..prefixes {
            core::ptr::copy_nonoverlapping(b"bound ".as_ptr(), head.add(STR_HDR_SIZE + i * 6), 6);
        }
        let joined = crate::__torajs_str_concat(head as *const u8, s as *const u8);
        __torajs_str_drop(head as *mut c_void);
        joined as *mut u8
    }
}
