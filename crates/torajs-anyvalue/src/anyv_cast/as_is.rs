//! The cast into an object slot whose readers dispatch on the cell
//! itself, so a value crosses as it is or not at all: the slot of an
//! interface whose members dispatch through witness rows, and of a
//! class rooted in an exotic builtin. The lowering writes the plan
//! (`ssa_lower_checked_cast::plan::uncast`):
//!
//! ```text
//! plan  := u32 lists_off, [OPT] node (at offset 4), lists
//! node  := u8 kind, u32 tag, u32 class list index, by kind:
//!   SUB  a cell of heap tag `tag` minted for a class of the list
//!        (`class MP extends Map`: its instances are Map cells, told
//!        apart from the builtin's own by `torajs-meta`'s record)
//!   WIT  a `Tag::Obj` cell of the layout of class tag `tag`, or an
//!        instance of a class of the list (the classes whose rows
//!        carry the interface's witnesses)
//! lists := as `super::plan`'s
//! ```
//!
//! Under OPT `undefined` crosses as the generic undefined cell and
//! `null` as NULL. Anything else records the TypeError naming the slot.
//! Kept apart from `super::obj`, so a program with no such slot links
//! none of it.

use core::ffi::c_void;

use torajs_rc::Tag;
use torajs_rc::undef_cell::undef_cell_ptr;

use super::plan::{OPT, Plan};
use super::{cell_tag, reject};
use crate::arg_struct_coerce::{__torajs_anyv_struct_fits, OBJ_CLASS_TAG_OFF, kin_admits};
use crate::nanbox::{AnyValue, as_void_ptr, is_cell, is_null, is_undefined};

pub(super) const SUB: u8 = 12;
pub(super) const WIT: u8 = 13;
/// The header's flags word (`torajs_rc::FLAG_SUBCLASSED` among them).
const CELL_FLAGS_OFF: usize = 6;

unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
}

#[cfg(not(test))]
unsafe extern "C" {
    /// torajs-meta: the class tag an exotic subclass instance was
    /// minted for, -1 for a cell with no record.
    fn __torajs_subclass_class_tag(cell: *const c_void) -> i64;
}

// Cargo-test stand-in: torajs-meta is not linked into this crate's
// tests; every cell flagged as a subclass instance answers this tag.
#[cfg(test)]
const TEST_SUBCLASS_TAG: i64 = 7;
#[cfg(test)]
unsafe fn __torajs_subclass_class_tag(_cell: *const c_void) -> i64 {
    TEST_SUBCLASS_TAG
}

/// Cast `v` into the slot `plan` describes: the cell with a stake of
/// its own, the generic undefined cell or NULL for a nullish value the
/// slot admits, or NULL under the TypeError naming `site`.
///
/// # Safety
/// `plan` is such a plan; a cell payload of `v` is live; `site` is a
/// live Str literal block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_cast_obj_as_is(
    v: AnyValue,
    plan: *const u8,
    _vtables: *const *mut c_void,
    site: *const u8,
) -> *mut c_void {
    let plan = Plan::<false>::new(plan, core::ptr::null());
    match unsafe { as_is(&plan, 4, v) } {
        Some(bits) => bits as *mut c_void,
        None => {
            unsafe { reject(v, site) };
            core::ptr::null_mut()
        }
    }
}

/// The slot bits of `v` by the node at `at`, with a stake of their own;
/// `None` for a value the slot does not take.
///
/// # Safety
/// `at` is a node of `plan`; a cell payload of `v` is live.
unsafe fn as_is(plan: &Plan, at: usize, v: AnyValue) -> Option<u64> {
    let kind = plan.u8(at);
    if kind == OPT && is_undefined(v) {
        return Some(undef_cell_ptr() as u64);
    }
    if kind == OPT && is_null(v) {
        return Some(0);
    }
    if kind == OPT {
        return unsafe { as_is(plan, at + 1, v) };
    }
    if !is_cell(v) {
        return None;
    }
    let tag = plan.u32(at + 1);
    let kin = plan.class_list(plan.u32(at + 5));
    debug_assert!(kind == SUB || kind == WIT);
    let passes = match kind {
        SUB => unsafe { minted_for(as_void_ptr(v), tag, kin) },
        _ => unsafe { witness(v, tag, kin) },
    };
    passes.then(|| {
        unsafe { __torajs_rc_inc(as_void_ptr(v)) };
        v
    })
}

/// Whether the cell `p` wears the heap tag `heap_tag` and was minted
/// for a class of the list `kin`: a plain cell of the builtin, or one
/// minted for another class, has none of the slot class's methods.
///
/// # Safety
/// `p` is a live heap cell; `kin` is a class list.
unsafe fn minted_for(p: *mut c_void, heap_tag: u32, kin: *const u8) -> bool {
    let hdr = p.cast::<u8>();
    let tag = unsafe { hdr.add(4).cast::<u16>().read() };
    let flags = unsafe { hdr.add(CELL_FLAGS_OFF).cast::<u16>().read() };
    if u32::from(tag) != heap_tag || flags & torajs_rc::FLAG_SUBCLASSED == 0 {
        return false;
    }
    let class = unsafe { __torajs_subclass_class_tag(p) };
    u32::try_from(class).is_ok_and(|c| unsafe { kin_admits(kin, c) })
}

/// Whether `v` is a `Tag::Obj` cell of the layout of `class_tag`, or an
/// instance of a class of the list `kin`, whatever its layout.
///
/// # Safety
/// `v` is a live heap cell; `kin` is a class list.
unsafe fn witness(v: AnyValue, class_tag: u32, kin: *const u8) -> bool {
    if unsafe { cell_tag(v) } != Tag::Obj as u16 {
        return false;
    }
    let p = as_void_ptr(v).cast::<u8>();
    let tag = unsafe { p.add(OBJ_CLASS_TAG_OFF).cast::<u32>().read() };
    (unsafe { __torajs_anyv_struct_fits(v, class_tag, core::ptr::null()) } == 1)
        || unsafe { kin_admits(kin, tag) }
}

#[cfg(test)]
mod tests;
