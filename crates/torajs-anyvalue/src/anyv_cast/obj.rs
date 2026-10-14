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

use super::plan::{Fail, NUM, OBJ, OPT, Plan};
use super::{cell_tag, reject};
use crate::arg_struct_coerce::{
    __torajs_anyv_struct_fits, OBJ_CLASS_TAG_OFF, OBJ_HEADER_SIZE, OBJ_TAG_OFF,
};
use crate::nanbox::{AnyValue, as_void_ptr, is_cell, is_null, is_undefined};

/// OBJ node flags: the slot names a class; the node copies nothing
/// (a field of the layout is of a type the plan cannot judge).
const CLASS: u8 = 1;
const FITS_ONLY: u8 = 2;
/// The OBJ node has no class list.
const NO_LIST: u32 = u32::MAX;
const OBJ_VTABLE_OFF: usize = 16;

unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_str_alloc(p: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(p: *mut c_void);
    fn __torajs_throw_check() -> i64;
    fn __torajs_struct_layout_lookup(class_tag: u32) -> *const c_void;
    /// torajs-dynobj — the deletion record of a declared field, for a
    /// slot past the ones the absent set can name.
    fn __torajs_obj_declared_field_delete(obj: *mut c_void, key: *mut c_void) -> i64;
}

/// The slots the absent set names: bit `i` of the u32 at
/// [`ABSENT_SET_OFF`] for slot `i` (torajs-meta `struct_absent`).
const ABSENT_SET_SLOTS: usize = 32;
const ABSENT_SET_OFF: usize = 12;
/// The header's flags half-word.
const OBJ_FLAGS_OFF: usize = 6;

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
    let plan = Plan::new(plan, vtables);
    match unsafe { plan.cast(4, v) } {
        Ok(bits) => bits as *mut c_void,
        Err(Fail::Mismatch) => {
            unsafe { reject(v, site) };
            core::ptr::null_mut()
        }
        Err(Fail::Thrown) => core::ptr::null_mut(),
    }
}

impl Plan {
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
        if let Some(copy) = self.copy_of(v, at) {
            return Ok(copy);
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
        self.note_copy(v, at, cell as u64);
        let mut field = at + 16;
        for _ in 0..n {
            let end = self.u32(field) as usize;
            let off = self.u32(field + 4) as usize;
            let len = self.u16(field + 8) as usize;
            let node = field + 10 + len;
            let got = unsafe { self.get(v, field + 10, len) };
            let cast = got.and_then(|g| {
                if self.u8(node) == OPT
                    && is_undefined(g)
                    && !unsafe { self.owns(v, field + 10, len) }
                {
                    unsafe { self.mark_absent(cell, off, field + 10, len) };
                }
                if self.optional_number(node) && is_null(g) {
                    return unsafe { self.park_null(cell, off, field + 10, len) };
                }
                let r = unsafe { self.cast(node, g) };
                unsafe { crate::nanbox_ffi::__torajs_anyv_rc_dec(g) };
                r
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

    /// Whether the node at `at` is an optional `number` member's: `OPT`
    /// over `NUM`, an f64 slot that spells a number or `undefined`.
    fn optional_number(&self, at: usize) -> bool {
        self.u8(at) == OPT && self.u8(at + 1) == NUM
    }

    /// A `null` the value carries for an optional `number` member: the
    /// slot takes its undefined sentinel and the `null` goes to the
    /// field's sidecar behind `FLAG_OBJ_EXOTIC_FIELD` (§10.1.9.2 [[Set]]
    /// on the fresh cell), where a write through the any lane would
    /// have put it. Answers the slot bits.
    ///
    /// # Safety
    /// `cell` is the fresh cell being built, with its header written;
    /// `name` / `len` address the field name in the plan.
    unsafe fn park_null(
        &self,
        cell: *mut u8,
        off: usize,
        name: usize,
        len: usize,
    ) -> Result<u64, Fail> {
        unsafe {
            (cell.add(off) as *mut u64).write(torajs_rc::F64_UNDEF_SENTINEL_BITS);
        }
        let key = unsafe { __torajs_str_alloc(self.base.add(name), len as i64) };
        unsafe {
            crate::struct_field_ffi::exotic::__torajs_obj_field_set_exotic_impl(
                cell.cast(),
                key.cast(),
                0,
                0,
                1,
            );
            __torajs_str_drop(key.cast());
        }
        if unsafe { __torajs_throw_check() } != 0 {
            return Err(Fail::Thrown);
        }
        Ok(torajs_rc::F64_UNDEF_SENTINEL_BITS)
    }

    /// Whether `v` owns the property named at `name` (`len` bytes):
    /// §7.3.12 HasOwnProperty, which tells a member the value lacks from
    /// one it holds as `undefined`.
    ///
    /// # Safety
    /// `v` is a live object cell.
    unsafe fn owns(&self, v: AnyValue, name: usize, len: usize) -> bool {
        let key = unsafe { __torajs_str_alloc(self.base.add(name), len as i64) };
        let has = unsafe { crate::prop_has::__torajs_any_own_prop_has(v, key.cast()) };
        unsafe { __torajs_str_drop(key.cast()) };
        has != 0
    }

    /// The optional member at slot offset `off` is no own property of
    /// the cell: the value lacked it (§10.1.11 — `"a" in p` is false
    /// and the keys leave it out). Recorded the way a literal that
    /// leaves the member out records it: the header bit and the slot's
    /// bit in the absent set, or the deletion record for a slot the set
    /// cannot name.
    ///
    /// # Safety
    /// `cell` is the fresh cell being built, with its header written;
    /// `name` / `len` address the field name in the plan.
    unsafe fn mark_absent(&self, cell: *mut u8, off: usize, name: usize, len: usize) {
        let slot = (off - OBJ_HEADER_SIZE) / 8;
        if slot < ABSENT_SET_SLOTS {
            unsafe {
                let flags = cell.add(OBJ_FLAGS_OFF) as *mut u16;
                flags.write(flags.read() | torajs_rc::flags::FLAG_OBJ_ABSENT_FIELD);
                let set = cell.add(ABSENT_SET_OFF) as *mut u32;
                set.write(set.read() | 1 << slot);
            }
            return;
        }
        let key = unsafe { __torajs_str_alloc(self.base.add(name), len as i64) };
        unsafe {
            __torajs_obj_declared_field_delete(cell.cast(), key.cast());
            __torajs_str_drop(key.cast());
        }
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
unsafe fn class_instance(p: *mut c_void) -> bool {
    let tag = unsafe { p.cast::<u8>().add(OBJ_CLASS_TAG_OFF).cast::<u32>().read() };
    let row = unsafe { __torajs_struct_layout_lookup(tag) };
    !row.is_null() && unsafe { row.cast::<u8>().add(4).cast::<u32>().read() } & 1 != 0
}

#[cfg(test)]
mod tests {
    use super::super::plan::{ANY, BOOL, BOXED, INT, NUM, OPT, STR};
    use super::*;
    use crate::nanbox::{VALUE_NULL, VALUE_TRUE, VALUE_UNDEFINED, box_double, box_int32};
    use torajs_rc::undef_cell::undef_cell_ptr;

    /// A plan whose root is `node`, with an empty lists table.
    fn plan_of(node: &[u8]) -> Vec<u8> {
        let lists_off = (4 + node.len()) as u32;
        let mut p = lists_off.to_le_bytes().to_vec();
        p.extend_from_slice(node);
        p.extend_from_slice(&0u32.to_le_bytes());
        p
    }

    fn cast(node: &[u8], v: AnyValue) -> Option<u64> {
        let bytes = plan_of(node);
        let plan = Plan::new(bytes.as_ptr(), core::ptr::null());
        unsafe { plan.cast(4, v) }.ok()
    }

    #[test]
    fn scalar_nodes_take_values_of_their_type() {
        assert_eq!(cast(&[INT], box_double(4.0)), Some(4));
        assert_eq!(cast(&[INT], box_double(4.5)), None);
        assert_eq!(cast(&[NUM], box_int32(2)), Some(2.0f64.to_bits()));
        assert_eq!(cast(&[BOOL], VALUE_TRUE), Some(1));
        assert_eq!(cast(&[BOOL], box_int32(1)), None);
        assert_eq!(cast(&[STR], VALUE_TRUE), None);
    }

    #[test]
    fn an_any_node_takes_any_primitive_as_it_is() {
        for v in [VALUE_UNDEFINED, VALUE_NULL, VALUE_TRUE, box_int32(9)] {
            assert_eq!(cast(&[ANY], v), Some(v));
        }
    }

    #[test]
    fn a_boxed_member_takes_its_kind_and_nullish_values() {
        assert_eq!(cast(&[BOXED, 0], box_double(2.5)), Some(box_double(2.5)));
        assert_eq!(cast(&[BOXED, 0], VALUE_NULL), Some(VALUE_NULL));
        assert_eq!(cast(&[BOXED, 0], VALUE_TRUE), None);
        assert_eq!(cast(&[BOXED, 1], VALUE_TRUE), Some(VALUE_TRUE));
    }

    #[test]
    fn an_optional_number_takes_undefined_as_the_sentinel_and_refuses_null() {
        assert_eq!(
            cast(&[OPT, NUM], VALUE_UNDEFINED),
            Some(torajs_rc::F64_UNDEF_SENTINEL_BITS)
        );
        assert_eq!(cast(&[OPT, NUM], VALUE_NULL), None);
        assert_eq!(cast(&[OPT, NUM], box_int32(3)), Some(3.0f64.to_bits()));
        assert_eq!(cast(&[OPT, NUM], VALUE_TRUE), None);
    }

    #[test]
    fn an_optional_object_takes_nullish_values_as_their_spellings() {
        let obj = [
            OPT, OBJ, 0, 1, 0, 0, 0, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0, 0, 0,
        ];
        assert_eq!(cast(&obj, VALUE_UNDEFINED), Some(undef_cell_ptr() as u64));
        assert_eq!(cast(&obj, VALUE_NULL), Some(0));
        assert_eq!(cast(&obj, box_int32(1)), None);
    }
}
