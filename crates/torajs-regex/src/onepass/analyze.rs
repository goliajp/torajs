//! One-pass determination and matcher construction.
//!
//! States are seeded by one program counter each: the start state by
//! pc 0, every other state by the successor of the consuming
//! instruction whose edge enters it. A state's body is the ε-closure
//! of its seed walked in the Pike VM's priority order (a split's
//! first target before its second, first arrival at a pc wins), with
//! the save slots met on the way carried along. The walk stops at
//! `Op::Match`: threads behind it are lower priority than a match
//! the VM has already committed to, so they never consume.
//!
//! The program is one-pass when no byte is claimed by two consuming
//! instructions of the same state unless both lead to the same
//! `(target, saves)` pair.

use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;

use super::ir::{Edge, MatcherIr, State};
use crate::parser::RE_FLAG_S;
use crate::program::{Inst, Op, Program};
use crate::vm::char_eq;

/// Build the matcher for `prog` (compiled with `Program::raw_classes`)
/// over Latin-1 code units. `None` when the program is not one-pass,
/// or uses a shape this walk does not serve: assertions, lookaround,
/// backreferences, a literal above 0x7F (the parser spells it as
/// UTF-8 bytes), a property class or a range past U+00FF.
pub fn build(prog: &Program) -> Option<MatcherIr> {
    if !servable(prog) {
        return None;
    }
    let mut states: Vec<State> = Vec::new();
    let mut by_seed: BTreeMap<usize, u32> = BTreeMap::new();
    let mut queue: Vec<usize> = vec![0];
    by_seed.insert(0, 0);
    while let Some(seed) = queue.pop() {
        let id = by_seed[&seed] as usize;
        if states.len() <= id {
            states.resize_with(id + 1, || State {
                edges: Vec::new(),
                accept: None,
            });
        }
        let Closure { consuming, accept } = closure(prog, seed);
        let mut table: Vec<Option<(u32, Vec<u32>)>> = vec![None; 256];
        for (pc, saves) in consuming {
            let next = pc + 1;
            let target = match by_seed.get(&next) {
                Some(&t) => t,
                None => {
                    let t = by_seed.len() as u32;
                    by_seed.insert(next, t);
                    queue.push(next);
                    t
                }
            };
            let ins = prog.insts[pc];
            for b in 0..=255u8 {
                if !steps(prog, ins, b) {
                    continue;
                }
                let want = (target, saves.clone());
                match &table[b as usize] {
                    None => table[b as usize] = Some(want),
                    Some(have) if *have == want => {}
                    Some(_) => return None,
                }
            }
        }
        states[id] = State {
            edges: coalesce(&table),
            accept,
        };
    }
    Some(MatcherIr {
        states,
        stride: stride(prog),
    })
}

fn servable(prog: &Program) -> bool {
    if !prog.sub_progs.is_empty() {
        return false;
    }
    let n = prog.len();
    let in_range = |t: i32| t >= 0 && (t as usize) < n;
    prog.insts.iter().enumerate().all(|(i, ins)| {
        let falls_through = i + 1 < n;
        match Op::from_u8(ins.op) {
            Some(Op::Char) => falls_through && ins.ch < 0x80,
            Some(Op::AnyChar) => falls_through,
            Some(Op::Class) => {
                falls_through
                    && (ins.a as usize) < prog.classes.len()
                    && single_unit_class(&prog.classes[ins.a as usize])
            }
            Some(Op::Save) => falls_through && ins.a >= 0,
            Some(Op::Jmp) => in_range(ins.a),
            Some(Op::Split) => in_range(ins.a) && in_range(ins.b),
            Some(Op::Match) => true,
            _ => false,
        }
    })
}

/// A class decided by one Latin-1 code unit: the bitmap holds
/// U+0000..U+00FF as parsed (`Program::raw_classes`), so `negate`
/// reaches every unit above 0x7F the way the spec's `[^…]` does;
/// a property table or a range past U+00FF needs the code point,
/// and an expansion leaf (`byte_only`) is a UTF-8 byte, not a unit.
fn single_unit_class(c: &crate::charclass::CharClass) -> bool {
    c.u_prop_tables.is_empty() && c.owned_ranges.is_empty() && !c.byte_only
}

fn steps(prog: &Program, ins: Inst, b: u8) -> bool {
    match Op::from_u8(ins.op) {
        Some(Op::Char) => char_eq(ins.ch, b, ins.pad as u8),
        Some(Op::AnyChar) => ins.pad as u8 & RE_FLAG_S != 0 || b != b'\n',
        Some(Op::Class) => {
            let ci = ins.pad as u8 & crate::parser::RE_FLAG_I != 0;
            prog.classes[ins.a as usize].test_cp_fold(b as i32, ci)
        }
        _ => false,
    }
}

struct Closure {
    /// consuming pcs in priority order, each with the slots written on
    /// its ε path from the seed
    consuming: Vec<(usize, Vec<u32>)>,
    accept: Option<Vec<u32>>,
}

fn closure(prog: &Program, seed: usize) -> Closure {
    let mut visited = vec![false; prog.len()];
    let mut stack: Vec<(usize, Vec<u32>)> = vec![(seed, Vec::new())];
    let mut consuming = Vec::new();
    let mut accept = None;
    while let Some((pc, saves)) = stack.pop() {
        if visited[pc] {
            continue;
        }
        visited[pc] = true;
        let ins = prog.insts[pc];
        match Op::from_u8(ins.op) {
            Some(Op::Jmp) => stack.push((ins.a as usize, saves)),
            Some(Op::Split) => {
                stack.push((ins.b as usize, saves.clone()));
                stack.push((ins.a as usize, saves));
            }
            Some(Op::Save) => {
                let slot = ins.a as u32;
                let mut saves = saves;
                if let Err(at) = saves.binary_search(&slot) {
                    saves.insert(at, slot);
                }
                stack.push((pc + 1, saves));
            }
            Some(Op::Match) => {
                accept = Some(saves);
                break;
            }
            _ => consuming.push((pc, saves)),
        }
    }
    Closure { consuming, accept }
}

fn coalesce(table: &[Option<(u32, Vec<u32>)>]) -> Vec<Edge> {
    let mut edges: Vec<Edge> = Vec::new();
    for (b, cell) in table.iter().enumerate() {
        let Some((target, saves)) = cell else {
            continue;
        };
        if let Some(last) = edges.last_mut()
            && last.hi as usize + 1 == b
            && last.target == *target
            && last.saves == *saves
        {
            last.hi = b as u8;
            continue;
        }
        edges.push(Edge {
            lo: b as u8,
            hi: b as u8,
            saves: saves.clone(),
            target: *target,
        });
    }
    edges
}

fn stride(prog: &Program) -> u32 {
    let max_slot = prog
        .insts
        .iter()
        .filter(|ins| ins.op == Op::Save as u8)
        .map(|ins| ins.a)
        .max()
        .unwrap_or(-1);
    (max_slot + 1).max(2) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::compile;
    use crate::parser::{Parser, RE_FLAG_I, RE_FLAG_U};
    use crate::vm::Workspace;
    use crate::vm::match_at::vm_match_at;

    /// the program the Pike VM runs (UTF-8 haystack) and the one the
    /// matcher is built from (code units); they differ only where a
    /// class is unsafe for the byte walk
    fn programs(pat: &str, flags: u8) -> (Program, Program) {
        let mut p = Parser::new(pat.as_bytes(), flags);
        let root = p.parse().expect("parse failed");
        let mut vm = Program::new();
        compile(&mut vm, &root, flags);
        vm.emit(Inst::match_accept());
        vm.has_save = vm.any_save();
        vm.finalize_backref_caps();
        let mut raw = Program::new();
        raw.raw_classes = true;
        compile(&mut raw, &root, flags);
        raw.emit(Inst::match_accept());
        (vm, raw)
    }

    fn matcher(pat: &str, flags: u8) -> Option<MatcherIr> {
        build(&programs(pat, flags).1)
    }

    /// the Pike VM's leftmost-first answer from `from`: start, end, row
    fn pike(prog: &Program, s: &[u8], from: usize, flags: u8) -> Option<(i64, i64, Vec<i64>)> {
        let mut ws = Workspace::for_program(prog);
        for st in from..=s.len() {
            let mut row = vec![-1i64; ws.arena.stride];
            let end = vm_match_at(prog, s, st as i64, flags, &mut ws, Some(&mut row), -1);
            if end >= 0 {
                row[0] = st as i64;
                row[1] = end;
                return Some((st as i64, end, row));
            }
        }
        None
    }

    /// every search start over `hay` answers exactly what the Pike VM
    /// answers; returns how many of them hit
    fn agrees(pat: &str, flags: u8, hay: &str) -> usize {
        let (prog, raw) = programs(pat, flags);
        let ir = build(&raw).unwrap_or_else(|| panic!("{pat}: not one-pass"));
        assert_eq!(
            ir.stride as usize,
            Workspace::for_program(&prog).arena.stride
        );
        let s = hay.as_bytes();
        let mut hits = 0;
        for from in 0..=s.len() {
            let want = pike(&prog, s, from, flags);
            let mut out = vec![-1i64; ir.stride as usize];
            let end = ir.exec(s, from, &mut out);
            match want {
                Some((st, wend, row)) => {
                    assert_eq!(end, wend, "{pat} from {from}: end");
                    assert_eq!(out[0], st, "{pat} from {from}: start");
                    assert_eq!(out, row, "{pat} from {from}: row");
                    hits += 1;
                }
                None => assert_eq!(end, -1, "{pat} from {from}: expected miss"),
            }
        }
        hits
    }

    /// some search starts hit, some miss
    fn same(pat: &str, flags: u8, hay: &str) {
        let hits = agrees(pat, flags, hay);
        assert!(hits > 0, "{pat}: nothing matched in {hay:?}");
        assert!(
            hits <= hay.len(),
            "{pat}: no search start missed in {hay:?}"
        );
    }

    /// every search start hits (a pattern that matches empty)
    fn always(pat: &str, flags: u8, hay: &str) {
        assert_eq!(agrees(pat, flags, hay), hay.len() + 1, "{pat} over {hay:?}");
    }

    #[test]
    fn bench_pattern() {
        same("([a-z]+)=(\\d+)", 0, "kq=17;=3;abc=905;zz12;");
        assert_eq!(agrees("([a-z]+)=(\\d+)", 0, "aaaaaaaa;bbbb="), 0);
        let ir = matcher("([a-z]+)=(\\d+)", 0).unwrap();
        // one state per seed pc: the loop body's class and the first
        // one share a closure shape but not a seed
        assert_eq!(ir.states.len(), 6);
        assert_eq!(ir.stride, 6);
        assert_eq!(ir.states[0].edges.len(), 1);
        assert_eq!(ir.states[0].edges[0].saves, [2]);
        assert!(ir.states[0].accept.is_none());
        let mut out = vec![-1i64; 6];
        assert_eq!(ir.exec(b"..ab=12;", 0, &mut out), 7);
        assert_eq!(out, [2, 7, 2, 4, 5, 7]);
        assert_eq!(ir.slots_written_past_accept(), [5]);
    }

    #[test]
    fn groups_alternation_and_optionals() {
        same("(a)?(b)", 0, "abb");
        same("(?:(a)|(b))c", 0, "acbc");
        same("(a)|(b)", 0, "xba");
        same("(cat|dog)s?", 0, "cats dog cow");
        same("((a)(b))c", 0, "xabcab");
        same("(?:(a)|b)+", 0, "abab c");
        same("(?:(\\d)(\\w)?,)+", 0, "1a,2,3b,");
        same("(x)(y)(z)(w)", 0, "xyzw");
    }

    #[test]
    fn empty_matches_and_loops() {
        always("(a*)", 0, "baa");
        always("(a*)*", 0, "aab");
        same("(a|)*b", 0, "aab");
        always("()|(a)", 0, "ab");
        always("x*", 0, "yxx");
        always("(a*?)", 0, "aa");
        always("(a+?)?", 0, "aa");
        let ir = matcher("(a*)", 0).unwrap();
        assert!(ir.states[0].accept.is_some() && !ir.states[0].edges.is_empty());
    }

    #[test]
    fn flags_fold_into_the_edges() {
        same("(He)(l+)o", RE_FLAG_I, "xHELLo hello");
        same("([a-c]+)([X-Z]*)", RE_FLAG_I, "AbCxYz.");
        same("(a.)(.b)?", RE_FLAG_S, "a\n\nb");
        same("(a.)(.b)?", 0, "a\nab.b");
        same("([^=;]+)=([^;]*);?", 0, "k=v;=x;ab=;q");
        same("(\\w+)@(\\w+)", RE_FLAG_U, "a@b c@d");
    }

    #[test]
    fn latin1_units_take_the_negated_class() {
        let ir = matcher("([^a]+)", 0).unwrap();
        let mut out = vec![-1i64; 4];
        assert_eq!(ir.exec(b"a\xe9\xffa", 0, &mut out), 3);
        assert_eq!(out, [1, 3, 1, 3]);
        let ir = matcher("(a)", RE_FLAG_I).unwrap();
        assert_eq!(ir.exec(b"\xc1\xe1A", 0, &mut out), 3);
        let ir = matcher("([\u{e9}-\u{ff}]+)", 0).unwrap();
        assert_eq!(ir.exec(b"a\xe9\xffa", 0, &mut out), 3);
        assert_eq!(out, [1, 3, 1, 3]);
        assert_eq!(ir.exec(b"\xe8", 0, &mut out), -1);
    }

    #[test]
    fn refuses_what_it_cannot_walk() {
        for (pat, flags) in [
            ("(a|ab)(c|bcd)", 0),
            ("(a+)(a*)b", 0),
            ("\\d|\\d\\d", 0),
            ("(a)\\1", 0),
            ("a(?=b)", 0),
            ("(?<!a)b", 0),
            ("^(a)", 0),
            ("(a)$", 0),
            ("\\b(\\w+)", 0),
            ("(\u{e9}+)=", 0),
            ("[\u{4e2d}]", 0),
            ("(\\p{L}+)", RE_FLAG_U),
        ] {
            assert!(matcher(pat, flags).is_none(), "{pat}");
        }
    }
}
