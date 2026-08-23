//! §10.4.5 integer-indexed exotic objects — the `TypedArray` view
//! (RFC 20260823-typedarray-substrate 刀 2).
//!
//! ```text
//! { header:8 | buffer:8 (AnyValue) | byte_offset:8 | array_len:8
//!   | kind:1 pad:7 }                                        (40 B)
//! ```
//!
//! `array_len == -1` is a **length-tracking** view (§10.4.5's
//! `[[ArrayLength]] = auto`), whose length is not stored anywhere —
//! it is re-derived from the buffer on every access, because a
//! resizable buffer can change under it between two reads. Absent is
//! a real state; a stored zero would be a different view.
//!
//! Nothing here caches a length across anything that can run user
//! code. `resolve` is the single place that answers "how long is
//! this view right now, and is it still in bounds", and every
//! operation calls it again rather than passing an old answer along.

use core::ffi::c_void;

use torajs_anyvalue::nanbox::{AnyValue, as_void_ptr, is_cell};
use torajs_rc::Tag;

use crate::arraybuffer::{byte_len, data_ptr, is_arraybuffer};

pub(crate) const BUFFER_OFF: usize = 8;
pub(crate) const BYTE_OFFSET_OFF: usize = 16;
pub(crate) const ARRAY_LEN_OFF: usize = 24;
pub(crate) const KIND_OFF: usize = 32;
/// Lazy expando props dynobj — NULL until the first own-property
/// write / define against the view (mirror of torajs-anyvalue
/// `member_get_layout` and torajs-dynobj
/// `layout.rs::TYPEDARRAY_PROPS_OFF`). +33..40 is `kind`'s pad;
/// `alloc_zeroed` seeds the slot.
pub(crate) const PROPS_OFF: usize = 40;
pub(crate) const CELL_SIZE: usize = 48;

/// `array_len` for a view whose length tracks its buffer.
pub(crate) const AUTO_LENGTH: i64 = -1;

/// The element types §23.2 lists, in the order the constructors are
/// declared. The discriminant is stored in the cell and is wire
/// format — append only.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Int8 = 0,
    Uint8 = 1,
    Uint8Clamped = 2,
    Int16 = 3,
    Uint16 = 4,
    Int32 = 5,
    Uint32 = 6,
    Float32 = 7,
    Float64 = 8,
    BigInt64 = 9,
    BigUint64 = 10,
    /// §23.2 Float16Array — appended rather than slotted next to the
    /// other floats, because the discriminant is wire format and the
    /// family tags are one subtraction away from it.
    Float16 = 11,
}

impl Kind {
    /// `BYTES_PER_ELEMENT` (§23.2.5.1 table 71).
    pub const fn element_size(self) -> i64 {
        match self {
            Kind::Int8 | Kind::Uint8 | Kind::Uint8Clamped => 1,
            Kind::Int16 | Kind::Uint16 | Kind::Float16 => 2,
            Kind::Int32 | Kind::Uint32 | Kind::Float32 => 4,
            Kind::Float64 | Kind::BigInt64 | Kind::BigUint64 => 8,
        }
    }

    /// Whether the element type is one of the two BigInt ones —
    /// which is what decides whether a write takes ToBigInt or
    /// ToNumber, and the two are not interchangeable (§23.2.5.x
    /// throws rather than coercing across).
    pub const fn is_bigint(self) -> bool {
        matches!(self, Kind::BigInt64 | Kind::BigUint64)
    }

    /// The constructor name, which is also the `@@toStringTag` and
    /// the `[[TypedArrayName]]`.
    pub const fn name(self) -> &'static str {
        match self {
            Kind::Int8 => "Int8Array",
            Kind::Uint8 => "Uint8Array",
            Kind::Uint8Clamped => "Uint8ClampedArray",
            Kind::Int16 => "Int16Array",
            Kind::Uint16 => "Uint16Array",
            Kind::Int32 => "Int32Array",
            Kind::Uint32 => "Uint32Array",
            Kind::Float32 => "Float32Array",
            Kind::Float64 => "Float64Array",
            Kind::BigInt64 => "BigInt64Array",
            Kind::BigUint64 => "BigUint64Array",
            Kind::Float16 => "Float16Array",
        }
    }

    /// The inverse of [`Kind::name`], for the lowering's name-keyed
    /// constructor route.
    pub fn from_name(name: &str) -> Option<Kind> {
        Some(match name {
            "Int8Array" => Kind::Int8,
            "Uint8Array" => Kind::Uint8,
            "Uint8ClampedArray" => Kind::Uint8Clamped,
            "Int16Array" => Kind::Int16,
            "Uint16Array" => Kind::Uint16,
            "Int32Array" => Kind::Int32,
            "Uint32Array" => Kind::Uint32,
            "Float32Array" => Kind::Float32,
            "Float64Array" => Kind::Float64,
            "BigInt64Array" => Kind::BigInt64,
            "BigUint64Array" => Kind::BigUint64,
            "Float16Array" => Kind::Float16,
            _ => return None,
        })
    }

    pub(crate) const fn from_repr(n: u8) -> Kind {
        match n {
            0 => Kind::Int8,
            1 => Kind::Uint8,
            2 => Kind::Uint8Clamped,
            3 => Kind::Int16,
            4 => Kind::Uint16,
            5 => Kind::Int32,
            6 => Kind::Uint32,
            7 => Kind::Float32,
            8 => Kind::Float64,
            9 => Kind::BigInt64,
            10 => Kind::BigUint64,
            _ => Kind::Float16,
        }
    }
}

/// §7.1 — is `av` an Object? Three heap tags are PRIMITIVES that
/// happen to live on the heap (a long string, a BigInt, a Symbol),
/// and `is_cell` alone would send `new Uint8Array("ab")` down the
/// object path instead of `ToIndex("ab")`, which is 0.
#[inline]
pub fn is_object_value(av: AnyValue) -> bool {
    if !is_cell(av) {
        return false;
    }
    let tag = unsafe { as_void_ptr(av).cast::<u8>().add(4).cast::<u16>().read() };
    tag != Tag::Str as u16 && tag != Tag::BigInt as u16 && tag != Tag::Symbol as u16
}

/// Is `av` a TypedArray cell? Answers on the heap tag alone.
#[inline]
pub fn is_typedarray(av: AnyValue) -> bool {
    if !is_cell(av) {
        return false;
    }
    unsafe { as_void_ptr(av).cast::<u8>().add(4).cast::<u16>().read() == Tag::TypedArray as u16 }
}

/// # Safety
/// `ptr` is a live TypedArray cell.
#[inline]
pub(crate) unsafe fn kind_of(ptr: *mut c_void) -> Kind {
    Kind::from_repr(unsafe { ptr.cast::<u8>().add(KIND_OFF).read() })
}

/// # Safety
/// `ptr` is a live TypedArray cell.
#[inline]
pub(crate) unsafe fn buffer_of(ptr: *mut c_void) -> AnyValue {
    unsafe { (ptr.cast::<u8>().add(BUFFER_OFF) as *const u64).read() }
}

/// # Safety
/// `ptr` is a live TypedArray cell.
#[inline]
pub(crate) unsafe fn byte_offset_of(ptr: *mut c_void) -> i64 {
    unsafe { (ptr.cast::<u8>().add(BYTE_OFFSET_OFF) as *const i64).read() }
}

/// # Safety
/// `ptr` is a live TypedArray cell.
#[inline]
pub(crate) unsafe fn array_len_of(ptr: *mut c_void) -> i64 {
    unsafe { (ptr.cast::<u8>().add(ARRAY_LEN_OFF) as *const i64).read() }
}

/// What a view is RIGHT NOW: its data pointer and its length, or
/// `None` when it is out of bounds — which §10.4.5 treats as the same
/// answer as detached everywhere it matters.
///
/// This is the only place that decides a length. Callers re-ask
/// rather than carry an old answer across anything that can run user
/// code, because a resizable buffer moves under them.
///
/// # Safety
/// `ptr` is a live TypedArray cell.
pub(crate) unsafe fn resolve(ptr: *mut c_void) -> Option<(*mut u8, i64)> {
    unsafe {
        let buf = buffer_of(ptr);
        if !is_arraybuffer(buf) {
            return None;
        }
        let bptr = as_void_ptr(buf);
        let data = data_ptr(bptr);
        if data.is_null() {
            return None;
        }
        let buf_len = byte_len(bptr);
        let off = byte_offset_of(ptr);
        if off > buf_len {
            return None;
        }
        let esize = kind_of(ptr).element_size();
        let stored = array_len_of(ptr);
        let len = if stored == AUTO_LENGTH {
            (buf_len - off) / esize
        } else {
            if off + stored * esize > buf_len {
                return None;
            }
            stored
        };
        Some((data.add(off as usize), len))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn element_sizes_match_table_71() {
        assert_eq!(Kind::Int8.element_size(), 1);
        assert_eq!(Kind::Uint8Clamped.element_size(), 1);
        assert_eq!(Kind::Int16.element_size(), 2);
        assert_eq!(Kind::Uint32.element_size(), 4);
        assert_eq!(Kind::Float32.element_size(), 4);
        assert_eq!(Kind::Float64.element_size(), 8);
        assert_eq!(Kind::BigUint64.element_size(), 8);
        assert_eq!(Kind::Float16.element_size(), 2);
    }

    #[test]
    fn names_round_trip_and_reprs_are_stable() {
        for n in 0u8..=11 {
            let k = Kind::from_repr(n);
            assert_eq!(k as u8, n);
            assert_eq!(Kind::from_name(k.name()), Some(k));
        }
        assert_eq!(Kind::from_name("Float64Array"), Some(Kind::Float64));
        assert_eq!(Kind::from_name("nope"), None);
    }

    #[test]
    fn only_the_two_bigint_kinds_are_bigint() {
        let big: Vec<Kind> = (0u8..=11)
            .map(Kind::from_repr)
            .filter(|k| k.is_bigint())
            .collect();
        assert_eq!(big, vec![Kind::BigInt64, Kind::BigUint64]);
    }
}
