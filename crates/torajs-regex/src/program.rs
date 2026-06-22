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
use crate::dfa::DfaProgram;
use alloc::{boxed::Box, vec::Vec};
use core::cell::UnsafeCell;

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
    /// `true` iff the AST passes [`crate::dfa::analyze`] — no
    /// backref / no lookaround — and thus is representable by a
    /// classical subset-construction DFA. Set at compile time by
    /// `regex/compile.rs` after `resolve_backrefs` runs (so named
    /// backrefs are correctly accounted). Presently informational —
    /// future chunks wire a DFA fast path in
    /// `vm::search_from_with_ws` when this flag is set. Default
    /// `false` keeps the never-match stub (rejected case) safely
    /// out of any future fast path.
    pub can_dfa: bool,
    /// Lazy per-Program DFA cache (mirror of [`crate::regex::RegExp::workspace_cache`]).
    /// `None` until the first [`Program::get_or_build_dfa`] call;
    /// subsequent calls reuse the built [`DfaProgram`] for every
    /// search/replace invocation against this program. The build cost
    /// (BFS subset construction over 256 input bytes per state) is paid
    /// once at first use; downstream `dfa_search` is then table-walk.
    ///
    /// Single-threaded by v0.2's no-multi-thread substrate (§6.2 of the
    /// design principles); biased-ARC multi-thread transition (v1.0+)
    /// will rebuild per owner-thread or share-transition the cache
    /// through the cross-thread atomic path.
    pub dfa_cache: UnsafeCell<Option<DfaProgram>>,
}

impl Program {
    pub fn new() -> Self {
        Self::default()
    }

    /// Lazy-build the DFA on first call; return `Some(&DfaProgram)` if
    /// this program is DFA-eligible ([`Program::can_dfa`] is `true`),
    /// `None` otherwise.
    ///
    /// Callers must additionally gate on the absence of `SAVE` / `Anchor*` /
    /// `WBound` / `NWBound` opcodes until chunks 8/9 land — those are
    /// terminal in [`crate::dfa::epsilon_closure`] and the resulting
    /// DFA does not yet represent their effects. Future chunks tighten
    /// the gate inside this method.
    ///
    /// Build cost is paid exactly once per `Program` instance via the
    /// `UnsafeCell` lazy slot; subsequent calls return the cached
    /// reference. Single-threaded by v0.2 substrate convention; v1.0+
    /// biased-ARC share will thread-local-index this cache (see field
    /// docs).
    pub fn get_or_build_dfa(&self) -> Option<&DfaProgram> {
        if !self.can_dfa {
            return None;
        }
        // SAFETY: v0.2 single-mutator substrate (§6.2 of the design
        // principles) guarantees no concurrent access. Mirrors the
        // `workspace_cache` UnsafeCell pattern on `RegExp`.
        let cell_ptr = self.dfa_cache.get();
        let opt: &mut Option<DfaProgram> = unsafe { &mut *cell_ptr };
        if opt.is_none() {
            *opt = Some(crate::dfa::build_dfa(self));
        }
        opt.as_ref()
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

    // get_or_build_dfa lazy-slot tests — exercise the UnsafeCell-backed
    // DFA cache: gating on `can_dfa`, lazy build on first call, identity
    // reuse on subsequent calls, and the post-build invariant that the
    // cached `DfaProgram` reflects the program structure.

    #[test]
    fn get_or_build_dfa_returns_none_when_can_dfa_is_false() {
        let mut p = Program::new();
        p.emit(Inst::char_lit(b'a'));
        p.emit(Inst::match_accept());
        // default: can_dfa = false → cache stays unbuilt and we get None.
        assert!(p.get_or_build_dfa().is_none());
        // SAFETY: not concurrently accessed in this test. Mirror the
        // unsafe in `get_or_build_dfa` to peek at the cache.
        let cache_state = unsafe { &*p.dfa_cache.get() };
        assert!(
            cache_state.is_none(),
            "rejected path must not eagerly build"
        );
    }

    #[test]
    fn get_or_build_dfa_builds_lazily_when_eligible() {
        let mut p = Program::new();
        p.emit(Inst::char_lit(b'a'));
        p.emit(Inst::match_accept());
        p.can_dfa = true;
        // Cache empty before the call.
        let pre = unsafe { &*p.dfa_cache.get() };
        assert!(pre.is_none());
        // First call builds the DFA in place.
        let dfa = p.get_or_build_dfa().expect("can_dfa = true => Some");
        // dead + start({0}) + accept({1}) = 3 states (same as direct
        // build_dfa unit test in dfa.rs).
        assert_eq!(dfa.states.len(), 3);
        // Cache now Some.
        let post = unsafe { &*p.dfa_cache.get() };
        assert!(post.is_some());
    }

    #[test]
    fn get_or_build_dfa_returns_same_dfa_across_calls() {
        let mut p = Program::new();
        p.emit(Inst::char_lit(b'x'));
        p.emit(Inst::match_accept());
        p.can_dfa = true;
        let first = p.get_or_build_dfa().unwrap() as *const _;
        let second = p.get_or_build_dfa().unwrap() as *const _;
        assert_eq!(first, second, "lazy slot must reuse same DfaProgram");
    }

    #[test]
    fn get_or_build_dfa_on_empty_program_returns_one_state() {
        let mut p = Program::new();
        p.can_dfa = true;
        // empty Program → epsilon_closure({0}) = {} → DfaProgram with
        // only the dead state, start = 0.
        let dfa = p.get_or_build_dfa().unwrap();
        assert_eq!(dfa.states.len(), 1);
        assert_eq!(dfa.start, 0);
    }
}
