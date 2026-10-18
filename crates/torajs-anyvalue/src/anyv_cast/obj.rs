//! The checked cast into an object-layout slot (`Obj(sid)`), or into
//! an array of them (RFC 20260922-any-to-typed-checked-cast §4.1, the
//! `Obj(sid)` row, and §4.4 item 3).
//!
//! What the slot takes follows its static type, which the lowering
//! hands over as a cast plan (`super::plan`):
//!
//! - a cell of the slot's layout passes with +1 and keeps its
//!   identity. For a class slot that is an instance of the class or of
//!   a subclass (its class list): an instance of another class would
//!   run this one's methods, so it is rejected. Another layout takes
//!   any cell with its fields ([`__torajs_anyv_struct_fits`]);
//! - an object that is not such a cell — a dynamic object (an object
//!   literal through `any`, a `JSON.parse` result), the struct cell of
//!   an object literal, or for a slot that is not a class's, any cell
//!   of another layout — is checked against
//!   the type and copied into a fresh cell of the layout (for a class
//!   slot, an instance of the class: its tag and vtable, so `instanceof`
//!   holds and the class's methods run on it). Every field the layout
//!   declares is read with [[Get]] (§7.3.2: an accessor runs its
//!   getter, an inherited property counts) and cast by its own type,
//!   recursively; only a missing field or one of another type rejects.
//!   Other properties, their order, the object's prototype, and whether
//!   something else holds it do not matter. The copy is a different
//!   object from the value: a write through one name is not seen
//!   through the other, the price §4.4 accepts for a structural type
//!   the layout represents by value;
//! - a slot whose annotation admits `undefined` / `null` takes them as
//!   the generic undefined cell and NULL;
//! - anything else records a TypeError naming the slot.

use core::ffi::c_void;

use torajs_rc::Tag;

use super::plan::{Fail, Field, MEMBER, OBJ, Plan};
use super::{cell_tag, reject};
use crate::arg_struct_coerce::{
    __torajs_anyv_struct_fits, OBJ_CLASS_TAG_OFF, OBJ_HEADER_SIZE, OBJ_TAG_OFF,
};
use crate::nanbox::{AnyValue, as_void_ptr, is_cell};

/// OBJ node flags: the slot names a class; the node copies nothing
/// (a field of the layout is of a type the plan cannot judge); the
/// node lists its class's prototype-chain keys after its fields.
const CLASS: u8 = 1;
const FITS_ONLY: u8 = 2;
pub(super) const PROTO_KEYS: u8 = 4;
/// The OBJ node has no class list.
const NO_LIST: u32 = u32::MAX;
const OBJ_VTABLE_OFF: usize = 16;

unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
    pub(super) fn __torajs_str_alloc(p: *const u8, len: i64) -> *mut u8;
    pub(super) fn __torajs_str_drop(p: *mut c_void);
    pub(super) fn __torajs_throw_check() -> i64;
    fn __torajs_struct_layout_lookup(class_tag: u32) -> *const c_void;
}

#[cfg(not(test))]
unsafe extern "C" {
    /// torajs-mmalloc zeroed sized alloc, the allocator the lowering's
    /// `__torajs_obj_alloc` uses, so the universal drop frees it.
    #[link_name = "__torajs_calloc"]
    fn calloc(size: usize) -> *mut u8;
}

// Cargo-test stand-in: the allocator symbol is linked into user
// binaries only, and no test builds a cell.
#[cfg(test)]
unsafe fn calloc(_size: usize) -> *mut u8 {
    unreachable!()
}

/// Cast `v` into the slot `plan` describes: the cell or array with a
/// stake of its own, NULL for a `null` a nullish slot admits, or NULL
/// under a pending throw — the TypeError naming `site` when `v` does
/// not fit, or what a getter run on the way threw. `vtables` holds the
/// vtable address of each object node of the plan.
///
/// # Safety
/// `plan` is a cast plan; `vtables` has an entry per object node; a
/// cell payload of `v` is live; `site` is a live Str literal block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_cast_obj(
    v: AnyValue,
    plan: *const u8,
    vtables: *const *mut c_void,
    site: *const u8,
) -> *mut c_void {
    unsafe { run(Plan::<false>::new(plan, vtables), v, site) }
}

/// The cast `plan` makes of `v`, rejecting a mismatch naming `site`.
///
/// # Safety
/// As [`__torajs_anyv_cast_obj`].
#[inline(always)]
pub(super) unsafe fn run<const OWN: bool>(
    plan: Plan<OWN>,
    v: AnyValue,
    site: *const u8,
) -> *mut c_void {
    match unsafe { plan.cast(4, v) } {
        Ok(bits) => bits as *mut c_void,
        Err(Fail::Mismatch) => {
            unsafe { reject(v, site) };
            core::ptr::null_mut()
        }
        Err(Fail::Thrown) => core::ptr::null_mut(),
    }
}

impl<const OWN: bool> Plan<OWN> {
    /// An object node (`at`): the cell itself when it is one of the
    /// slot's layout, else a fresh cell copied field by field.
    ///
    /// # Safety
    /// `at` is an OBJ node; a cell payload of `v` is live.
    pub(super) unsafe fn obj(&self, at: usize, v: AnyValue) -> Result<u64, Fail> {
        debug_assert_eq!(self.u8(at), OBJ);
        if !is_cell(v) {
            return Err(Fail::Mismatch);
        }
        let flags = self.u8(at + 1);
        let class = flags & CLASS != 0;
        let class_tag = self.u32(at + 2);
        let list = self.u32(at + 6);
        let kin = match list {
            NO_LIST => core::ptr::null(),
            idx => self.class_list(idx),
        };
        match unsafe { cell_tag(v) } {
            t if t == Tag::Obj as u16 => {
                if unsafe { __torajs_anyv_struct_fits(v, class_tag, kin) } == 1 {
                    unsafe { __torajs_rc_inc(as_void_ptr(v)) };
                    return Ok(v);
                }
                // an instance of another class would run this one's
                // methods; a struct cell of an object literal is no
                // instance of anything, and is copied like a dynamic
                // object
                if class && unsafe { class_instance(as_void_ptr(v)) } {
                    return Err(Fail::Mismatch);
                }
            }
            t if t == Tag::DynObj as u16 => {}
            _ => return Err(Fail::Mismatch),
        }
        if flags & FITS_ONLY != 0 {
            return Err(Fail::Mismatch);
        }
        if let Some(copy) = self.copies.copy_of(v, at) {
            return Ok(copy);
        }
        // a property of its own under a method or accessor name would
        // be lost to the class's member in the copy; a plan listing
        // keys always comes through an own entry
        if OWN && flags & PROTO_KEYS != 0 && unsafe { super::own::owns_key(self, v, at) } {
            return Err(Fail::Mismatch);
        }
        let n = self.u16(at + 14) as usize;
        // the copy exists before its fields are read, so a field that
        // reaches `v` again answers it
        let cell = unsafe { calloc(OBJ_HEADER_SIZE + n * 8) };
        unsafe {
            *(cell as *mut u32) = 1;
            *(cell.add(OBJ_TAG_OFF) as *mut u16) = Tag::Obj as u16;
            *(cell.add(OBJ_CLASS_TAG_OFF) as *mut u32) = class_tag;
            let vt = if self.vtables.is_null() {
                core::ptr::null_mut()
            } else {
                *self.vtables.add(self.u32(at + 10) as usize)
            };
            *(cell.add(OBJ_VTABLE_OFF) as *mut *mut c_void) = vt;
        }
        self.copies.note_copy(v, at, cell as u64);
        let mut field = at + 16;
        for _ in 0..n {
            let end = self.u32(field) as usize;
            let off = self.u32(field + 4) as usize;
            let len = self.u16(field + 8) as usize;
            let node = field + 10 + len;
            let got = unsafe { self.get(v, field + 10, len) };
            let cast = got.and_then(|g| match self.member {
                Some(member) if self.u8(node) == MEMBER => {
                    let at = Field {
                        cell,
                        off,
                        name: field + 10,
                        len,
                        node: node + 1,
                    };
                    unsafe { member(self, v, g, &at) }
                }
                _ => unsafe { self.field_value(node, g) },
            });
            match cast {
                Ok(bits) => unsafe { (cell.add(off) as *mut u64).write(bits) },
                Err(fail) => {
                    // a field not yet filled is zero, which the drop
                    // of the layout skips
                    unsafe { self.release(at, cell as u64) };
                    return Err(fail);
                }
            }
            field = end;
        }
        Ok(cell as u64)
    }

    /// The slot bits of the field value `g` (owned) cast by `node`;
    /// `g` is released.
    ///
    /// # Safety
    /// `node` is a node of this plan; `g` is an owned value.
    pub(super) unsafe fn field_value(&self, node: usize, g: AnyValue) -> Result<u64, Fail> {
        let r = unsafe { self.cast(node, g) };
        unsafe { crate::nanbox_ffi::__torajs_anyv_rc_dec(g) };
        r
    }

    /// `v.[[Get]](name)` for the field name at `name` (`len` bytes):
    /// an owned value, or `Thrown` when a getter threw.
    ///
    /// # Safety
    /// `v` is a live object cell.
    unsafe fn get(&self, v: AnyValue, name: usize, len: usize) -> Result<AnyValue, Fail> {
        let key = unsafe { __torajs_str_alloc(self.base.add(name), len as i64) };
        let got = unsafe {
            crate::member_get_receiver::__torajs_any_member_get_with_receiver(v, key.cast(), v)
        };
        unsafe { __torajs_str_drop(key.cast()) };
        if unsafe { __torajs_throw_check() } != 0 {
            unsafe { crate::nanbox_ffi::__torajs_anyv_rc_dec(got) };
            return Err(Fail::Thrown);
        }
        Ok(got)
    }
}

/// Whether the `Tag::Obj` cell `p` is an instance of a declared class:
/// its layout row's flags word (bit 0, `torajs-structmeta`'s
/// `StructLayoutEntry::flags`) says so.
///
/// # Safety
/// `p` is a live `Tag::Obj` cell.
pub(super) unsafe fn class_instance(p: *mut c_void) -> bool {
    let tag = unsafe { p.cast::<u8>().add(OBJ_CLASS_TAG_OFF).cast::<u32>().read() };
    let row = unsafe { __torajs_struct_layout_lookup(tag) };
    !row.is_null() && unsafe { row.cast::<u8>().add(4).cast::<u32>().read() } & 1 != 0
}

#[cfg(test)]
mod tests;
