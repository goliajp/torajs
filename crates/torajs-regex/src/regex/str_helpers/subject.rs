//! The searched text as a hit's consumers see it: what its offsets
//! count, how a span of it becomes a string. The runtime engine walks
//! bytes — the ASCII payload in place, or a transcoded copy — so its
//! offsets are mapped to code units through the prefix; a compiled
//! matcher walks the string's own code units, so its offsets are the
//! code units already and a span is a slice of the string.

use core::ffi::c_void;

use super::{STR_HDR_SIZE, span_str};
use crate::regex::__torajs_rc_inc;
use crate::regex::offset_map::byte_to_utf16_units;
use crate::regex::static_keys::FLAG_STATIC_LITERAL;

unsafe extern "C" {
    /// torajs-str — `s.slice(start, end)` in code units of `s`
    fn __torajs_str_slice(s: *const u8, start: i64, end: i64) -> *mut u8;
}

/// A string's own payload: Latin-1 bytes, or UTF-16 units.
pub enum UnitsView<'a> {
    Narrow(&'a [u8]),
    Wide(&'a [u16]),
}

impl UnitsView<'_> {
    #[inline]
    pub fn len(&self) -> usize {
        match self {
            UnitsView::Narrow(s) => s.len(),
            UnitsView::Wide(s) => s.len(),
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub fn as_ptr(&self) -> *const u8 {
        match self {
            UnitsView::Narrow(s) => s.as_ptr(),
            UnitsView::Wide(s) => s.as_ptr() as *const u8,
        }
    }

    #[inline]
    pub fn wide(&self) -> bool {
        matches!(self, UnitsView::Wide(_))
    }

    #[inline]
    pub fn unit(&self, i: usize) -> u32 {
        match self {
            UnitsView::Narrow(s) => s[i] as u32,
            UnitsView::Wide(s) => s[i] as u32,
        }
    }
}

/// The payload of `p`, in its own encoding (flags u16 @6 bit 1 is
/// Latin-1; length u32 @8 counts code units).
///
/// # Safety
///
/// `p` is a live tora Str whose payload outlives `'a`.
#[inline]
pub unsafe fn str_units_view<'a>(p: *const c_void) -> UnitsView<'a> {
    let s = p as *const u8;
    let length = unsafe { *(s.add(8) as *const u32) } as usize;
    let flags = unsafe { *(s.add(6) as *const u16) };
    let payload = unsafe { s.add(STR_HDR_SIZE) };
    if flags & 0x0002 != 0 {
        UnitsView::Narrow(unsafe { core::slice::from_raw_parts::<'a, u8>(payload, length) })
    } else {
        UnitsView::Wide(unsafe {
            core::slice::from_raw_parts::<'a, u16>(payload as *const u16, length)
        })
    }
}

pub struct Subject<'a> {
    str_ptr: *const c_void,
    text: Text<'a>,
}

enum Text<'a> {
    /// the runtime engine's haystack; offsets are bytes of it
    Bytes { s: &'a [u8], is_ascii: bool },
    /// the string's own payload; offsets are code units
    Units { len: usize },
}

impl<'a> Subject<'a> {
    /// The engine's view: `s` is the ASCII payload of `str_ptr` when
    /// `is_ascii`, else its transcoded form.
    #[inline]
    pub fn bytes(str_ptr: *const c_void, s: &'a [u8], is_ascii: bool) -> Self {
        Subject {
            str_ptr,
            text: Text::Bytes { s, is_ascii },
        }
    }

    /// The matcher's view: `len` code units of `str_ptr` itself.
    #[inline]
    pub fn units(str_ptr: *const c_void, len: usize) -> Self {
        Subject {
            str_ptr,
            text: Text::Units { len },
        }
    }

    #[inline]
    pub fn str_ptr(&self) -> *const c_void {
        self.str_ptr
    }

    /// The UTF-16 code unit offset of `off` (§22.2.7.8 measures
    /// `index`, `indices` and the callback position in them).
    #[inline]
    pub fn units_at(&self, off: i64) -> i64 {
        match self.text {
            Text::Bytes { s, is_ascii } => byte_to_utf16_units(s, off, is_ascii),
            Text::Units { .. } => off,
        }
    }

    /// The string for `start..end` of the searched text; the subject
    /// itself, shared, when that is all of it.
    ///
    /// # Safety
    ///
    /// `0 <= start <= end <= len`; the subject is live.
    pub unsafe fn span(&self, start: i64, end: i64) -> *mut u8 {
        match self.text {
            Text::Bytes { s, is_ascii } => unsafe {
                span_str(self.str_ptr, s, start, end, is_ascii)
            },
            Text::Units { len } => {
                if start == 0 && end as usize == len {
                    // a `.rodata` literal keeps no count; flags u16 @6
                    let flags = unsafe { *((self.str_ptr as *const u8).add(6) as *const u16) };
                    if flags & FLAG_STATIC_LITERAL == 0 {
                        unsafe { __torajs_rc_inc(self.str_ptr as *mut c_void) };
                    }
                    return self.str_ptr as *mut u8;
                }
                unsafe { __torajs_str_slice(self.str_ptr as *const u8, start, end) }
            }
        }
    }
}
