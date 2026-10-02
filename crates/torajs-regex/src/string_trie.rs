//! Shared-prefix trie lowering for the multi-cp strings of a v-mode
//! class ([`NodeKind::StringSet`]) — the `\q{…}` and
//! `\p{RGI_Emoji_*}` members (ES §22.2.2.7 CompileAtom,
//! `Atom :: CharacterClass`).
//!
//! The strings are keyed by the unit the program consumes (UTF-8
//! bytes, or code points under `Program::raw_classes`), so every
//! shared prefix is one path and every split fans out only on the
//! units that actually differ at that depth. An alternation of flat
//! concats puts the head of every string into the ε-closure of each
//! loop iteration; the trie keeps that closure at the root's fan-out.
//!
//! Priority: the spec tries the strings in descending length. Any two
//! strings that both match at one position are a prefix pair, so at
//! a node that both accepts and continues the continuation is tried
//! first and the accepting fall-through last — the deepest match is
//! reached first and the Pike VM backs off to shorter prefixes in
//! length order. Strings of equal length exclude each other, so the
//! order of siblings is free (sorted by unit for determinism).

use crate::node::Node;
use crate::program::{Inst, Program};
use crate::utf8::utf8_encode_cp;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

#[derive(Default)]
struct Trie {
    accept: bool,
    kids: BTreeMap<u32, Trie>,
}

impl Trie {
    fn insert(&mut self, units: &[u32]) {
        let mut cur = self;
        for &u in units {
            cur = cur.kids.entry(u).or_default();
        }
        cur.accept = true;
    }
}

/// Emit `node`'s strings as one trie. `rev` walks each string from
/// its last unit (reverse Pike VM, lookbehind bodies).
pub(crate) fn compile_string_set(prog: &mut Program, node: &Node, rev: bool) {
    let mut trie = Trie::default();
    let mut units: Vec<u32> = Vec::new();
    for s in &node.kids {
        units.clear();
        for ch in &s.kids {
            if prog.raw_classes {
                units.push(ch.ch);
            } else {
                let mut buf = [0u8; 4];
                let len = utf8_encode_cp(ch.ch as i32, &mut buf);
                units.extend(buf[..len].iter().map(|&b| u32::from(b)));
            }
        }
        if rev {
            units.reverse();
        }
        trie.insert(&units);
    }
    emit(prog, &trie, node.eff_ims as u16);
}

/// Children first, accepting fall-through last; a node with a single
/// continuation and no accept emits straight-line code.
fn emit(prog: &mut Program, t: &Trie, pad: u16) {
    let n_branches = t.kids.len() + usize::from(t.accept);
    let mut jmps: Vec<usize> = Vec::new();
    for (i, (&unit, sub)) in t.kids.iter().enumerate() {
        let split = (i + 1 < n_branches).then(|| prog.emit(Inst::split(0, 0)) as usize);
        let mut ins = Inst::char_lit(unit);
        ins.pad = pad;
        prog.emit(ins);
        emit(prog, sub, pad);
        if let Some(sidx) = split {
            // no jump when only the accepting fall-through follows
            if i + 1 < t.kids.len() {
                jmps.push(prog.emit(Inst::jmp(0)) as usize);
            }
            prog.insts[sidx].a = sidx as i32 + 1;
            prog.insts[sidx].b = prog.next_idx();
        }
    }
    let end = prog.next_idx();
    for j in jmps {
        prog.insts[j].a = end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::NodeKind;
    use crate::program::Op;
    use alloc::boxed::Box;

    fn string_set(strings: &[&str]) -> Box<Node> {
        let mut set = Node::new(NodeKind::StringSet);
        for s in strings {
            let mut concat = Node::new(NodeKind::Concat);
            for c in s.chars() {
                let mut ch = Node::new(NodeKind::Char);
                ch.ch = u32::from(c);
                concat.push_kid(ch);
            }
            set.push_kid(concat);
        }
        set
    }

    fn ops(prog: &Program) -> Vec<Op> {
        prog.insts
            .iter()
            .map(|i| Op::from_u8(i.op).unwrap())
            .collect()
    }

    #[test]
    fn prefix_pair_tries_the_longer_string_first() {
        let mut prog = Program::new();
        compile_string_set(&mut prog, &string_set(&["ab", "abc"]), false);
        // a b SPLIT(c, end) c
        assert_eq!(ops(&prog), [Op::Char, Op::Char, Op::Split, Op::Char]);
        assert_eq!(prog.insts[2].a, 3);
        assert_eq!(prog.insts[2].b, 4);
    }

    #[test]
    fn siblings_share_the_prefix_and_fan_out_once() {
        let mut prog = Program::new();
        compile_string_set(&mut prog, &string_set(&["ab", "ac", "ad"]), false);
        // a SPLIT b JMP SPLIT c JMP d
        assert_eq!(
            ops(&prog),
            [
                Op::Char,
                Op::Split,
                Op::Char,
                Op::Jmp,
                Op::Split,
                Op::Char,
                Op::Jmp,
                Op::Char
            ]
        );
        assert_eq!(prog.insts[3].a, 8);
        assert_eq!(prog.insts[6].a, 8);
    }

    #[test]
    fn non_ascii_keys_on_utf8_bytes_unless_raw() {
        let mut prog = Program::new();
        compile_string_set(&mut prog, &string_set(&["éa", "éb"]), false);
        let bytes: Vec<u8> = prog.insts[..2].iter().map(Inst::byte).collect();
        assert_eq!(bytes, [0xC3, 0xA9]);
        assert_eq!(prog.insts[2].op, Op::Split as u8);

        let mut raw = Program::new();
        raw.raw_classes = true;
        compile_string_set(&mut raw, &string_set(&["éa", "éb"]), false);
        assert_eq!(raw.insts[0].cp(), 0xE9);
        assert_eq!(raw.insts[1].op, Op::Split as u8);
    }

    #[test]
    fn reverse_keys_on_the_last_unit_first() {
        let mut prog = Program::new();
        compile_string_set(&mut prog, &string_set(&["ab", "cb"]), true);
        assert_eq!(prog.insts[0].byte(), b'b');
        assert_eq!(prog.insts[1].op, Op::Split as u8);
    }
}
