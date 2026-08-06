//! §23.1.3.36 `Array.prototype.toString` — steps 2-4.
//!
//! ```text
//! 2. Let func be ? Get(array, "join").
//! 3. If IsCallable(func) is false, set func to %Object.prototype.toString%.
//! 4. Return ? Call(func, array).
//! ```
//!
//! The join is a LOOKUP on the receiver, not a fixed call into the
//! join kernel, and the difference is observable four ways: a
//! patched `Array.prototype.join`, an own `a.join`, a subclass
//! `join`, and a non-callable one — that last landing on the
//! `[object Array]` badge rather than on a comma-joined list. All
//! four reach here through `String(a)` and `a + ""` as well, because
//! ToPrimitive resolves `toString` through the same dispatcher.
//!
//! The unpatched program does not pay for any of it: two relaxed
//! bitmap loads and a NULL props slot answer "nothing shadows the
//! join" and the caller runs the kernel directly.

use core::ffi::c_void;

use torajs_rc::{ANY_METHOD_JOIN, FLAG_SUBCLASSED};

use crate::method_call::{closure_cell_entry, invoke_boxed, invoke_with_this};
use crate::method_value::builtin_method_mid;
use crate::nanbox::{AnyValue, as_void_ptr, is_cell};

unsafe extern "C" {
    /// torajs-arr — own-key membership on the side-props table.
    fn __torajs_arrprops_has(arr: *mut c_void, key: *const c_void) -> i32;
    /// Universal NaN-box-safe heap dropper.
    fn __torajs_value_drop_heap(p: *mut c_void);
}

/// `torajs-rc/builtin_proto.rs` — `ARRAY_PROTO_TAG`.
const ARRAY_PROTO_TAG: i64 = 2;

/// The `"join"` key as a pooled Str cell — the probes and the member
/// read all take a Str, and a known-mid arm carries no name bytes.
/// Caller drops it.
unsafe fn join_key() -> *mut u8 {
    unsafe {
        let bytes = b"join";
        let s = crate::__torajs_str_alloc_pooled(bytes.len() as u64);
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), s.add(16), bytes.len());
        s
    }
}

/// Does anything shadow `Array.prototype`'s own join for this
/// receiver — a patch or delete on the prototype, an own entry, or a
/// subclass method? `false` is the caller's fast path: the kernel IS
/// what step 2 would have resolved, so calling it directly is the
/// same program.
///
/// # Safety
/// `arr` is a live array heap block.
unsafe fn join_is_shadowed(arr: *mut c_void) -> bool {
    unsafe {
        if torajs_rc::builtin_proto::__torajs_builtin_proto_is_shadowed(
            ARRAY_PROTO_TAG,
            ANY_METHOD_JOIN,
        ) != 0
        {
            return true;
        }
        if (arr.cast::<u8>().add(6) as *const u16).read() & FLAG_SUBCLASSED != 0 {
            return true;
        }
        let key = join_key();
        let owns = __torajs_arrprops_has(arr, key as *const c_void) != 0;
        crate::__torajs_str_drop(key as *mut c_void);
        owns
    }
}

/// Steps 2-4 when something shadows the kernel; `None` when nothing
/// does and the caller should run its own join.
///
/// # Safety
/// `arr` is a live array heap block; `recv` is that same cell boxed.
pub(crate) unsafe fn spec_to_string(arr: *mut c_void, recv: AnyValue) -> Option<AnyValue> {
    unsafe {
        if !join_is_shadowed(arr) {
            return None;
        }
        let key = join_key();
        // A subclass `join` sits between the receiver's own face and
        // `Array.prototype`, so it is resolved where the spec chain
        // puts it — before the member read, which starts at the own
        // face and continues to the prototype.
        if (arr.cast::<u8>().add(6) as *const u16).read() & FLAG_SUBCLASSED != 0
            && let Some(adapter) =
                crate::method_call_subclass::subclass_adapter(arr, key as *const u8)
        {
            crate::__torajs_str_drop(key as *mut c_void);
            return Some(invoke_boxed(arr, adapter as u64, core::ptr::null(), 0));
        }
        let func = crate::arr_member_value::__torajs_arr_member_value(arr, key as *const c_void);
        crate::__torajs_str_drop(key as *mut c_void);
        let cell = if is_cell(func) {
            as_void_ptr(func)
        } else {
            core::ptr::null_mut()
        };
        // What the entry names is the kernel itself — a `join` put
        // back after a delete, or reached through a patch on some
        // other method. Run it directly instead of re-entering the
        // dispatcher under the same mid.
        if !cell.is_null() && builtin_method_mid(cell) == Some(ANY_METHOD_JOIN) {
            __torajs_value_drop_heap(cell);
            return None;
        }
        if !cell.is_null()
            && let Some((env, entry)) = closure_cell_entry(cell)
        {
            let out = invoke_with_this(env, entry, recv, core::ptr::null(), 0);
            __torajs_value_drop_heap(cell);
            return Some(out);
        }
        // Step 3 — not callable, so the call is
        // %Object.prototype.toString% and the answer is the badge.
        if !cell.is_null() {
            __torajs_value_drop_heap(cell);
        }
        Some(crate::method_call_object_proto::object_proto_to_string(
            recv,
        ))
    }
}
