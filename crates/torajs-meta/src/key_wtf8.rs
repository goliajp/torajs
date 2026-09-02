//! A property key's Str cell as WTF-8 bytes — the spelling the
//! compiler bakes struct field names in, so a key can be compared
//! against a layout / accessor / method name byte for byte.
//!
//! The Str payload is Latin-1 or UTF-16 code units (never UTF-8);
//! reading it as name bytes compared `é` (`E9`) against `C3 A9`
//! and a UTF-16 key's low bytes against anything (rotation 559).
//! A Latin-1 payload that is pure ASCII is already its WTF-8
//! spelling and is borrowed; every other key is transcoded by
//! torajs-str's kernel into an inline buffer, or the heap when the
//! spelling is longer than that.

use core::ffi::c_void;

unsafe extern "C" {
    /// torajs-str — write the key's WTF-8 spelling into
    /// `buf[..cap]`, answering the full length (a short buffer is
    /// filled up to `cap` and the caller retries).
    fn __torajs_str_wtf8_into(s: *const u8, buf: *mut u8, cap: u32) -> u32;
}

const STR_LEN_OFF: usize = 8;
const STR_DATA_OFF: usize = 16;
const HDR_FLAGS_OFF: usize = 6;
/// torajs-str `STR_FLAG_IS_LATIN1` mirror.
const STR_FLAG_IS_LATIN1: u16 = 0x0002;
/// torajs-str `FLAG_SUBSTR_INLINE | FLAG_SUBSTR_VIEW` mirror — a
/// Substr cell keeps no payload of its own at the owned offsets.
const STR_FLAGS_SUBSTR: u16 = (1 << 0) | (1 << 10);
const INLINE_CAP: usize = 64;

pub(crate) enum KeyWtf8 {
    /// The key's own payload — Latin-1, pure ASCII.
    Borrowed(*const u8, u32),
    Inline([u8; INLINE_CAP], u32),
    Heap(Vec<u8>),
}

impl KeyWtf8 {
    /// # Safety
    /// `key` is a live Str (or Substr) cell.
    pub(crate) unsafe fn of(key: *const c_void) -> KeyWtf8 {
        let p = key.cast::<u8>();
        let flags = unsafe { p.add(HDR_FLAGS_OFF).cast::<u16>().read() };
        if flags & STR_FLAGS_SUBSTR == 0 && flags & STR_FLAG_IS_LATIN1 != 0 {
            let len = unsafe { p.add(STR_LEN_OFF).cast::<u32>().read() };
            let bytes = unsafe { core::slice::from_raw_parts(p.add(STR_DATA_OFF), len as usize) };
            if bytes.is_ascii() {
                return KeyWtf8::Borrowed(bytes.as_ptr(), len);
            }
        }
        let mut inline = [0u8; INLINE_CAP];
        let n = unsafe { __torajs_str_wtf8_into(p, inline.as_mut_ptr(), INLINE_CAP as u32) };
        if n as usize <= INLINE_CAP {
            return KeyWtf8::Inline(inline, n);
        }
        let mut heap = vec![0u8; n as usize];
        unsafe { __torajs_str_wtf8_into(p, heap.as_mut_ptr(), n) };
        KeyWtf8::Heap(heap)
    }

    #[inline]
    pub(crate) fn as_bytes(&self) -> &[u8] {
        match self {
            // SAFETY: `Borrowed` was cut from a live key's payload.
            KeyWtf8::Borrowed(p, n) => unsafe { core::slice::from_raw_parts(*p, *n as usize) },
            KeyWtf8::Inline(buf, n) => &buf[..*n as usize],
            KeyWtf8::Heap(v) => v,
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
