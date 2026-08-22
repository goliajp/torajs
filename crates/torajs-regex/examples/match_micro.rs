//! Micro-harness for the match search loop alone — compile a pattern
//! once, build its DFA once, then hammer `vm::search::search_from` the
//! way `regex-dfa-*-100k` runs in steady state (the RegExp literal is
//! LICM-hoisted, so per iteration only the search happens).
//!
//! Not a gate: an ablation loop. The profiler sees the search as one
//! symbol (`dfa_search_mid` / `search_from_with_ws`), so this is how
//! its inside gets priced — in particular how much of it is the outer
//! loop re-running an ANCHORED DFA at every start position that fails.
//! Run: `cargo run --release -p torajs-regex --example match_micro`
//!
//! Columns per case:
//! - `probes`  — start positions the outer loop tries before the hit
//! - `full`    — ns/iter searching the real haystack from 0
//! - `at-hit`  — ns/iter searching the haystack cut to the match start
//!   (what a perfect start-position skip would cost)
//! - `rescan`  — `full - at-hit`, the price of the failed probes

use std::time::Instant;

use torajs_regex::compiler::compile;
use torajs_regex::dfa::build_dfa;
use torajs_regex::flags::parse_flags;
use torajs_regex::parser::Parser;
use torajs_regex::program::{Inst, Program};
use torajs_regex::vm::Workspace;
use torajs_regex::vm::search::{search_from, search_from_with_ws};

struct Case {
    name: &'static str,
    pat: &'static str,
    flags: &'static str,
    hay: &'static str,
}

/// The six haystacks below are what the bench fixtures build per
/// iteration (`"..." + i.toString()` with a mid-range `i`).
const CASES: &[Case] = &[
    Case {
        name: "iflag",
        pat: "hello",
        flags: "i",
        hay: "before HELLO world 50000",
    },
    Case {
        name: "dotall",
        pat: "a.+c",
        flags: "s",
        hay: "pre a\nmiddle\nc post 50000",
    },
    Case {
        name: "uflag",
        pat: "\\p{L}+",
        flags: "u",
        hay: "  hello world 50000",
    },
    Case {
        name: "safeclass",
        pat: "[a-z]+@[a-z]+",
        flags: "",
        hay: "contact me at bob@example now 50000",
    },
    Case {
        name: "wbound",
        pat: "\\bworld\\b",
        flags: "",
        hay: "before world after 50000",
    },
    Case {
        name: "minlit",
        pat: "x",
        flags: "",
        hay: "x",
    },
];

fn build(pat: &str, flags: &str) -> (Program, u8) {
    let flag_bits = parse_flags(flags.as_bytes()).expect("flags parse");
    let mut parser = Parser::new(pat.as_bytes(), flag_bits);
    let root = parser.parse().expect("pattern parses");
    let mut prog = Program::new();
    prog.can_dfa = torajs_regex::dfa::analyze(&root).is_eligible()
        && !torajs_regex::dfa::tree_contains_ml_anchor_end(&root);
    compile(&mut prog, &root, flag_bits);
    prog.emit(Inst::match_accept());
    prog.has_save = prog.any_save();
    prog.finalize_backref_caps();
    (prog, flag_bits)
}

fn time_ns(iters: u32, mut f: impl FnMut()) -> f64 {
    // one warm pass so the branch predictor and caches are hot
    for _ in 0..(iters / 10).max(1) {
        f();
    }
    let t = Instant::now();
    for _ in 0..iters {
        f();
    }
    t.elapsed().as_nanos() as f64 / iters as f64
}

fn main() {
    let iters: u32 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(2_000_000);

    println!(
        "{:<11} {:>6} {:>9} {:>9} {:>9} {:>7}",
        "case", "probes", "full ns", "at-hit ns", "rescan ns", "share"
    );
    for c in CASES {
        let (prog, flag_bits) = build(c.pat, c.flags);
        let dfa = build_dfa(&prog, flag_bits);
        let hay = c.hay.as_bytes();

        let m = search_from(&prog, hay, 0, flag_bits, Some(&dfa)).expect("case must match");
        let hit_at = m.start as usize;

        // Probe count: how many start positions the outer loop walks
        // before one succeeds. Mirrors `dfa_probe`'s all-starts-equal
        // shape closely enough to count (patterns with `\b` pick a mid
        // entry instead, which changes which DFA runs, not how many).
        let mut probes = 0usize;
        for st in 0..=hay.len() {
            probes += 1;
            if torajs_regex::dfa::dfa_search(&dfa, &prog, &hay[st..]).is_some() {
                break;
            }
        }

        // `search_from` hard-codes `haystack_is_ascii = false`, and
        // under the u flag that switches the whole dead-start scan
        // off — so timing through it measured a path the runtime does
        // not take, and any change to that scan read as no change at
        // all. `__torajs_str_match_regex` classifies the haystack
        // first (`str_slice_ascii_view`) and passes the answer down;
        // this does the same.
        let mut ws: Option<Workspace> = None;
        let is_ascii = hay.is_ascii();
        let full = time_ns(iters, || {
            std::hint::black_box(search_from_with_ws(
                &prog,
                std::hint::black_box(hay),
                0,
                flag_bits,
                &mut ws,
                Some(&dfa),
                is_ascii,
                true,
            ));
        });
        let cut = &hay[hit_at..];
        let cut_ascii = cut.is_ascii();
        let at_hit = time_ns(iters, || {
            std::hint::black_box(search_from_with_ws(
                &prog,
                std::hint::black_box(cut),
                0,
                flag_bits,
                &mut ws,
                Some(&dfa),
                cut_ascii,
                true,
            ));
        });
        println!(
            "{:<11} {:>6} {:>9.1} {:>9.1} {:>9.1} {:>6.0}%",
            c.name,
            probes,
            full,
            at_hit,
            full - at_hit,
            (full - at_hit) / full * 100.0
        );
    }
}
