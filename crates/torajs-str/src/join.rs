//! Two operands into one fresh cell — the core under every concat
//! kernel that borrows both sides.
//!
//! A [`Side`] is an operand as the join reads it: where its code units
//! start, how many there are, and the header flags that describe them.
//! An owned Str and a Substr view differ only in how that triple is
//! found, so one join serves `Str + Str` and the three view mixes.
//!
//! The result takes the wider of the two encodings: both Latin-1 gives
//! Latin-1, anything else UTF-16 with the Latin-1 side widened on the
//! way in. It is marked ASCII only when both sides are.
//!
//! [`join`] is laid out for the short, same-width case — a word and a
//! separator, a key and a suffix. When the joined payload fits a pooled
//! block and the pool has one, the whole kernel is straight-line code
//! with no call in it: pop, header, two [`copy_short`]s. Everything
//! else (a width mismatch, a long result, an empty pool class) leaves
//! through one tail call to [`join_general`].

use torajs_rc::HeapHeader;

use crate::block::StrBlock;
use crate::copy::{copy_bytes, copy_short, widen_latin1};
use crate::layout::{
    STR_DATA_OFF, STR_FLAG_IS_ASCII, STR_FLAG_IS_LATIN1, STR_LEN_OFF, STR_POOL_PAYLOAD,
};
use crate::max_len::{raise_too_long, str_len_of};
use crate::substr_methods::{substr_len, substr_offset, substr_parent};

/// One operand of a concat: `len` code units at `ptr`, in the encoding
/// `flags` names ([`STR_FLAG_IS_LATIN1`]) and with the ASCII mark
/// ([`STR_FLAG_IS_ASCII`]) its cell — or a view's parent — carries.
// `repr(C)`, and handed to the out-of-line halves through the C ABI:
// that way a side travels in two registers and the shortcut can leave
// with a plain tail call instead of spilling it
#[derive(Clone, Copy)]
#[repr(C)]
pub(crate) struct Side {
    pub ptr: *const u8,
    pub len: u32,
    pub flags: u16,
}

impl Side {
    /// The text a null Str operand reads as: the JS `null` value.
    pub const NULL_TEXT: Side = Side {
        ptr: b"null".as_ptr(),
        len: 4,
        flags: STR_FLAG_IS_LATIN1 | STR_FLAG_IS_ASCII,
    };

    /// No code units: joined on, it leaves the other side alone.
    pub const EMPTY: Side = Side {
        ptr: b"".as_ptr(),
        len: 0,
        flags: STR_FLAG_IS_LATIN1 | STR_FLAG_IS_ASCII,
    };

    /// # Safety
    ///
    /// `s` is a valid, non-null Str heap block.
    #[inline(always)]
    pub unsafe fn of_str(s: *const u8) -> Self {
        unsafe {
            Side {
                ptr: s.add(STR_DATA_OFF),
                len: (s.add(STR_LEN_OFF) as *const u32).read(),
                flags: (*(s as *const HeapHeader)).flags,
            }
        }
    }

    /// [`Self::of_str`], with a null pointer read as [`Self::NULL_TEXT`].
    ///
    /// # Safety
    ///
    /// `s` is null or a valid Str heap block.
    #[inline(always)]
    pub unsafe fn of_nullable_str(s: *const u8) -> Self {
        if s.is_null() {
            Self::NULL_TEXT
        } else {
            unsafe { Self::of_str(s) }
        }
    }

    /// # Safety
    ///
    /// `v` is a valid Substr whose parent is live.
    #[inline(always)]
    pub unsafe fn of_view(v: *const u8) -> Self {
        unsafe {
            let parent = substr_parent(v);
            let flags = (*(parent as *const HeapHeader)).flags;
            let shift = u32::from(flags & STR_FLAG_IS_LATIN1 == 0);
            let at = (substr_offset(v) as usize) << shift;
            Side {
                ptr: parent.add(STR_DATA_OFF + at),
                len: substr_len(v) as u32,
                flags,
            }
        }
    }

    #[inline(always)]
    pub fn is_latin1(self) -> bool {
        self.flags & STR_FLAG_IS_LATIN1 != 0
    }

    /// Whether the side's code units all fit Latin-1: its width says
    /// so, or it has none.
    #[inline(always)]
    pub fn fits_latin1(self) -> bool {
        self.is_latin1() || self.len == 0
    }

    /// The side's ASCII mark, which an empty side always has.
    #[inline(always)]
    pub fn ascii_mark(self) -> u16 {
        if self.len == 0 {
            STR_FLAG_IS_ASCII
        } else {
            self.flags & STR_FLAG_IS_ASCII
        }
    }
}

/// Fresh refcount-1 Str holding `a` followed by `b`.
///
/// # Safety
///
/// Each side is readable for its `len` code units in its encoding.
#[inline(always)]
pub(crate) unsafe fn join(a: Side, b: Side) -> *mut u8 {
    unsafe { join_or(a, b, |ptr, len, flags| too_long_keeping(ptr, len, flags)) }
}

/// [`join`] with the answer for a result past the longest Str left to
/// `too_long`, handed the left side's fields: it raises the RangeError
/// and answers the left side.
///
/// # Safety
///
/// Same contract as [`join`]; `too_long` answers an owned Str.
#[inline(always)]
pub(crate) unsafe fn join_or(
    a: Side,
    b: Side,
    too_long: impl FnOnce(*const u8, u32, u16) -> *mut u8,
) -> *mut u8 {
    // both lengths under twice the largest pooled payload: neither the
    // sum below nor its byte count can wrap
    if (a.flags ^ b.flags) & STR_FLAG_IS_LATIN1 == 0 && (a.len | b.len) < 2 * STR_POOL_PAYLOAD {
        let shift = u32::from(!a.is_latin1());
        let a_bytes = a.len << shift;
        let b_bytes = b.len << shift;
        // the width bit is the same on both sides; the ASCII mark
        // needs both
        let flags = a.flags & b.flags & (STR_FLAG_IS_LATIN1 | STR_FLAG_IS_ASCII);
        if let Some(block) = StrBlock::pop_pooled(a.len + b.len, a_bytes + b_bytes, flags) {
            unsafe {
                let dst = block.0.as_ptr().add(STR_DATA_OFF);
                copy_short(dst, a.ptr, a_bytes as usize);
                copy_short(dst.add(a_bytes as usize), b.ptr, b_bytes as usize);
            }
            return block.into_raw();
        }
    }
    if !fits_one_str(a, b) {
        return too_long(a.ptr, a.len, a.flags);
    }
    unsafe { join_general(a, b) }
}

/// Raise the RangeError for a join past the longest Str and answer a
/// fresh cell holding the left side alone: the caller stores the
/// answer before its throw check runs, and `s = s + x` must leave `s`
/// as it was.
///
/// The side comes apart in its three fields: a [`Side`] argument made
/// the hot joins build it in a frame slot just to have it ready for
/// this call they almost never make.
///
/// # Safety
///
/// `ptr` is readable for `len` code units in the encoding `flags`
/// names.
#[cold]
#[inline(never)]
pub(crate) unsafe fn too_long_keeping(ptr: *const u8, len: u32, flags: u16) -> *mut u8 {
    unsafe {
        raise_too_long();
        join_general(Side { ptr, len, flags }, Side::EMPTY)
    }
}

/// Whether `a` and `b` together fit the longest Str. Each side is at
/// most [`crate::max_len::STR_MAX_LEN`], so the u32 sum cannot wrap.
#[inline(always)]
pub(crate) fn fits_one_str(a: Side, b: Side) -> bool {
    str_len_of((a.len + b.len) as usize).is_some()
}

/// [`join`] without the short-case shortcut: any widths, any length,
/// allocating when the pool has nothing.
///
/// # Safety
///
/// Same contract as [`join`], and the two sides [`fits_one_str`]: the
/// callers ask, so the copy loops here stay as they were.
#[inline(never)]
pub(crate) unsafe extern "C" fn join_general(a: Side, b: Side) -> *mut u8 {
    // an empty side holds no code unit to keep the width for: the
    // other side alone picks it, or a Latin-1 payload would come out
    // UTF-16
    let out_latin1 = a.fits_latin1() && b.fits_latin1();
    // only a Latin-1 cell ever carries the ASCII mark
    let ascii = a.ascii_mark() & b.ascii_mark();
    let block = StrBlock::alloc_with_flags(a.len + b.len, out_latin1, ascii);
    let shift = u32::from(!out_latin1);
    unsafe {
        let dst = block.0.as_ptr().add(STR_DATA_OFF);
        write_side(dst, a, out_latin1);
        write_side(dst.add((a.len as usize) << shift), b, out_latin1);
    }
    block.into_raw()
}

/// Write one side at `dst` in the result's encoding, widening it when
/// the result is UTF-16 and the side is not.
///
/// # Safety
///
/// `dst` is writable for the side's code units in the result encoding.
#[inline(always)]
pub(crate) unsafe fn write_side(dst: *mut u8, side: Side, out_latin1: bool) {
    unsafe {
        if side.is_latin1() == out_latin1 {
            let shift = u32::from(!out_latin1);
            copy_bytes(dst, side.ptr, (side.len as usize) << shift);
        } else {
            widen_latin1(dst, side.ptr, side.len as usize);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::__torajs_str_free;
    use crate::max_len::STR_MAX_LEN;
    use core::sync::atomic::Ordering;

    #[test]
    fn a_join_past_the_longest_string_raises_and_answers_the_left_side() {
        let left = Side {
            ptr: b"left".as_ptr(),
            len: 4,
            flags: STR_FLAG_IS_LATIN1 | STR_FLAG_IS_ASCII,
        };
        // only the length is read before the bound refuses the join
        let huge = Side {
            ptr: b"".as_ptr(),
            len: STR_MAX_LEN as u32,
            flags: STR_FLAG_IS_LATIN1,
        };
        crate::STUB_THROW_RAISED.store(false, Ordering::SeqCst);
        let out = unsafe { join(left, huge) };
        assert!(crate::STUB_THROW_RAISED.load(Ordering::SeqCst));
        let got = unsafe { Side::of_str(out) };
        assert_eq!(got.len, 4);
        assert_eq!(unsafe { core::slice::from_raw_parts(got.ptr, 4) }, b"left");
        unsafe { __torajs_str_free(out) };
    }
}
