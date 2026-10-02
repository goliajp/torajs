//! Facts about a matcher's shape that a native walk compiles against:
//! where a dead scan can resume.

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
}
