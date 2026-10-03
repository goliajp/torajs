//! The replace-callback walk over a subject's own code units, for a
//! literal with a compiled matcher for the subject's encoding:
//! offsets are code units already, a span is a slice of the subject,
//! and the result is built in code units and stored in the narrowest
//! encoding that holds it.

use alloc::vec::Vec;
use core::ffi::c_void;

use super::exec_find::{run_matcher, search_start};
use super::str_helpers::subject::{Subject, UnitsView, str_units_view};
use super::{__torajs_str_drop, MatcherFn, RegExp, STR_HDR_SIZE, str_from_bytes, str_units};
use crate::flags::unicode_mode;
use crate::onepass::ir::{is_high_surrogate, is_low_surrogate};

unsafe extern "C" {
    /// torajs-str — a block of `len` code units, Latin-1 or UTF-16
    fn __torajs_str_alloc_pooled_enc(len: u64, is_latin1: i64) -> *mut u8;
}

/// The walk over a subject's own code units through the literal's
/// matcher for its encoding.
pub(super) struct UnitsWalk<'a> {
    pub str_ptr: *const c_void,
    pub re: &'a RegExp,
    pub re_cell: *mut RegExp,
    pub view: UnitsView<'a>,
    pub matcher: MatcherFn,
    pub global: bool,
    pub sticky_once: bool,
}

impl UnitsWalk<'_> {
    /// # Safety
    ///
    /// `view` is the payload of the live `str_ptr`, `matcher` was
    /// synthesized for its encoding, and `re_cell` is the live cell
    /// of `re`.
    pub(super) unsafe fn run(
        &self,
        mut invoke: impl FnMut(*mut c_void, &[i64], &Subject, i64) -> Result<*mut c_void, ()>,
    ) -> *mut c_void {
        let view = &self.view;
        let len = view.len() as i64;
        let subject = Subject::units(self.str_ptr, view.len());
        let stride = self.re.matcher_stride as usize;
        let mut out = UnitBuf::for_subject(view);
        let mut pos = if self.sticky_once {
            search_start(view, self.re.flags, self.re.last_index_i64())
        } else {
            0
        };
        let mut matched = false;
        // units of the subject already in `out`
        let mut copied = 0usize;
        while pos <= len {
            let Some(m) = (unsafe { run_matcher(self.matcher, stride, view, pos) }) else {
                break;
            };
            matched = true;
            if self.sticky_once {
                // §22.2.7.2 step 15, before the callback runs (step 14)
                unsafe { (*self.re_cell).set_last_index_num(m.end as f64) };
            }
            out.push(view, copied, m.start as usize);
            let match_str = unsafe { subject.span(m.start, m.end) } as *mut c_void;
            let ret = invoke(match_str, m.saves(), &subject, m.start);
            unsafe { __torajs_str_drop(match_str) };
            let Ok(ret_str) = ret else {
                // pending throw inside the callback: the caller's
                // throw check discards this placeholder
                let s = unsafe { str_units(self.str_ptr) };
                return unsafe { str_from_bytes(&s) as *mut c_void };
            };
            if !ret_str.is_null() {
                let ret_view = unsafe { str_units_view(ret_str) };
                out.push(&ret_view, 0, ret_view.len());
                unsafe { __torajs_str_drop(ret_str) };
            }
            copied = m.end as usize;
            pos = if m.end == m.start {
                m.end + advance_empty(view, self.re.flags, m.end)
            } else {
                m.end
            };
            if !self.global {
                break;
            }
        }
        if self.sticky_once && !matched {
            // §22.2.7.2 step 12.a.i
            unsafe { (*self.re_cell).set_last_index_num(0.0) };
        }
        out.push(view, copied, view.len());
        unsafe { out.into_str() }
    }
}

/// Units an empty match at `at` advances by (§22.2.6.11 step 11.c.iv
/// AdvanceStringIndex): a surrogate pair is one step under `u` / `v`.
pub(super) fn advance_empty(view: &UnitsView<'_>, flags: u8, at: i64) -> i64 {
    let at = at as usize;
    if view.wide()
        && unicode_mode(flags)
        && at + 1 < view.len()
        && is_high_surrogate(view.unit(at))
        && is_low_surrogate(view.unit(at + 1))
    {
        2
    } else {
        1
    }
}

/// A result built in code units: Latin-1 until a unit past U+00FF
/// arrives, UTF-16 from then on.
pub(super) enum UnitBuf {
    Narrow(Vec<u8>),
    Wide(Vec<u16>),
}

impl UnitBuf {
    pub(super) fn for_subject(view: &UnitsView<'_>) -> Self {
        match view {
            UnitsView::Narrow(s) => UnitBuf::Narrow(Vec::with_capacity(s.len() + 16)),
            UnitsView::Wide(s) => UnitBuf::Wide(Vec::with_capacity(s.len() + 16)),
        }
    }

    /// Append units `from..to` of `view`.
    pub(super) fn push(&mut self, view: &UnitsView<'_>, from: usize, to: usize) {
        match (view, &mut *self) {
            (UnitsView::Narrow(s), UnitBuf::Narrow(out)) => out.extend_from_slice(&s[from..to]),
            (UnitsView::Narrow(s), UnitBuf::Wide(out)) => {
                out.extend(s[from..to].iter().map(|&b| u16::from(b)));
            }
            (UnitsView::Wide(s), UnitBuf::Wide(out)) => out.extend_from_slice(&s[from..to]),
            (UnitsView::Wide(s), UnitBuf::Narrow(out)) => {
                let mut wide: Vec<u16> = Vec::with_capacity(out.len() + s.len() + 16);
                wide.extend(out.iter().map(|&b| u16::from(b)));
                wide.extend_from_slice(&s[from..to]);
                *self = UnitBuf::Wide(wide);
            }
        }
    }

    /// The string of these units, in the narrowest encoding that
    /// holds them.
    ///
    /// # Safety
    ///
    /// Calls the torajs-str allocator; the answer is a fresh
    /// refcount-1 Str.
    pub(super) unsafe fn into_str(self) -> *mut c_void {
        let narrow = match self {
            UnitBuf::Narrow(bytes) => bytes,
            UnitBuf::Wide(units) if units.iter().all(|&u| u <= 0xFF) => {
                units.iter().map(|&u| u as u8).collect()
            }
            UnitBuf::Wide(units) => unsafe {
                let block = __torajs_str_alloc_pooled_enc(units.len() as u64, 0);
                core::ptr::copy_nonoverlapping(
                    units.as_ptr(),
                    block.add(STR_HDR_SIZE) as *mut u16,
                    units.len(),
                );
                return block as *mut c_void;
            },
        };
        unsafe {
            let block = __torajs_str_alloc_pooled_enc(narrow.len() as u64, 1);
            core::ptr::copy_nonoverlapping(narrow.as_ptr(), block.add(STR_HDR_SIZE), narrow.len());
            block as *mut c_void
        }
    }
}
