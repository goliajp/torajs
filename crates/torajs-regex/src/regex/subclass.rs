//! RegExp-subclass instance mint + ctor `super(pattern)` (RFC
//! 20260730-exotic-backed-class-instance blade 2).
//!
//! `class C extends RegExp` mints a REAL RegExp cell — the whole
//! test/exec/match surface rides the existing arms. The mint's
//! default is the empty pattern (`new RegExp()` ≡ `/(?:)/`), so a
//! bare `super()` is a no-op; `super(pattern)` recompiles per
//! §22.2.3.1 — a RegExp argument copies its source and flags
//! (step 5), anything else runs ToString — and the compiled innards
//! SWAP into the already-minted instance (the cell pointer is the
//! instance's identity: header, flag bit and side-table entry stay
//! put; the discarded innards leave through the ordinary drop).
//! An invalid pattern raises the §22.2.3.1 SyntaxError and leaves
//! the default innards in place (throw-shape answer, no fresh
//! reference).

use alloc::vec::Vec;
use core::ffi::c_void;

use super::compile::{compile_bytes, flag_letters};
use super::{__torajs_throw_syntax_error, RegExp, TAG_REGEX, str_slice};

/// `torajs_rc::FLAG_SUBCLASSED` mirror (flags bit 0, blade 0).
const FLAG_SUBCLASSED: u16 = 1;
/// `torajs_rc::AnySlotTag::Heap` mirror.
const ANY_HEAP: i64 = 4;
/// `torajs_rc::AnySlotTag::Undefined` mirror (`compile_any` twin).
const ANY_UNDEF_TAG: i64 = 5;

unsafe extern "C" {
    /// torajs-meta — blade-0 identity registration / classmeta proto.
    fn __torajs_subclass_register(cell: *mut c_void, class_tag: i64, proto_cell: u64);
    fn __torajs_proto_cell_raw(tag: i64) -> u64;
    /// torajs-anyvalue — NaN-box encode / decode + ToString.
    /// `cell_ptr` is the borrow-safe heap probe (0 for every
    /// immediate INCLUDING ShortStr) — the legacy `unbox_value`
    /// shim MATERIALIZES a ShortStr into an owned Str the probe
    /// would leak (~32B per super(pattern) call; the churn probe
    /// caught it). `unbox_value` stays for `this_av`, which is
    /// always a real heap box.
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_anyv_cell_ptr(v: u64) -> i64;
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    fn __torajs_anyv_to_str(v: u64) -> *mut c_void;
    /// torajs-str / torajs-rc — temp release + owned answer.
    fn __torajs_str_drop(s: *mut c_void);
    fn __torajs_rc_inc(p: *mut c_void);
}

/// Mint a RegExp-subclass instance — the empty-pattern default,
/// carrying the class identity. Answers the boxed instance.
///
/// # Safety
/// `class_tag` is the class's registered tag.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_subclass_alloc(class_tag: i64) -> u64 {
    let raw = compile_bytes(b"(?:)".to_vec(), Vec::new());
    unsafe {
        let re = raw as *mut RegExp;
        (*re).header.flags |= FLAG_SUBCLASSED;
        let proto_cell = __torajs_proto_cell_raw(class_tag);
        __torajs_subclass_register(raw, class_tag, proto_cell);
        __torajs_anyv_box_from_pair(ANY_HEAP, raw as i64)
    }
}

/// `super(pattern)` inside a RegExp-subclass ctor — see module doc.
/// Answers a fresh owned reference on the instance; the SyntaxError
/// path answers the borrowed box un-inc'd for the throw-check to
/// divert past.
///
/// # Safety
/// `this_av` is the factory's freshly minted subclass instance boxed
/// ANY_HEAP (or any non-cell box, answered back unchanged); `pat_av`
/// is any boxed value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_subclass_super(this_av: u64, pat_av: u64) -> u64 {
    unsafe {
        let this = __torajs_anyv_unbox_value(this_av) as *mut RegExp;
        if this.is_null() {
            return this_av;
        }
        // §22.2.3.1 step 5 vs step 6 — a RegExp pattern copies
        // source + flags; an undefined pattern is the empty pattern;
        // everything else is ToString'd (a pending throw from
        // ToString leaves the default innards).
        let (pat, fl) = if __torajs_anyv_unbox_tag(pat_av) == ANY_UNDEF_TAG {
            (b"(?:)".to_vec(), Vec::new())
        } else {
            match pattern_as_regex(pat_av) {
                Some(re) => ((*re).src_bytes.clone(), flag_letters((*re).flags)),
                None => {
                    let s = __torajs_anyv_to_str(pat_av);
                    if s.is_null() {
                        return this_av;
                    }
                    let bytes = str_slice(s);
                    __torajs_str_drop(s);
                    (bytes, Vec::new())
                }
            }
        };
        recompile_into(this_av, this, pat, fl)
    }
}

/// `super(pattern, flags)` — the §22.2.4.1 two-argument form (RFC
/// 20260815 residue close; the last loud arm of the exotic rest
/// ctor). Flags resolution mirrors `compile_any`: an undefined flags
/// argument keeps a RegExp pattern's own letters (step 5), anything
/// else is ToString'd; an unknown / duplicate letter funnels through
/// `compile_bytes`'s rejected stub into the §22.2.3.1 SyntaxError.
/// Same answer contract as the one-argument form.
///
/// # Safety
/// `this_av` as in [`__torajs_regex_subclass_super`]; `pat_av` /
/// `flags_av` are any boxed values.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_subclass_super_flags(
    this_av: u64,
    pat_av: u64,
    flags_av: u64,
) -> u64 {
    unsafe {
        let this = __torajs_anyv_unbox_value(this_av) as *mut RegExp;
        if this.is_null() {
            return this_av;
        }
        let explicit_flags: Option<Vec<u8>> = if __torajs_anyv_unbox_tag(flags_av) == ANY_UNDEF_TAG
        {
            None
        } else {
            let s = __torajs_anyv_to_str(flags_av);
            if s.is_null() {
                return this_av;
            }
            let b = str_slice(s);
            __torajs_str_drop(s);
            Some(b)
        };
        let (pat, re_fl) = if __torajs_anyv_unbox_tag(pat_av) == ANY_UNDEF_TAG {
            (b"(?:)".to_vec(), Vec::new())
        } else if let Some(re) = pattern_as_regex(pat_av) {
            ((*re).src_bytes.clone(), flag_letters((*re).flags))
        } else {
            let s = __torajs_anyv_to_str(pat_av);
            if s.is_null() {
                return this_av;
            }
            let b = str_slice(s);
            __torajs_str_drop(s);
            (b, Vec::new())
        };
        let fl = explicit_flags.unwrap_or(re_fl);
        recompile_into(this_av, this, pat, fl)
    }
}

/// Shared tail of the `super(…)` forms: compile, reject with the
/// §22.2.3.1 SyntaxError (borrowed answer, un-inc'd for the
/// throw-check to divert past), or swap the compiled innards into
/// the instance — identity (header: rc / tag / FLAG_SUBCLASSED)
/// stays with `this`; the discarded innards leave through the
/// ordinary drop.
///
/// # Safety
/// `this` is the live cell `this_av` boxes.
unsafe fn recompile_into(this_av: u64, this: *mut RegExp, pat: Vec<u8>, fl: Vec<u8>) -> u64 {
    unsafe {
        let fresh = compile_bytes(pat, fl) as *mut RegExp;
        if (*fresh).rejected != 0 {
            __torajs_throw_syntax_error(c"Invalid regular expression".as_ptr() as *const u8);
            super::lifecycle::__torajs_regex_drop(fresh as *mut c_void);
            return this_av;
        }
        core::ptr::swap(this, fresh);
        core::mem::swap(&mut (*this).header, &mut (*fresh).header);
        // `fresh` now holds the discarded innards and this call's
        // only reference — release through the ordinary drop.
        super::lifecycle::__torajs_regex_drop(fresh as *mut c_void);
        __torajs_rc_inc(this as *mut c_void);
    }
    this_av
}

/// Decode a boxed value down to a live RegExp cell, else `None`
/// (shared with the compile_any §22.2.3.1 entry).
pub(crate) unsafe fn pattern_as_regex(v: u64) -> Option<*mut RegExp> {
    unsafe {
        let p = __torajs_anyv_cell_ptr(v) as *mut RegExp;
        if p.is_null() || (*p).header.type_tag != TAG_REGEX {
            return None;
        }
        Some(p)
    }
}
