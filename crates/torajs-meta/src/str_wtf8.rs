//! A Str cell's content as WTF-8 bytes — the spelling the compiler
//! bakes names in and the one an output buffer is written in, so a
//! cell can be compared against a name, or spent as text, byte for
//! byte. `torajs_rc::str_wtf8` twin: this crate takes no Cargo dep
//! on torajs-rc by design, so it keeps the boundary itself.
//!
//! The Str payload is Latin-1 or UTF-16 code units (never UTF-8);
//! reading it as name bytes compared `é` (`E9`) against `C3 A9`
//! (rotation 559), and a UTF-16 key's `len` code units read as `len`
//! BYTES spelled half the key (`"慮敭ab"` answered the `name`
//! descriptor, rotation 560). A Latin-1 payload that is pure ASCII
//! is already its WTF-8 spelling and is borrowed; every other cell
//! is transcoded by torajs-str's kernel into an inline buffer, or
//! the heap when the spelling is longer than that.

use core::ffi::c_void;

unsafe extern "C" {
    /// torajs-str — write the key's WTF-8 spelling into
    /// `buf[..cap]`, answering the full length (a short buffer is
    /// filled up to `cap` and the caller retries).
    fn __torajs_str_wtf8_into(s: *const u8, buf: *mut u8, cap: u32) -> u32;
}

const STR_LEN_OFF: usize = 8;
const STR_DATA_OFF: usize = 16;
const HDR_TYPE_TAG_OFF: usize = 4;
const HDR_FLAGS_OFF: usize = 6;
/// torajs-rc `Tag::Symbol` mirror.
const TAG_SYMBOL: u16 = 7;
/// torajs-str `STR_FLAG_IS_LATIN1` mirror.
const STR_FLAG_IS_LATIN1: u16 = 0x0002;
/// torajs-str `FLAG_SUBSTR_INLINE | FLAG_SUBSTR_VIEW` mirror — a
/// Substr cell keeps no payload of its own at the owned offsets.
const STR_FLAGS_SUBSTR: u16 = (1 << 0) | (1 << 10);
const INLINE_CAP: usize = 64;

pub(crate) enum StrWtf8 {
    /// The key's own payload — Latin-1, pure ASCII.
    Borrowed(*const u8, u32),
    Inline([u8; INLINE_CAP], u32),
    Heap(Vec<u8>),
}

impl StrWtf8 {
    /// A Symbol key (§6.1.7's other kind) names no baked field,
    /// accessor or method, and its cell keeps a pointer where a Str
    /// keeps `len`: it answers a spelling no WTF-8 name can equal (a
    /// lone `0xFF` byte) instead of being read as a Str.
    ///
    /// # Safety
    /// `cell` is a live Str, Substr or Symbol cell.
    pub(crate) unsafe fn of(cell: *const c_void) -> StrWtf8 {
        let p = cell.cast::<u8>();
        if unsafe { p.add(HDR_TYPE_TAG_OFF).cast::<u16>().read() } == TAG_SYMBOL {
            let mut never = [0u8; INLINE_CAP];
            never[0] = 0xFF;
            return StrWtf8::Inline(never, 1);
        }
        let flags = unsafe { p.add(HDR_FLAGS_OFF).cast::<u16>().read() };
        if flags & STR_FLAGS_SUBSTR == 0 && flags & STR_FLAG_IS_LATIN1 != 0 {
            let len = unsafe { p.add(STR_LEN_OFF).cast::<u32>().read() };
            let bytes = unsafe { core::slice::from_raw_parts(p.add(STR_DATA_OFF), len as usize) };
            if bytes.is_ascii() {
                return StrWtf8::Borrowed(bytes.as_ptr(), len);
            }
        }
        let mut inline = [0u8; INLINE_CAP];
        let n = unsafe { __torajs_str_wtf8_into(p, inline.as_mut_ptr(), INLINE_CAP as u32) };
        if n as usize <= INLINE_CAP {
            return StrWtf8::Inline(inline, n);
        }
        let mut heap = vec![0u8; n as usize];
        unsafe { __torajs_str_wtf8_into(p, heap.as_mut_ptr(), n) };
        StrWtf8::Heap(heap)
    }

    #[inline]
    pub(crate) fn as_bytes(&self) -> &[u8] {
        match self {
            // SAFETY: `Borrowed` was cut from a live key's payload.
            StrWtf8::Borrowed(p, n) => unsafe { core::slice::from_raw_parts(*p, *n as usize) },
            StrWtf8::Inline(buf, n) => &buf[..*n as usize],
            StrWtf8::Heap(v) => v,
        }
    }

    #[inline]
    pub(crate) fn as_ptr(&self) -> *const u8 {
        self.as_bytes().as_ptr()
    }

    #[inline]
    pub(crate) fn len(&self) -> u32 {
        self.as_bytes().len() as u32
    }
}
