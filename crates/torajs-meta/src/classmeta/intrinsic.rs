//! Which registered classes stand in for a spec intrinsic, and the
//! §10.1.13 default prototype that follows from it (RFC
//! 20260920-struct-user-proto K3).
//!
//! `DisposableStack`, `AggregateError`, `TypeError` … are classes the
//! compiler injects; at runtime they have exactly the shape of a user
//! class. GetPrototypeFromConstructor needs the difference: when
//! `newTarget.prototype` is not an object, the object gets the
//! prototype of the intrinsic that created it — the nearest injected
//! class on the constructor's chain — or `%Object.prototype%` when the
//! chain is all user classes.

use core::ffi::c_void;

use super::{CLASSES_BY_TAG_IMM, MAX_CLASSES, in_range};

// Same shape and posture as CLASS_SOURCE_BY_TAG (source.rs).
static mut INTRINSIC_BY_TAG: [bool; MAX_CLASSES] = [false; MAX_CLASSES];

/// `%Function.prototype%`'s builtin-proto tag (torajs-rc
/// `builtin_proto::FUNCTION_PROTO_TAG`).
const FUNCTION_PROTO_TAG: i64 = 13;

/// Chain hops before the walk gives up — a class chain is acyclic by
/// construction, this only bounds a re-parented class object.
const MAX_HOPS: usize = 64;

unsafe extern "C" {
    fn __torajs_builtin_proto_tag_of(p: *const c_void) -> i64;
    fn __torajs_get_builtin_prototype(tag: i64) -> *mut c_void;
    fn __torajs_rc_dec(p: *mut c_void);
}

/// Record that class `tag` is an injected intrinsic. Emitted right
/// after its `class_register` for the classes in
/// `Ast::intrinsic_classes`.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_class_mark_intrinsic(tag: i64) {
    if !in_range(tag) {
        return;
    }
    // SAFETY: single-threaded JS runtime, direct indexed write.
    unsafe { INTRINSIC_BY_TAG[tag as usize] = true };
}

/// Is `cell` the class object of an INJECTED class? The compiler
/// injects exactly the §20.5 Error hierarchy, and every member of it
/// is callable as a function with the same behaviour as `new`
/// (§20.5.1.1, §20.5.6.2, §20.5.7.1) — which is the question the
/// any-lane [[Call]] asks here. A user class answers 0 and gets
/// §10.2.1 step 2's refusal instead.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_class_is_intrinsic_cell(cell: *const c_void) -> i32 {
    if cell.is_null() {
        return 0;
    }
    let tag = tag_of_class_cell(cell);
    if tag < 0 {
        return 0;
    }
    // SAFETY: single-threaded JS runtime, direct indexed read of an
    // in-range tag.
    i32::from(unsafe { INTRINSIC_BY_TAG[tag as usize] })
}

/// The tag whose REGISTERED class object is `cell` — one evaluation of
/// a unit class answers its template's — or -1.
pub(super) fn tag_of_class_cell(cell: *const c_void) -> i64 {
    let mut tag = 0usize;
    while tag < MAX_CLASSES {
        // SAFETY: single-threaded JS runtime, direct indexed reads.
        if unsafe { CLASSES_BY_TAG_IMM[tag] } as *const c_void == cell {
            return tag as i64;
        }
        tag += 1;
    }
    unsafe { super::unit::__torajs_class_unit_tag_of(cell) }
}

/// §10.1.13 step 4's fallback for constructor `ctor`, BORROWED: the
/// prototype of the nearest injected intrinsic on its class chain, or
/// `%Object.prototype%` when the chain reaches `%Function.prototype%`
/// (all user classes). 0 when `ctor` is no registered class, or its
/// chain leaves the classes for something else (`extends Map`, a
/// plain function parent): the object the target built already has
/// the right intrinsic prototype, and the caller keeps it.
///
/// This is a by-tag registry READER for the link-time class register
/// judgment, like `__torajs_class_source_for_cell`.
///
/// # Safety
/// `ctor` is any AnyValue word.
#[unsafe(no_mangle)]
#[inline(never)]
pub unsafe extern "C" fn __torajs_class_default_proto(ctor: u64) -> u64 {
    let mut cur = ctor;
    for _ in 0..MAX_HOPS {
        if !super::is_cell_imm(cur) {
            return 0;
        }
        let cell = cur as *const c_void;
        let tag = tag_of_class_cell(cell);
        if tag < 0 {
            if cur == ctor || unsafe { __torajs_builtin_proto_tag_of(cell) } != FUNCTION_PROTO_TAG {
                return 0;
            }
            let op = unsafe { __torajs_get_builtin_prototype(crate::reflect::OBJECT_PROTO_TAG) };
            return op as u64;
        }
        // SAFETY: `tag` came from the table scan, so it is in range.
        if unsafe { INTRINSIC_BY_TAG[tag as usize] } {
            return super::proto_anyv_borrowed(tag);
        }
        // The class object's [[Prototype]] is its parent class, or
        // %Function.prototype% for a base class.
        let parent = unsafe { crate::reflect_proto::__torajs_anyv_get_proto_of_any(cur) };
        if super::is_cell_imm(parent) {
            // The link keeps the parent alive; the walk only compares.
            unsafe { __torajs_rc_dec(parent as *mut c_void) };
        }
        cur = parent;
    }
    0
}
