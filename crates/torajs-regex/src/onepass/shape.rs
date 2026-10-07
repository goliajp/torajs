//! Facts about a matcher's shape that a native walk compiles against:
//! where a dead scan can resume, and which slots an attempt has to
//! clear before it starts.

use alloc::vec;
use alloc::vec::Vec;

use super::ir::{Edge, MatcherIr};

impl MatcherIr {
    /// The state a dead scan resumes past: the start state has no
    /// probes and no accept, every edge out of it enters one state
    /// `S`, `S` has no probes, no accept, and loops back to itself on
    /// exactly the units that enter it, and no other state leads to
    /// `S`. A start inside a run of those units reaches `S` where the
    /// run ends, at the same position the attempt that began the run
    /// reached it, and the walk's control never reads a slot, so when
    /// that attempt dies every start inside the run dies too: the
    /// scan may resume at the end of the run. A later state leading
    /// back to `S` would make the attempt enter `S` again after the
    /// run, and the end recorded would be the wrong run's.
    pub fn leading_run_state(&self) -> Option<u32> {
        let start = &self.states[0];
        if start.probes != 0 || start.variants[0].accept.is_some() {
            return None;
        }
        let edges = &start.variants[0].edges;
        let s = edges.first()?.target;
        if edges.iter().any(|e| e.target != s) {
            return None;
        }
        let run = &self.states[s as usize];
        if run.probes != 0 || run.variants[0].accept.is_some() {
            return None;
        }
        let reentered = self.states.iter().enumerate().any(|(i, st)| {
            i != 0
                && i != s as usize
                && st
                    .variants
                    .iter()
                    .any(|v| v.edges.iter().any(|e| e.target == s))
        });
        if reentered {
            return None;
        }
        let loops = run.variants[0].edges.iter().filter(|e| e.target == s);
        (merged(edges.iter()) == merged(loops)).then_some(s)
    }
    /// Slots an accept can answer with before any write of this
    /// attempt reached them: every slot of `2..stride` that some path
    /// from the start state to some accept leaves unwritten. A save
    /// and a clear both write. A fresh attempt must set these to -1;
    /// the others are written on every path to every accept, so an
    /// earlier attempt's values never show through.
    pub fn slots_unset_at_accept(&self) -> Vec<u32> {
        let n = self.states.len();
        // slots written on every path from the start to the top of
        // each state; `None` until the state is reached
        let mut must: Vec<Option<Vec<u32>>> = vec![None; n];
        must[0] = Some(Vec::new());
        let mut stack = vec![0usize];
        while let Some(i) = stack.pop() {
            let at = must[i].clone().unwrap();
            for v in &self.states[i].variants {
                for e in &v.edges {
                    let mut via = at.clone();
                    via.extend_from_slice(&e.saves);
                    via.extend_from_slice(&e.clears);
                    via.sort_unstable();
                    via.dedup();
                    let t = e.target as usize;
                    let next = match &must[t] {
                        None => via,
                        Some(have) => have.iter().copied().filter(|s| via.contains(s)).collect(),
                    };
                    if must[t].as_ref() != Some(&next) {
                        must[t] = Some(next);
                        stack.push(t);
                    }
                }
            }
        }
        let mut unset: Vec<u32> = Vec::new();
        for (i, st) in self.states.iter().enumerate() {
            let Some(at) = &must[i] else { continue };
            for v in &st.variants {
                let Some(acc) = &v.accept else { continue };
                unset.extend((2..self.stride).filter(|s| {
                    !at.contains(s) && !acc.contains(s) && !v.accept_clears.contains(s)
                }));
            }
        }
        unset.sort_unstable();
        unset.dedup();
        unset
    }
}

/// `edges` (sorted, disjoint) as ranges, with adjacent ones joined.
fn merged<'a>(edges: impl Iterator<Item = &'a Edge>) -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32)> = Vec::new();
    for e in edges {
        match out.last_mut() {
            Some(last) if last.1 + 1 == e.lo => last.1 = e.hi,
            _ => out.push((e.lo, e.hi)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::compile;
    use crate::parser::{Parser, RE_FLAG_I, RE_FLAG_U};
    use crate::program::{Inst, Program};

    fn matcher(pat: &str, flags: u8, wide: bool) -> MatcherIr {
        let mut p = Parser::new(pat.as_bytes(), flags);
        let root = p.parse().expect("parse failed");
        let mut prog = Program::new();
        prog.raw_classes = true;
        compile(&mut prog, &root, flags);
        prog.emit(Inst::match_accept());
        super::super::build(&prog, flags, wide).expect("not one-pass")
    }

    #[test]
    fn the_run_a_dead_scan_resumes_past() {
        let run = |pat: &str, flags: u8| matcher(pat, flags, false).leading_run_state();
        assert_eq!(run(r"([a-z]+)=(\d+)", 0), Some(1));
        assert!(run(r"[^=;]+=", 0).is_some());
        assert!(run(r"(?:[a-m]|[n-z])+=", 0).is_some());
        assert!(run(r"([a-z]+)=(\d+)", RE_FLAG_I).is_some());
        assert!(
            matcher(r"(\p{L}+)=(\d+)", RE_FLAG_U, true)
                .leading_run_state()
                .is_some()
        );
        // the run state accepts
        assert!(run(r"[a-z]+", 0).is_none());
        assert!(run(r"([a-z]+)=?", 0).is_none());
        // the start state accepts, or probes the position
        assert!(run(r"[a-z]*=", 0).is_none());
        assert!(run(r"\b[a-z]+=", 0).is_none());
        // the run loops on fewer units than enter it, or on more
        assert!(run(r"[a-z][a-y]*=", 0).is_none());
        assert!(run(r"[a-y][a-z]*=", 0).is_none());
        // the start state has another way out
        assert!(run(r"(?:[a-z]+|[0-9])=", 0).is_none());
        // a repetition re-enters a copy of its body, never the run
        // state itself (`x+` compiles as `x x*`), so these keep the lane
        assert!(run(r"(?:[a-z]+=[a-z]+=[a-z0-9][a-z])+X", 0).is_some());
        assert!(run(r"(?:[a-z]+=)+X", 0).is_some());
        assert!(run(r"([a-z]+)(?:=\d+)+", 0).is_some());
        // a state other than the start leading back into the run
        // state would record the wrong run's end: the lane declines
        let mut ir = matcher(r"([a-z]+)=(\d+)", 0, false);
        assert_eq!(ir.leading_run_state(), Some(1));
        let back = Edge {
            lo: b'='.into(),
            hi: b'='.into(),
            saves: alloc::vec::Vec::new(),
            clears: alloc::vec::Vec::new(),
            target: 1,
        };
        ir.states[3].variants[0].edges.insert(0, back);
        assert_eq!(ir.leading_run_state(), None);
        // no loop at all
        assert!(run(r"[a-z]=", 0).is_none());
    }

    #[test]
    fn a_loop_back_to_the_program_start_enters_a_state_of_its_own() {
        for pat in [
            r"(?:(a)b)*c",
            r"(?:(a)b)*",
            r"(?:(x)(y)?z)*w",
            r"(?:[a-z]+=)*X",
        ] {
            for wide in [false, true] {
                let ir = matcher(pat, 0, wide);
                let into_start = ir.states.iter().any(|st| {
                    st.variants
                        .iter()
                        .any(|v| v.edges.iter().any(|e| e.target == 0))
                });
                assert!(!into_start, "{pat}");
            }
        }
        let ir = matcher(r"(?:(a)b)*c", 0, false);
        let mut out = [-1i64; 4];
        assert_eq!(ir.exec(b"ababc", 0, &mut out), 5);
        assert_eq!(out, [0, 5, 2, 3]);
    }

    #[test]
    fn slots_a_fresh_attempt_must_clear() {
        let unset = |pat: &str| matcher(pat, 0, false).slots_unset_at_accept();
        // every path to the accept writes both groups
        assert_eq!(unset(r"([a-z]+)=(\d+)"), []);
        assert_eq!(unset("((a)(b))c"), []);
        assert_eq!(unset("(a*)"), []);
        // an accept reached past a skipped or untaken group
        assert_eq!(unset("(a)?(b)"), [2, 3]);
        assert_eq!(unset("(a)|(b)"), [2, 3, 4, 5]);
        assert_eq!(unset("(a*)(b)?"), [4, 5]);
        assert_eq!(unset("x(a)?"), [2, 3]);
        assert_eq!(unset(r"([a-z]+)=(\d+)?"), [4, 5]);
    }

    /// The emitted walk over a Latin-1 payload: the row is written in
    /// place across attempts, an attempt clears `unset` only, a held
    /// slot is put back when the walk dies past its accept, and a
    /// dead attempt that began a leading run resumes past it.
    fn native_walk(ir: &MatcherIr, unset: &[u32], s: &[u8], out: &mut [i64]) -> i64 {
        let held = ir.slots_written_past_accept();
        let run = ir.leading_run_state().map(|r| r as usize);
        let mut backup = alloc::vec![0i64; held.len()];
        let mut st = 0usize;
        while st <= s.len() {
            for &k in unset {
                out[k as usize] = -1;
            }
            let mut last = -1i64;
            let (mut state, mut pos, mut run_end) = (0usize, st, None);
            loop {
                let v = ir.states[state].variant(super::super::ir::context(s, pos));
                if let Some(acc) = &v.accept {
                    for &k in acc {
                        out[k as usize] = pos as i64;
                    }
                    for &k in &v.accept_clears {
                        out[k as usize] = -1;
                    }
                    last = pos as i64;
                    if v.edges.is_empty() {
                        break;
                    }
                    for (b, &k) in backup.iter_mut().zip(&held) {
                        *b = out[k as usize];
                    }
                }
                let u = s.get(pos).map(|&c| c as u32);
                let e = u.and_then(|u| v.edges.iter().find(|e| e.lo <= u && u <= e.hi));
                let Some(e) = e else {
                    if run == Some(state) {
                        run_end = Some(pos);
                    }
                    if last >= 0 {
                        for (b, &k) in backup.iter().zip(&held) {
                            out[k as usize] = *b;
                        }
                    }
                    break;
                };
                if run == Some(state) && e.target as usize != state {
                    run_end = Some(pos);
                }
                for &k in &e.saves {
                    out[k as usize] = pos as i64;
                }
                for &k in &e.clears {
                    out[k as usize] = -1;
                }
                pos += 1;
                state = e.target as usize;
            }
            if last >= 0 {
                out[0] = st as i64;
                out[1] = last;
                return last;
            }
            st = run_end.unwrap_or(st) + 1;
        }
        -1
    }

    /// every string over `alphabet` up to `max` units
    fn subjects(alphabet: &[u8], max: usize) -> Vec<Vec<u8>> {
        let mut all = alloc::vec![Vec::new()];
        let mut from = 0;
        for _ in 0..max {
            let to = all.len();
            for i in from..to {
                for &c in alphabet {
                    let mut s = all[i].clone();
                    s.push(c);
                    all.push(s);
                }
            }
            from = to;
        }
        all
    }

    /// The first subject on which the walk clearing `unset` answers
    /// other than the reference walk, from a row whose groups hold a
    /// stale value rather than -1.
    fn first_divergence(ir: &MatcherIr, unset: &[u32], alphabet: &[u8]) -> Option<Vec<u8>> {
        let stride = ir.stride as usize;
        subjects(alphabet, 6).into_iter().find(|s| {
            let mut want = alloc::vec![-1i64; stride];
            let mut got = alloc::vec![99i64; stride];
            let a = ir.exec(s.as_slice(), 0, &mut want);
            let b = native_walk(ir, unset, s, &mut got);
            a != b || (a >= 0 && want != got)
        })
    }

    #[test]
    fn a_walk_clearing_only_the_unset_slots_answers_the_reference_rows() {
        for (pat, alphabet) in [
            (r"([a-z]+)=(\d+)", &b"ab=1"[..]),
            (r"([a-z]+)=(\d+)?", b"ab=1"),
            (r"([a-z]+)(?:=(\d+))+", b"a=1;"),
            (r"(?:([a-z]+)=)+X", b"a=X"),
            ("(a)?(b)", b"ab"),
            ("(a)|(b)", b"abc"),
            ("(?:(a)|(b))+c", b"abc"),
            ("(a*)(b)?", b"ab"),
            ("x(a)?", b"xa"),
            ("((a)(b))c", b"abc"),
            ("(?:(a)b)*c", b"abc"),
            ("(?:(x)(y)?z)*w", b"xyzw"),
        ] {
            let ir = matcher(pat, 0, false);
            let unset = ir.slots_unset_at_accept();
            assert_eq!(first_divergence(&ir, &unset, alphabet), None, "{pat}");
        }
        // the resume shapes above do resume
        for pat in [r"([a-z]+)=(\d+)?", r"([a-z]+)(?:=(\d+))+"] {
            assert!(
                matcher(pat, 0, false).leading_run_state().is_some(),
                "{pat}"
            );
        }
        // the check fails when an unset slot is left out
        for pat in ["(a)?(b)", "(a)|(b)"] {
            let ir = matcher(pat, 0, false);
            let unset = ir.slots_unset_at_accept();
            assert!(first_divergence(&ir, &unset[2..], b"ab").is_some(), "{pat}");
        }
    }
}
