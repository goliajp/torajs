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

#[derive(Debug, Default)]
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
}

impl Program {
    pub fn new() -> Self {
        Self::default()
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

    pub fn add_sub(&mut self, sub: Box<Program>) -> i32 {
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
