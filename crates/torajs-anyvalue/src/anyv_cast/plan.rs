//! The cast plan the object cast walks (`super::obj`): the slot's
//! static type, compiled by the lowering into rodata bytes
//! (`ssa_lower_checked_cast::plan`, which writes the same layout).
//!
//! ```text
//! plan  := u32 lists_off, node (the root, at offset 4), lists
//! node  := u8 kind, then by kind:
//!   ANY  INT  NUM  BOOL  STR            nothing
//!   BOXED   u8 member (0 number, 1 boolean): a `T | null` box
//!   OBJ     u8 flags (1 = a class slot, 2 = copies nothing, 4 =
//!           lists keys), u32 class_tag, u32 class list index
//!           (u32::MAX: none), u32 vtable index, u16 field count,
//!           fields, [keys]
//!   keys    u16 count, then per key: u16 length, name (WTF-8): the
//!           class's prototype-chain keys ([`Walk`]), a
//!           well-known symbol key spelled `__sym_Symbol_<x>__`
//!   REF     u32 offset of an OBJ node (a type that contains itself)
//!   ARR     u8 element kind (`__torajs_arr_any_to_typed`), node
//!   CELL    u16 heap tag
//!   OPT     node: the node's type, `| null | undefined`; over NUM
//!           (an optional `number` member's f64 slot) `undefined` is
//!           the slot's sentinel and a field's `null` goes to the
//!           cell's sidecar (`super::members`)
//! field := u32 end (offset past this field), u32 slot offset,
//!          u16 name length, name (WTF-8), [MEMBER] node
//!   MEMBER  the field is an optional member (`m?: T`) over an OPT
//!           node: a value lacking the key leaves it out of the copy
//!           (`super::members`)
//! lists := u32 count, then per list: u32 n, u32 tags[n]
//! ```
//!
//! Reads are unaligned: the bytes are a string literal block.

mod arr;

use core::cell::RefCell;
use core::ffi::c_void;
use core::marker::PhantomData;
use std::collections::BTreeMap;

use torajs_rc::Tag;
use torajs_rc::undef_cell::undef_cell_ptr;

use super::cell_tag;
use crate::nanbox::{
    AnyValue, as_bool, as_double, as_int32, as_void_ptr, is_bool, is_cell, is_double, is_int32,
    is_null, is_short_str, is_undefined,
};

pub(super) const ANY: u8 = 0;
pub(super) const INT: u8 = 1;
pub(super) const NUM: u8 = 2;
pub(super) const BOOL: u8 = 3;
pub(super) const STR: u8 = 4;
pub(super) const BOXED: u8 = 5;
pub(super) const OBJ: u8 = 6;
pub(super) const REF: u8 = 7;
pub(super) const ARR: u8 = 8;
pub(super) const CELL: u8 = 9;
pub(super) const OPT: u8 = 10;
pub(super) const MEMBER: u8 = 11;

/// Below 2^53 every integer-valued double converts to i64 exactly.
const I64_EXACT: f64 = 9_007_199_254_740_992.0;

unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_str_undef() -> *mut u8;
}

/// Why a value did not cross: it is not of the slot's type (the
/// caller records the TypeError), or user code run on the way (a
/// getter) or a nested kernel already recorded a throw.
pub(super) enum Fail {
    Mismatch,
    Thrown,
}

/// A plan, the vtable addresses of its object nodes, and the copies
/// this cast has made so far: an object or array reached again (a
/// cycle, or one value under two names) answers the copy already made
/// for the same node, so a cycle stays a cycle and the walk ends.
///
/// `member` casts an optional member's field (a `MEMBER` node); only
/// the entry for a plan with one sets it (`super::members`), so a
/// program that casts no optional member links none of that code.
///
/// `W` is the walk ([`Walk`]). The layout does not depend on it.
#[repr(C)]
pub(super) struct Plan<W: Walk = Plain> {
    pub(super) copies: Copies,
    pub(super) base: *const u8,
    pub(super) vtables: *const *mut c_void,
    pub(super) member: Option<MemberCast<W>>,
    walk: PhantomData<W>,
}

/// The cast of an optional member's field into the fresh cell, from
/// the value `v` that holds the field and the field's value (owned).
pub(super) type MemberCast<W> =
    unsafe fn(&Plan<W>, AnyValue, AnyValue, &Field) -> Result<u64, Fail>;

/// How a cast walks a plan whose class nodes list keys (`PROTO_KEYS`)
/// an object copied into them must not hold: the copy's methods and
/// accessors are the class's, so an object holding a property under
/// one of those names would answer the class's member instead of its
/// own. The own walk refuses such an object; the walk every other
/// program links, [`Plain`], never reads the keys. The own walk is
/// instantiated only by the crate that defines it, an archive member
/// of its own, so a program that casts into no class with a method or
/// accessor links none of it; it walks every node that may hold an
/// object node and hands the plain walk every other one (the walk's
/// `cast_own`).
///
/// That crate calls every non-generic function the walk calls, so each
/// is exported from this one; a function that is otherwise inlined at
/// every call site is marked `#[inline]`, or exporting it would add an
/// out-of-line copy to this member, which every link parses.
pub trait Walk: Sized {
    /// The walk reads the keys a class node lists.
    const OWN: bool;

    /// Whether the object `v` holds a property under the key `name`
    /// (`len` bytes, WTF-8, a well-known symbol key spelled
    /// `__sym_Symbol_<x>__`).
    ///
    /// # Safety
    /// `v` is a live object cell; `name` addresses `len` bytes.
    unsafe fn holds(v: AnyValue, name: *const u8, len: usize) -> bool;
}

/// The walk that does not read the keys.
pub enum Plain {}

impl Walk for Plain {
    const OWN: bool = false;

    unsafe fn holds(_v: AnyValue, _name: *const u8, _len: usize) -> bool {
        false
    }
}

/// The copies a cast has made so far, by the value and the node they
/// were made for.
pub(super) struct Copies(RefCell<BTreeMap<(AnyValue, usize), u64>>);

impl Copies {
    /// The copy already made of `v` for the node at `at`, with a stake
    /// for the caller.
    #[inline]
    pub(super) fn copy_of(&self, v: AnyValue, at: usize) -> Option<u64> {
        let copy = *self.0.borrow().get(&(v, at))?;
        unsafe { __torajs_rc_inc(copy as *mut c_void) };
        Some(copy)
    }

    /// Record `copy` as the copy of `v` for the node at `at`, before
    /// its contents are filled in.
    #[inline]
    pub(super) fn note_copy(&self, v: AnyValue, at: usize, copy: u64) {
        self.0.borrow_mut().insert((v, at), copy);
    }
}

/// A field of an OBJ node being copied: the fresh cell and the slot's
/// offset in it, the name (`len` bytes at `name`) and the field's node
/// (past its `MEMBER` byte).
pub(super) struct Field {
    pub(super) cell: *mut u8,
    pub(super) off: usize,
    pub(super) name: usize,
    pub(super) len: usize,
    pub(super) node: usize,
}

impl<W: Walk> Plan<W> {
    pub(super) fn new(base: *const u8, vtables: *const *mut c_void) -> Plan<W> {
        Plan {
            copies: Copies(RefCell::new(BTreeMap::new())),
            base,
            vtables,
            member: None,
            walk: PhantomData,
        }
    }

    pub(super) fn u8(&self, at: usize) -> u8 {
        unsafe { self.base.add(at).read() }
    }

    pub(super) fn u16(&self, at: usize) -> u16 {
        unsafe { self.base.add(at).cast::<u16>().read_unaligned() }
    }

    pub(super) fn u32(&self, at: usize) -> u32 {
        unsafe { self.base.add(at).cast::<u32>().read_unaligned() }
    }

    /// The class list `idx` of the lists table, in the layout
    /// `__torajs_anyv_struct_fits` reads (a u32 count, then the tags).
    pub(super) fn class_list(&self, idx: u32) -> *const u8 {
        let mut at = self.u32(0) as usize + 4;
        for _ in 0..idx {
            at += 4 + 4 * self.u32(at) as usize;
        }
        unsafe { self.base.add(at) }
    }

    /// The slot bits of `v` cast by the node at `at`, holding a stake
    /// of their own; `v` is borrowed.
    ///
    /// # Safety
    /// `at` is a node of this plan; a cell payload of `v` is live.
    pub(super) unsafe fn cast(&self, at: usize, v: AnyValue) -> Result<u64, Fail> {
        if W::OWN {
            return unsafe { self.cast_own(at, v) };
        }
        let tag = is_cell(v).then(|| unsafe { cell_tag(v) });
        match self.u8(at) {
            ANY => {
                if is_cell(v) {
                    unsafe { __torajs_rc_inc(as_void_ptr(v)) };
                }
                Ok(v)
            }
            INT if is_int32(v) => Ok(as_int32(v) as i64 as u64),
            INT if is_double(v)
                && as_double(v).fract() == 0.0
                && as_double(v).abs() < I64_EXACT =>
            {
                Ok(as_double(v) as i64 as u64)
            }
            NUM if is_int32(v) => Ok((as_int32(v) as f64).to_bits()),
            NUM if is_double(v) => Ok(as_double(v).to_bits()),
            BOOL if is_bool(v) => Ok(u64::from(as_bool(v))),
            STR if is_short_str(v) || tag == Some(Tag::Str as u16) => {
                Ok(unsafe { crate::nanbox_ffi::__torajs_anyv_to_str(v) } as u64)
            }
            BOXED if is_null(v) || is_undefined(v) => Ok(v),
            BOXED if self.u8(at + 1) == 0 && (is_int32(v) || is_double(v)) => Ok(v),
            BOXED if self.u8(at + 1) == 1 && is_bool(v) => Ok(v),
            OPT if is_undefined(v) => Ok(match self.u8(at + 1) {
                STR => (unsafe { __torajs_str_undef() }) as u64,
                NUM => torajs_rc::F64_UNDEF_SENTINEL_BITS,
                _ => undef_cell_ptr() as u64,
            }),
            // an f64 slot has no spelling for null: a field's takes it
            // to the sidecar (`super::obj`), a bare node refuses it
            OPT if is_null(v) && self.u8(at + 1) == NUM => Err(Fail::Mismatch),
            OPT if is_null(v) => Ok(0),
            OPT => unsafe { self.cast(at + 1, v) },
            REF => unsafe { self.cast(self.u32(at + 1) as usize, v) },
            CELL if tag == Some(self.u16(at + 1)) => {
                unsafe { __torajs_rc_inc(as_void_ptr(v)) };
                Ok(v)
            }
            ARR if tag == Some(Tag::Arr as u16) => unsafe { self.arr(at, as_void_ptr(v)) },
            OBJ => unsafe { self.obj(at, v) },
            _ => Err(Fail::Mismatch),
        }
    }

    /// [`Self::cast`] in the own walk: a node that may hold an object
    /// node (an object, a reference to one, an array, a non-nullish
    /// value of an optional node) is walked here, so every object node
    /// is reached by this walk; any other node, which holds none, is
    /// cast by the plain walk.
    ///
    /// # Safety
    /// As [`Self::cast`].
    unsafe fn cast_own(&self, at: usize, v: AnyValue) -> Result<u64, Fail> {
        match self.u8(at) {
            OBJ => unsafe { self.obj(at, v) },
            REF => unsafe { self.cast_own(self.u32(at + 1) as usize, v) },
            ARR if is_cell(v) && unsafe { cell_tag(v) } == Tag::Arr as u16 => unsafe {
                self.arr(at, as_void_ptr(v))
            },
            OPT if !(is_undefined(v) || is_null(v)) => unsafe { self.cast_own(at + 1, v) },
            // SAFETY: the two walks share one layout, and the plain
            // walk reaches no object node (nor the `member` cast an
            // object node's fields take) from a node that holds none
            node => {
                debug_assert!(!matches!(node, OBJ | REF | MEMBER));
                unsafe { (*(self as *const Self).cast::<Plan>()).cast(at, v) }
            }
        }
    }

    /// Release slot bits [`Self::cast`] answered for the node at `at`.
    ///
    /// # Safety
    /// `bits` came from `cast` on that node.
    pub(super) unsafe fn release(&self, at: usize, bits: u64) {
        let at = match self.u8(at) {
            OPT => at + 1,
            _ => at,
        };
        if matches!(self.u8(at), INT | NUM | BOOL | BOXED) {
            return;
        }
        if bits == 0 || bits == undef_cell_ptr() as u64 || is_str_undef(bits) {
            return;
        }
        unsafe { crate::nanbox_ffi::__torajs_anyv_rc_dec(bits) };
    }
}

/// `bits` is the Str slot's undefined sentinel.
#[inline]
fn is_str_undef(bits: u64) -> bool {
    bits == unsafe { __torajs_str_undef() } as u64
}
