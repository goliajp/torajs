//! §10.4.3 String-exotic own-property face for the `any`-lane member
//! probe pair (`member_get.rs`'s tag / value cascade).
//!
//! A DYNAMIC key on a string receiver (`s[k]`, `k` a runtime Str) had
//! no arm in that cascade at all: `length` and every canonical index
//! fell through to the builtin-method reify probe, which only knows
//! method names, so both answered a silent `undefined`. The literal
//! form (`s["length"]` pre-lowers to the static length read) and the
//! numeric form (`s[1]` rides the index lane) were fine, which is why
//! it stayed hidden — test262's propertyHelper reads every key
//! dynamically.
//!
//! ## Why the index face interns its answer
//!
//! The pair is borrow-shaped: the lowering's data path takes an owned
//! stake on the payload (`ssa_lower_accessor.rs` emits
//! `any_payload_rc_inc` and then a pure bit-encode) and the consumer
//! releases it. So an arm may only answer a cell that stays alive
//! INDEPENDENTLY of that inc/dec pair — hand out a fresh rc=1 cell and
//! it strands at rc=1 forever (32B per read).
//!
//! An Arr element can be borrowed because the array owns it
//! (`arr_own_pair` reads the slot in place). A string owns no
//! per-character cells, so the answer has to be created — and the
//! shape this codebase already uses for a virtual own property whose
//! value is a cell is the IMMORTAL interned cell
//! (`closure_virtual_pair`'s name cells). A code unit is therefore
//! minted once into a `FLAG_STATIC_LITERAL` Str and borrowed forever.
//!
//! Keyed by UTF-16 code unit, the table covers every answer the face
//! can produce — including the lone surrogate an astral character
//! indexes to. Latin-1 (the whole `"abc"[k]` universe) sits in BSS;
//! the wide plane's slots are allocated on first use, so a program
//! that only ever indexes Latin-1 never pays for them.

use core::ffi::c_void;
use core::sync::atomic::{AtomicU64, Ordering};

use torajs_rc::{AnySlotTag, Tag};

use crate::method_value::{STR_DATA_OFF, STR_LEN_OFF, mint_immortal_str};
use crate::nanbox::{AnyValue, as_void_ptr, is_cell, is_short_str, short_str_bytes, short_str_len};
use crate::nanbox_ffi_materialize::materialize_short_str;

unsafe extern "C" {
    /// torajs-str — UTF-16 code unit at `i`, `-1` when out of range
    /// (§22.1.3.3's `charCodeAt`; resolves Substr views internally).
    fn __torajs_str_any_char_code_at(s: *const u8, i: i64) -> i64;
    /// torajs-str — release the temp materialized from a ShortStr.
    fn __torajs_str_drop(s: *mut c_void);
}

/// Code units `0..=0xFF` — a Latin-1 payload, one byte. Everything
/// above lives in the lazily allocated wide plane below.
const WIDE_BASE: usize = 0x100;
const WIDE_SLOTS: usize = 0x1_0000 - WIDE_BASE;

static LATIN1_CHAR_CELLS: [AtomicU64; WIDE_BASE] = [const { AtomicU64::new(0) }; WIDE_BASE];
static WIDE_CHAR_CELLS: AtomicU64 = AtomicU64::new(0);

/// The wide plane's slot array, allocated on first non-Latin-1 use.
/// A losing racer frees its own allocation, so exactly one table is
/// ever installed.
fn wide_table() -> *const AtomicU64 {
    let p = WIDE_CHAR_CELLS.load(Ordering::Acquire);
    if p != 0 {
        return p as *const AtomicU64;
    }
    let layout = core::alloc::Layout::array::<AtomicU64>(WIDE_SLOTS).unwrap();
    // SAFETY: non-zero layout; `AtomicU64`'s all-zero bit pattern is
    // the valid `AtomicU64::new(0)`, so zeroed memory is initialized.
    let fresh = unsafe { std::alloc::alloc_zeroed(layout) } as *mut AtomicU64;
    match WIDE_CHAR_CELLS.compare_exchange(0, fresh as u64, Ordering::AcqRel, Ordering::Acquire) {
        Ok(_) => fresh as *const AtomicU64,
        Err(won) => {
            // SAFETY: `fresh` came from this allocator with `layout`
            // and was never published.
            unsafe { std::alloc::dealloc(fresh as *mut u8, layout) };
            won as *const AtomicU64
        }
    }
}

/// Mint an immortal one-code-unit Str: Latin-1 payload (one byte) for
/// `cu <= 0xFF`, UTF-16 LE (two bytes) above it. `length` is one CODE
/// UNIT either way. Wide twin of `method_value::mint_immortal_str`,
/// which hard-codes the Latin-1 encoding.
fn mint_immortal_code_unit(cu: u16) -> *mut u8 {
    if cu <= 0xFF {
        return mint_immortal_str(&[cu as u8]);
    }
    // SAFETY: fresh allocation sized for the 16-byte Str prefix + the
    // two payload bytes, fully initialized below.
    unsafe {
        let layout = core::alloc::Layout::from_size_align(STR_DATA_OFF + 2, 8).unwrap();
        let cell = std::alloc::alloc_zeroed(layout);
        *(cell as *mut u32) = 1;
        *(cell.add(4) as *mut u16) = Tag::Str as u16;
        // No IS_LATIN1 — the payload is UTF-16 LE.
        *(cell.add(6) as *mut u16) = torajs_rc::FLAG_STATIC_LITERAL;
        *(cell.add(STR_LEN_OFF) as *mut u32) = 1;
        core::ptr::copy_nonoverlapping(cu.to_le_bytes().as_ptr(), cell.add(STR_DATA_OFF), 2);
        cell
    }
}

/// The interned cell for one code unit. A race mints twice and one
/// pointer is dropped on the floor — the same bounded, once-per-slot
/// window `method_value::builtin_method_name_cell` accepts, and string
/// identity is not observable in the language.
fn char_cell(cu: u16) -> *mut u8 {
    let idx = cu as usize;
    let slot: &AtomicU64 = if idx < WIDE_BASE {
        &LATIN1_CHAR_CELLS[idx]
    } else {
        // SAFETY: `wide_table` hands back `WIDE_SLOTS` initialized
        // slots; `idx - WIDE_BASE` is in range for a u16 key.
        unsafe { &*wide_table().add(idx - WIDE_BASE) }
    };
    let p = slot.load(Ordering::Relaxed);
    if p != 0 {
        return p as *mut u8;
    }
    let cell = mint_immortal_code_unit(cu);
    slot.store(cell as u64, Ordering::Relaxed);
    cell
}

/// UTF-16 code unit at `idx`, `None` when out of range.
///
/// # Safety
/// `recv` is a ShortStr or a live `Tag::Str` cell (plain or Substr).
unsafe fn code_unit_at(recv: AnyValue, idx: u64) -> Option<u16> {
    let i = i64::try_from(idx).ok()?;
    if is_short_str(recv) {
        let len = short_str_len(recv) as usize;
        let bytes = short_str_bytes(recv);
        let payload = &bytes[..len];
        if payload.iter().all(|b| *b < 0x80) {
            // ASCII: byte index == code-unit index.
            return payload.get(idx as usize).map(|b| *b as u16);
        }
        // A non-ASCII ShortStr payload is UTF-8, whose code-unit view
        // only exists on the heap layout — materialize, read, release
        // (the index lane's own shape).
        let parent = unsafe { materialize_short_str(recv) };
        let cu = unsafe { __torajs_str_any_char_code_at(parent, i) };
        unsafe { __torajs_str_drop(parent as *mut c_void) };
        return u16::try_from(cu).ok();
    }
    if !is_cell(recv) {
        return None;
    }
    let cu = unsafe { __torajs_str_any_char_code_at(as_void_ptr(recv) as *const u8, i) };
    u16::try_from(cu).ok()
}

/// §10.4.3 `[[GetOwnProperty]]` on a string receiver, in the probe
/// pair's `(tag, value)` shape. `None` = not an own-domain key, so the
/// caller falls through to its builtin-method reify tail — which is
/// also the answer for a canonical index PAST the end, since §10.4.3
/// makes that a genuine absence and the chain gets to speak.
///
/// # Safety
/// `recv` is a ShortStr or a live `Tag::Str` cell (a StringWrapper
/// passes its inner cell); `key` is a live Str cell.
pub(crate) unsafe fn str_own_pair(recv: AnyValue, key: *const c_void) -> Option<(u64, u64)> {
    if unsafe { crate::prop_has::key_is(key, b"length") } {
        // Immediates only — reuse the single source of truth for a
        // string's code-unit count rather than re-mirroring layouts.
        let av = unsafe { crate::len_get::__torajs_any_length_get(recv) };
        return Some((
            crate::nanbox_encode::__torajs_anyv_unbox_tag(av) as u64,
            crate::nanbox_encode::__torajs_anyv_unbox_value(av) as u64,
        ));
    }
    let idx = unsafe { crate::prop_has::canonical_index(key) }?;
    let cu = unsafe { code_unit_at(recv, idx) }?;
    Some((AnySlotTag::Heap as u64, char_cell(cu) as u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latin1_code_unit_cell_is_interned_and_immortal() {
        let a = char_cell(b'a' as u16);
        let b = char_cell(b'a' as u16);
        assert_eq!(a, b, "same code unit must reuse the interned cell");
        // SAFETY: `char_cell` hands back a live Str cell.
        unsafe {
            assert_eq!(*(a.add(STR_LEN_OFF) as *const u32), 1, "one code unit");
            assert_eq!(*(a.add(STR_DATA_OFF)), b'a', "Latin-1 payload byte");
            assert_ne!(
                *(a.add(6) as *const u16) & torajs_rc::FLAG_STATIC_LITERAL,
                0,
                "must be immortal — the probe pair only borrows it"
            );
        }
    }

    #[test]
    fn wide_code_unit_cell_is_utf16_le() {
        let cell = char_cell(0x4E2D);
        assert_eq!(cell, char_cell(0x4E2D), "wide plane interns too");
        // SAFETY: `char_cell` hands back a live Str cell.
        unsafe {
            assert_eq!(*(cell.add(STR_LEN_OFF) as *const u32), 1, "one code unit");
            assert_eq!(*(cell.add(STR_DATA_OFF)), 0x2D, "UTF-16 LE low byte");
            assert_eq!(*(cell.add(STR_DATA_OFF + 1)), 0x4E, "UTF-16 LE high byte");
        }
    }

    #[test]
    fn latin1_and_wide_planes_do_not_collide() {
        assert_ne!(char_cell(0xFF), char_cell(0x100));
    }
}
