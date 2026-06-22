//! Regex bytecode program — port of `runtime_regex.c` L1032-1152.
//!
//! Flat instruction array + interned CharClass table + recursive
//! sub-programs for lookahead/lookbehind bodies. Produced by
//! [`crate::compiler::compile`]; consumed by the future VM
//! (P6.2-d).
//!
//! ## Instruction layout (12 bytes, packed)
//!
//! ```text
//!   op : u8     opcode (see Op)
//!   ch : u8     OP_CHAR literal
//!   pad: u16
//!   a  : i32    OP_CLASS=cls_idx, OP_JMP=target, OP_SPLIT=t1,
//!               OP_SAVE=slot, OP_LOOK*=sub_prog_idx, OP_BACKREF=cap_idx
//!   b  : i32    OP_SPLIT=t2
//! ```
//!
//! Multiple `Inst`s form a Thompson NFA: thread fork (`SPLIT`),
//! unconditional hop (`JMP`), input consume (`CHAR` / `ANYCHAR` /
//! `CLASS`), zero-width (`ANCHOR_B/E` / `WBOUND` / `NWBOUND` /
//! `LOOKAHEAD` / `NEG_LOOKAHEAD` / `LOOKBEHIND` / `NEG_LOOKBEHIND`),
//! capture-slot write (`SAVE`), accept (`MATCH`), and capture
//! re-consume (`BACKREF`).

use crate::charclass::CharClass;
use alloc::{boxed::Box, vec::Vec};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Op {
    Char = 1,
    AnyChar = 2,
    Class = 3,
    AnchorB = 4,
    AnchorE = 5,
    WBound = 6,
    NWBound = 7,
    Jmp = 8,
    Split = 9,
    Match = 10,
    Save = 11,
    Lookahead = 12,
    NegLookahead = 13,
    Lookbehind = 14,
    NegLookbehind = 15,
    Backref = 16,
}

impl Op {
    pub fn from_u8(b: u8) -> Option<Op> {
        match b {
            1 => Some(Op::Char),
            2 => Some(Op::AnyChar),
            3 => Some(Op::Class),
            4 => Some(Op::AnchorB),
            5 => Some(Op::AnchorE),
            6 => Some(Op::WBound),
            7 => Some(Op::NWBound),
            8 => Some(Op::Jmp),
            9 => Some(Op::Split),
            10 => Some(Op::Match),
            11 => Some(Op::Save),
            12 => Some(Op::Lookahead),
            13 => Some(Op::NegLookahead),
            14 => Some(Op::Lookbehind),
            15 => Some(Op::NegLookbehind),
            16 => Some(Op::Backref),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Inst {
    pub op: u8,
    pub ch: u8,
    pub pad: u16,
    pub a: i32,
    pub b: i32,
}

impl Inst {
    /// Construct an instruction with `ch=0 a=0 b=0`. For ops whose
    /// only payload is the opcode (`AnyChar`, `AnchorB/E`, `WBound`,
    /// `NWBound`, `Match`).
    pub fn simple(op: Op) -> Self {
        Self {
            op: op as u8,
            ch: 0,
            pad: 0,
            a: 0,
            b: 0,
        }
    }

    pub fn char_lit(ch: u8) -> Self {
        Self {
            op: Op::Char as u8,
            ch,
            pad: 0,
            a: 0,
            b: 0,
        }
    }

    pub fn class_ref(cls_idx: i32) -> Self {
        Self {
            op: Op::Class as u8,
            ch: 0,
            pad: 0,
            a: cls_idx,
            b: 0,
        }
    }

    pub fn jmp(target: i32) -> Self {
        Self {
            op: Op::Jmp as u8,
            ch: 0,
            pad: 0,
            a: target,
            b: 0,
        }
    }

    pub fn split(a: i32, b: i32) -> Self {
        Self {
            op: Op::Split as u8,
            ch: 0,
            pad: 0,
            a,
            b,
        }
    }

    pub fn save(slot: i32) -> Self {
        Self {
            op: Op::Save as u8,
            ch: 0,
            pad: 0,
            a: slot,
            b: 0,
        }
    }

    pub fn match_accept() -> Self {
        Self::simple(Op::Match)
    }

    pub fn backref(cap_idx: i32) -> Self {
        Self {
            op: Op::Backref as u8,
            ch: 0,
            pad: 0,
            a: cap_idx,
            b: 0,
        }
    }

    pub fn lookaround(op: Op, sub_idx: i32) -> Self {
        debug_assert!(matches!(
            op,
            Op::Lookahead | Op::NegLookahead | Op::Lookbehind | Op::NegLookbehind
        ));
        Self {
            op: op as u8,
            ch: 0,
            pad: 0,
            a: sub_idx,
            b: 0,
        }
    }
}

#[derive(Debug)]
pub struct Program {
    pub insts: Vec<Inst>,
    pub classes: Vec<CharClass>,
    /// Sub-programs for lookahead / lookbehind bodies. Each body
    /// compiles into its own `Program` with an `OP_MATCH` at the end;
    /// the parent emits `OP_LOOKAHEAD`/`OP_LOOKBEHIND` with `a` =
    /// sub-program index. Recursively dropped via `Vec<Box<...>>` —
    /// no manual `prog_free` recursion needed (replaces the C port's
    /// `prog_free` recursion).
    pub sub_progs: Vec<Box<Program>>,
    /// V0.2 P14-S2 perf — literal-prefix anchor for SIMD-fast
    /// search. Set by `regex/compile.rs` at the end of compilation
    /// when the program's first byte-consuming op is an `OP_CHAR(b)`
    /// (Save/AnchorB and other zero-width ops are skipped over —
    /// `(...)` capture groups don't disqualify the optimization)
    /// and the i flag is not set. `search_from_with_ws` uses this
    /// to memchr-skip ahead to the next candidate start position,
    /// avoiding the per-position NFA simulation on the gaps.
    /// Mirrors the literal-prefix optimisation in `rust-regex` /
    /// RE2 / Hyperscan. On `str-replace-100k` no-match probe this
    /// drops the Pike VM cost from ~1011 ns/iter to ~30 ns/iter.
    pub prefix_byte: Option<u8>,
    /// V0.2 P14-S17 — per-Program saves arena row stride, computed
    /// once at compile time. `Workspace::for_program` reads this
    /// instead of re-scanning `insts` on every cache-miss
    /// allocation. Mirrors the `prefix_byte` lifecycle: emitted by
    /// `compute_saves_stride()` after the final `OP_MATCH` is
    /// appended, then read by every consumer that builds a
    /// Workspace for this Program. `2` is the conservative default
    /// (whole-match slots 0/1) for Programs lacking any `OP_SAVE`.
    pub saves_stride: usize,
}

impl Default for Program {
    fn default() -> Self {
        Self {
            insts: Vec::new(),
            classes: Vec::new(),
            sub_progs: Vec::new(),
            prefix_byte: None,
            // V0.2 P14-S17 — `2` is the conservative minimum (whole-
            // match slots 0/1) for Programs that never call
            // `compute_saves_stride`. `compute_saves_stride()` is
            // expected to refine this when the Program is finalised;
            // a zero default would divide-by-zero in `SavesArena`.
            saves_stride: 2,
        }
    }
}

impl Program {
    pub fn new() -> Self {
        Self::default()
    }

    /// V0.2 P14-S17 — finalise `saves_stride` after all `OP_SAVE`
    /// instructions are emitted. Scans `insts` for the highest
    /// `OP_SAVE` slot and stores `(max_slot + 1).max(2)` as the
    /// per-row width of the saves arena. Sub-programs are NOT
    /// recursed — each `Box<Program>` is expected to be finalised
    /// at the site that built it (e.g. `compile_lookaround`).
    pub fn compute_saves_stride(&mut self) {
        let mut max_slot: i32 = -1;
        for inst in &self.insts {
            if inst.op == Op::Save as u8 && inst.a > max_slot {
                max_slot = inst.a;
            }
        }
        self.saves_stride = if max_slot < 0 {
            2
        } else {
            (max_slot as usize) + 1
        };
    }

    /// Append an instruction; return its index (i32 to match the
    /// `Inst.a/b` jump-target type).
    pub fn emit(&mut self, inst: Inst) -> i32 {
        let idx = self.insts.len() as i32;
        self.insts.push(inst);
        idx
    }

    /// Append `cc` to the interned class table; return its index.
    /// (Future P14 perf path: dedupe by structural equality. The C
    /// port doesn't dedupe either, so this is a no-loss carryover.)
    pub fn intern_class(&mut self, cc: &CharClass) -> i32 {
        let idx = self.classes.len() as i32;
        self.classes.push(*cc);
        idx
    }

    pub fn add_sub(&mut self, mut sub: Box<Program>) -> i32 {
        sub.compute_saves_stride();
        let idx = self.sub_progs.len() as i32;
        self.sub_progs.push(sub);
        idx
    }

    /// Index of the next instruction that `emit` will produce — used
    /// by the compiler to backpatch `JMP` / `SPLIT` targets after a
    /// sub-tree has been emitted.
    pub fn next_idx(&self) -> i32 {
        self.insts.len() as i32
    }

    /// Convenience: number of instructions currently in the program.
    pub fn len(&self) -> usize {
        self.insts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.insts.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emit_returns_index_and_grows_vec() {
        let mut p = Program::new();
        assert_eq!(p.emit(Inst::simple(Op::Match)), 0);
        assert_eq!(p.emit(Inst::char_lit(b'a')), 1);
        assert_eq!(p.len(), 2);
    }

    #[test]
    fn intern_class_returns_sequential_indices() {
        let mut p = Program::new();
        let cc1 = CharClass::new();
        let mut cc2 = CharClass::new();
        cc2.add(b'A');
        assert_eq!(p.intern_class(&cc1), 0);
        assert_eq!(p.intern_class(&cc2), 1);
        assert_eq!(p.classes.len(), 2);
    }

    #[test]
    fn add_sub_owns_sub_programs() {
        let mut p = Program::new();
        let sub1 = Box::new({
            let mut s = Program::new();
            s.emit(Inst::char_lit(b'x'));
            s
        });
        let idx = p.add_sub(sub1);
        assert_eq!(idx, 0);
        assert_eq!(p.sub_progs[0].len(), 1);
        assert_eq!(p.sub_progs[0].insts[0].ch, b'x');
    }

    #[test]
    fn compute_saves_stride_defaults_to_two() {
        let p = Program::new();
        assert_eq!(p.saves_stride, 2);
    }

    #[test]
    fn compute_saves_stride_picks_max_save_slot() {
        let mut p = Program::new();
        p.emit(Inst::save(0));
        p.emit(Inst::save(1));
        p.emit(Inst::save(2));
        p.emit(Inst::save(3));
        p.compute_saves_stride();
        // max OP_SAVE slot = 3 → stride = 4 (slots 0..=3).
        assert_eq!(p.saves_stride, 4);
    }

    #[test]
    fn compute_saves_stride_no_saves_keeps_two() {
        let mut p = Program::new();
        p.emit(Inst::char_lit(b'a'));
        p.emit(Inst::match_accept());
        p.compute_saves_stride();
        assert_eq!(p.saves_stride, 2);
    }

    #[test]
    fn add_sub_finalises_sub_stride() {
        let mut p = Program::new();
        let sub = Box::new({
            let mut s = Program::new();
            s.emit(Inst::save(0));
            s.emit(Inst::save(1));
            s.emit(Inst::save(2));
            s.emit(Inst::save(3));
            s
        });
        // pre-add: stride still at default
        assert_eq!(sub.saves_stride, 2);
        p.add_sub(sub);
        // post-add: add_sub calls compute_saves_stride
        assert_eq!(p.sub_progs[0].saves_stride, 4);
    }

    #[test]
    fn inst_factories_match_layout() {
        let i = Inst::char_lit(b'A');
        assert_eq!(i.op, Op::Char as u8);
        assert_eq!(i.ch, b'A');

        let s = Inst::split(3, 7);
        assert_eq!(s.op, Op::Split as u8);
        assert_eq!(s.a, 3);
        assert_eq!(s.b, 7);

        let m = Inst::match_accept();
        assert_eq!(m.op, Op::Match as u8);
    }

    #[test]
    fn op_from_u8_roundtrip() {
        for op in [
            Op::Char,
            Op::AnyChar,
            Op::Class,
            Op::AnchorB,
            Op::AnchorE,
            Op::WBound,
            Op::NWBound,
            Op::Jmp,
            Op::Split,
            Op::Match,
            Op::Save,
            Op::Lookahead,
            Op::NegLookahead,
            Op::Lookbehind,
            Op::NegLookbehind,
            Op::Backref,
        ] {
            assert_eq!(Op::from_u8(op as u8), Some(op));
        }
        assert_eq!(Op::from_u8(0), None);
        assert_eq!(Op::from_u8(17), None);
    }

    #[test]
    fn inst_size_matches_c_port() {
        assert_eq!(core::mem::size_of::<Inst>(), 12);
    }
}
