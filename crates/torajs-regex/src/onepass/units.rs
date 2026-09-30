//! What a consuming instruction admits, and the segments of the unit
//! space over which every instruction of a program gives one answer.
//!
//! A unit is what one step of the walk reads: a Latin-1 code unit, a
//! UTF-16 code unit, or under `u` / `v` over UTF-16 a code point (a
//! surrogate pair read as one). Every unit up to `0xFF` is a segment
//! of its own. Above that a segment runs between two consecutive
//! points where some instruction's answer can change: the line
//! terminators `.` skips, the units that share a canonical form with
//! a Latin-1 one, a literal, and the bounds of a class's ranges and
//! property tables.

use alloc::vec;
use alloc::vec::Vec;

use super::fold::{SHARED_HIGH_UNITS, canonicalize};
use super::ir::{UNIT_MAX_NARROW, UNIT_MAX_PAIRS, UNIT_MAX_WIDE, is_line_terminator};
use crate::parser::{RE_FLAG_I, RE_FLAG_S};
use crate::program::{Inst, Op, Program};

/// The segments of `prog` over one payload, sorted and covering every
/// unit the walk can read.
pub(super) fn segments(prog: &Program, wide: bool, pairs: bool) -> Vec<(u32, u32)> {
    let mut segs: Vec<(u32, u32)> = (0..=UNIT_MAX_NARROW).map(|u| (u, u)).collect();
    if !wide {
        return segs;
    }
    let max = if pairs { UNIT_MAX_PAIRS } else { UNIT_MAX_WIDE };
    let mut cuts: Vec<u32> = vec![UNIT_MAX_NARROW + 1];
    let mut cut = |lo: u32, hi: u32| {
        cuts.push(lo);
        cuts.push(hi + 1);
    };
    for &p in [0x2028, 0x2029].iter().chain(&SHARED_HIGH_UNITS) {
        cut(p, p);
    }
    for ins in &prog.insts {
        match Op::from_u8(ins.op) {
            Some(Op::Char) => cut(ins.cp(), ins.cp()),
            Some(Op::Class) => {
                let class = &prog.classes[ins.a as usize];
                let tables = class.u_prop_tables.iter().flat_map(|t| t.iter());
                for r in tables.chain(&class.owned_ranges) {
                    cut(r.lo as u32, r.hi as u32);
                }
            }
            _ => {}
        }
    }
    cuts.retain(|&c| c > UNIT_MAX_NARROW && c <= max);
    cuts.sort_unstable();
    cuts.dedup();
    for (i, &lo) in cuts.iter().enumerate() {
        let hi = cuts.get(i + 1).map_or(max, |&next| next - 1);
        segs.push((lo, hi));
    }
    segs
}

/// Whether `ins` consumes the unit `u` (§22.2.2.7.3: a `.` skips the
/// four LineTerminators unless `s`; under `i` a character matches
/// what canonicalizes with it, and a class what canonicalizes with a
/// member; a class is tested against the unit, which under `u` / `v`
/// is the code point, a lone surrogate included).
pub(super) fn admits(prog: &Program, ins: Inst, u: u32, unicode: bool) -> bool {
    let ci = ins.pad as u8 & RE_FLAG_I != 0;
    match Op::from_u8(ins.op) {
        Some(Op::Char) => {
            let c = ins.cp();
            c == u || (ci && canonicalize(c, unicode) == canonicalize(u, unicode))
        }
        Some(Op::AnyChar) => ins.pad as u8 & RE_FLAG_S != 0 || !is_line_terminator(u),
        Some(Op::Class) => {
            let class = &prog.classes[ins.a as usize];
            // membership of the class as written; `negate` is applied
            // after the fold, as CharacterSetMatcher's `invert` is
            let member = |m: u32| class.test_cp(m as i32) != class.negate;
            let found = member(u)
                || (ci && {
                    let cu = canonicalize(u, unicode);
                    (0..=UNIT_MAX_NARROW).any(|m| member(m) && canonicalize(m, unicode) == cu)
                });
            found != class.negate
        }
        _ => false,
    }
}
