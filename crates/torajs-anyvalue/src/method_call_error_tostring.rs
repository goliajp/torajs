//! `Error.prototype.toString` (§20.5.3.4) and the dispatcher the
//! typed tier calls into — split from
//! [`super::method_call_object_proto`], whose own subject is the
//! §20.1.4.3/.5 own-property probes and the §20.1.3.6 badge. Two
//! different questions on the same shelf: what does the base object
//! surface answer for ANY receiver, versus what does an Error
//! instance render as.

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::nanbox::{AnyValue, is_short_str};
use crate::nanbox_encode::__torajs_anyv_box_pointer;

unsafe extern "C" {
    /// torajs-meta — Error.prototype.toString (§20.5.3.4): render
    /// `name: message` from a FLAG_ERROR OBJ instance pointer.
    fn __torajs_error_to_string(p: *const u8) -> *mut u8;
    /// torajs-throw — pending-throw probe (override invoke abort).
    fn __torajs_throw_check() -> i64;
    /// torajs-throw — typed-tier non-string override boundary.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// `Error.prototype.toString` (§20.5.3.4) for a struct receiver: a
/// FLAG_ERROR OBJ answers `name: message` (via the torajs-meta helper,
/// the same one the SSA typed-tier lowering emits), boxed as an owned
/// Str. `None` when the struct is not Error-derived — the caller then
/// answers the "[object Object]" badge (ES §19.1.3.6). Ordered after
/// the own-property probe by its call site, so a monkey-patched own
/// `toString` still wins; a monkey-patched PROTOTYPE entry
/// (`Error.prototype.toString = f`) wins through the chain probe
/// below (rotation 141).
///
/// # Safety
/// `obj` is a live `Tag::Obj` struct cell.
pub(crate) unsafe fn error_struct_tostring(obj: *mut c_void) -> Option<AnyValue> {
    let flags = unsafe { (obj.cast::<u8>().add(6) as *const u16).read() };
    if flags & torajs_rc::FLAG_ERROR == 0 {
        return None;
    }
    if let Some(v) = unsafe { error_tostring_override(obj) } {
        return Some(v);
    }
    let s = unsafe { __torajs_error_to_string(obj.cast::<u8>()) };
    Some(unsafe { __torajs_anyv_box_pointer(s as *mut c_void) })
}

/// §20.5.3.4 monkey-patch probe for an error instance's `toString`
/// dispatch (test262 tostring-1/2): the class prototype chain's own
/// `toString` entry is the mid-156 builtin cell `__proto_Error`
/// installs — a user `Error.prototype.toString = f` overwrites that
/// dynobj entry, and `e.toString()` must run f. `None` = absent /
/// still the builtin (caller rides the fixed-offset fast lane);
/// `Some(v)` = an override was invoked (result rides as-is — its
/// pending throw, if any, stays recorded) or the stored entry was
/// not callable (the TypeError is recorded). The explicitly reified
/// ORIGINAL (`const orig = Error.prototype.toString; orig.call(e)`)
/// never routes here — the mid-156 dispatch arm keeps §20.5.3.4
/// itself. Args are not forwarded (the dispatch path drops argc for
/// the 0-arg builtin) — an override reading its arguments is a
/// recorded boundary, as is an accessor-shaped chain entry (both
/// answer the not-callable TypeError, loud).
///
/// # Safety
/// `obj` is a live FLAG_ERROR `Tag::Obj` struct cell.
pub(crate) unsafe fn error_tostring_override(obj: *mut c_void) -> Option<AnyValue> {
    let (tag, val) = unsafe { crate::struct_error_msg::error_proto_chain_pair(obj, b"toString") };
    if tag == torajs_rc::AnySlotTag::Undef as u64 {
        return None;
    }
    if tag != torajs_rc::AnySlotTag::Heap as u64
        || val == 0
        || unsafe { (val as *const u8).add(4).cast::<u16>().read() } != Tag::Closure as u16
    {
        return Some(unsafe { crate::method_call::not_callable() });
    }
    let cell = val as *mut c_void;
    let recv = unsafe {
        crate::nanbox_encode::__torajs_anyv_box_from_pair(
            torajs_rc::AnySlotTag::Heap as i64,
            obj as i64,
        )
    };
    if let Some(mid) = unsafe { crate::method_value::builtin_method_mid(cell) } {
        if mid == torajs_rc::ANY_METHOD_ERROR_TO_STRING {
            return None;
        }
        return Some(unsafe {
            crate::method_call::any_method_call_inner(
                recv,
                mid,
                core::ptr::null(),
                core::ptr::null_mut(),
                core::ptr::null(),
                0,
            )
        });
    }
    if let Some((env, entry)) = unsafe { crate::method_call::closure_cell_entry(cell) } {
        return Some(unsafe {
            crate::method_call::invoke_with_this(env, entry, recv, core::ptr::null(), 0)
        });
    }
    Some(unsafe { crate::method_call::not_callable() })
}

/// Typed-tier entry for `<error-instance>.toString()` — the SSA
/// lowering's Str-typed call site (rotation 141, replacing its
/// direct `__torajs_error_to_string` emit so a monkey-patched
/// `Error.prototype.toString` is honored on statically-typed
/// receivers too). No override → the fixed-offset formatter; an
/// override answering a string unwraps to an owned Str cell; a
/// non-string override answer is a recorded typed-tier boundary
/// (loud TypeError — the slot is statically `Str`, silently
/// reinterpreting would be worse). NULL = pending throw recorded
/// (the lowering's throw-check diverts).
///
/// # Safety
/// `p` points to a live FLAG_ERROR `Tag::Obj` heap instance.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_error_tostring_dispatch(p: *const u8) -> *mut u8 {
    let Some(av) = (unsafe { error_tostring_override(p as *mut c_void) }) else {
        return unsafe { __torajs_error_to_string(p) };
    };
    if unsafe { __torajs_throw_check() } != 0 {
        unsafe { crate::nanbox_ffi::__torajs_anyv_rc_dec(av) };
        return core::ptr::null_mut();
    }
    // Heap Str cell — hand the invoke's +1 through as-is.
    if crate::nanbox::is_cell(av)
        && unsafe { (av as *const u8).add(4).cast::<u16>().read() } == Tag::Str as u16
    {
        return av as *mut u8;
    }
    // ShortStr immediate — materialize (string→string, no coercion).
    if is_short_str(av) {
        return unsafe { crate::nanbox_ffi::__torajs_anyv_to_str(av) } as *mut u8;
    }
    unsafe {
        crate::nanbox_ffi::__torajs_anyv_rc_dec(av);
        __torajs_throw_type_error(
            c"not yet supported: Error toString override returned a non-string on a typed receiver"
                .as_ptr(),
        );
    }
    core::ptr::null_mut()
}
