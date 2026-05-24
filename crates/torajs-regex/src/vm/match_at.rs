//! `vm_match_at` — inner Pike NFA loop. Port of
//! `runtime_regex.c` L1779-2011.
//!
//! Drives the cur/nxt thread-list swap one input position at a time,
//! dispatching input-consuming ops (`CHAR`, `ANYCHAR`, `CLASS`,
//! `BACKREF`) and recording `MATCH` outcomes. Epsilon ops are
//! resolved transitively by [`add_thread`](super::dispatch::add_thread).

use super::{Workspace, char_eq};
use crate::node::REGEX_SAVE_SLOTS;
use crate::parser::{RE_FLAG_S, RE_FLAG_U};
use crate::program::{Op, Program};
use crate::utf8::{utf8_decode_cp, utf8_len_for};
use crate::vm::Thread;
use crate::vm::dispatch::add_thread;

/// Try matching `prog` at exactly `start_pos`. Returns the end
/// position on hit (so `start_pos..end_pos` is the matched range),
/// or `-1` on miss. On hit, also writes the winning thread's saves
/// into `out_saves` (if provided).
///
/// `end_target` gates the leftmost-first MATCH semantics:
/// - `< 0` → normal: any MATCH at any pos wins
/// - `>= 0` → length-restricted: only MATCH at `pos == end_target`
///   commits (used by lookbehind probing).
#[allow(clippy::too_many_arguments)]
pub fn vm_match_at(
    prog: &Program,
    s: &[u8],
    start_pos: i64,
    flags: u8,
    ws: &mut Workspace,
    mut out_saves: Option<&mut [i64; REGEX_SAVE_SLOTS]>,
    end_target: i64,
) -> i64 {
    let slen = s.len() as i64;

    ws.cur.clear();
    ws.cur.step_id = ws.next_step_id();
    let empty_saves = [-1i64; REGEX_SAVE_SLOTS];
    // Seed PC=0 through the epsilon expander.
    add_thread(
        &mut ws.cur,
        &mut ws.vc,
        0,
        prog,
        s,
        start_pos,
        flags,
        &empty_saves,
    );

    let mut end_pos: i64 = -1;

    let mut pos = start_pos;
    while pos <= slen {
        if end_target >= 0 && pos > end_target {
            break;
        }
        ws.nxt.clear();
        ws.nxt.step_id = ws.next_step_id();
        let mut saw_match_this_step = false;
        let mut ti = 0;
        // Iterate cur via index — body may push into cur (BACKREF
        // epsilon hop on empty capture) and we want to see those.
        while ti < ws.cur.list.len() && !saw_match_this_step {
            // Snapshot the thread (bounded ~540-byte clone) so we
            // can call into add_thread (&mut ws.cur / &mut ws.nxt)
            // without aliasing the indexed read.
            let t = ws.cur.list[ti].clone();
            ti += 1;
            if t.u_skip > 0 {
                // u_skip defer — pass-through to nxt with skip
                // decremented. Bypasses the visited table on
                // purpose so the deferred thread isn't dropped.
                ws.nxt.push(Thread {
                    pc: t.pc,
                    br_offset: t.br_offset,
                    u_skip: t.u_skip - 1,
                    saves: t.saves,
                });
                continue;
            }
            let ins = prog.insts[t.pc];
            let Some(op) = Op::from_u8(ins.op) else {
                continue;
            };
            match op {
                Op::Char => {
                    if pos < slen && char_eq(ins.ch, s[pos as usize], flags) {
                        add_thread(
                            &mut ws.nxt,
                            &mut ws.vn,
                            (t.pc + 1) as i32,
                            prog,
                            s,
                            pos + 1,
                            flags,
                            &t.saves,
                        );
                    }
                }
                Op::AnyChar => {
                    if pos < slen && (flags & RE_FLAG_S != 0 || s[pos as usize] != b'\n') {
                        // Under u flag `.` consumes 1 code point;
                        // schedule the destination thread(s) with
                        // u_skip = adv-1 so they sit (adv-1) outer
                        // steps before dispatching pc+1.
                        let mut adv: i64 = 1;
                        if flags & RE_FLAG_U != 0 {
                            let ul = utf8_len_for(s[pos as usize]) as i64;
                            if ul >= 1 && pos + ul <= slen {
                                adv = ul;
                            }
                        }
                        let n_before = ws.nxt.list.len();
                        add_thread(
                            &mut ws.nxt,
                            &mut ws.vn,
                            (t.pc + 1) as i32,
                            prog,
                            s,
                            pos + adv,
                            flags,
                            &t.saves,
                        );
                        if adv > 1 {
                            let skip = (adv - 1) as i32;
                            for th in &mut ws.nxt.list[n_before..] {
                                th.u_skip = skip;
                            }
                        }
                    }
                }
                Op::Class => {
                    if pos < slen {
                        let cc = &prog.classes[ins.a as usize];
                        let mut adv: i64 = 1;
                        let matched;
                        if flags & RE_FLAG_U != 0 {
                            let ul = utf8_len_for(s[pos as usize]) as i64;
                            if ul >= 1 && pos + ul <= slen {
                                let (cp, dec_len) = utf8_decode_cp(&s[pos as usize..]);
                                adv = if dec_len > 0 { dec_len as i64 } else { ul };
                                matched = cc.test_cp(cp);
                            } else {
                                matched = cc.test(s[pos as usize]);
                            }
                        } else {
                            matched = cc.test(s[pos as usize]);
                        }
                        if matched {
                            let n_before = ws.nxt.list.len();
                            add_thread(
                                &mut ws.nxt,
                                &mut ws.vn,
                                (t.pc + 1) as i32,
                                prog,
                                s,
                                pos + adv,
                                flags,
                                &t.saves,
                            );
                            if adv > 1 {
                                let skip = (adv - 1) as i32;
                                for th in &mut ws.nxt.list[n_before..] {
                                    th.u_skip = skip;
                                }
                            }
                        }
                    }
                }
                Op::Backref => {
                    handle_backref(prog, s, pos, flags, &t, ins.a, ws);
                }
                Op::Match => {
                    // Normal mode: leftmost-first wins; stop scanning
                    // this step. Length-restricted (lookbehind): only
                    // commit if pos meets end_target; otherwise the
                    // thread is dead but keep scanning — other threads
                    // may extend further via nxt and MATCH later.
                    if end_target < 0 || pos == end_target {
                        saw_match_this_step = true;
                        end_pos = pos;
                        if let Some(ref mut o) = out_saves {
                            **o = t.saves;
                        }
                    }
                }
                _ => {}
            }
        }
        // Swap cur <-> nxt + their visited tables.
        std::mem::swap(&mut ws.cur, &mut ws.nxt);
        std::mem::swap(&mut ws.vc, &mut ws.vn);
        if ws.cur.is_empty() {
            break;
        }
        pos += 1;
    }

    // End-of-input: any thread sitting on MATCH after the loop is
    // also an acceptance.
    for t in &ws.cur.list {
        if prog.insts[t.pc].op == Op::Match as u8
            && t.u_skip == 0
            && (end_target < 0 || slen == end_target)
        {
            end_pos = slen;
            if let Some(ref mut o) = out_saves {
                **o = t.saves;
            }
            break;
        }
    }
    end_pos
}

/// OP_BACKREF dispatch — extracted because the body is hairy enough
/// to push `vm_match_at` over the 200-LOC fn limit by itself.
fn handle_backref(
    prog: &Program,
    s: &[u8],
    pos: i64,
    flags: u8,
    t: &Thread,
    cap_idx: i32,
    ws: &mut Workspace,
) {
    let slen = s.len() as i64;
    let slot_s = (2 * cap_idx) as usize;
    let slot_e = (2 * cap_idx + 1) as usize;
    let (cs, ce) = if cap_idx >= 1 && slot_e < REGEX_SAVE_SLOTS {
        (t.saves[slot_s], t.saves[slot_e])
    } else {
        (-1, -1)
    };
    let cap_len = if cs < 0 || ce < 0 { 0 } else { ce - cs };
    if cap_len == 0 {
        // Epsilon-style — schedule pc+1 in cur at the same pos. The
        // outer while-loop re-reads cur.list.len() each iteration so
        // the freshly-pushed thread is visited; visited-table dedup
        // prevents infinite insertion.
        add_thread(
            &mut ws.cur,
            &mut ws.vc,
            (t.pc + 1) as i32,
            prog,
            s,
            pos,
            flags,
            &t.saves,
        );
        return;
    }
    if pos < slen
        && char_eq(
            s[(cs + t.br_offset as i64) as usize],
            s[pos as usize],
            flags,
        )
    {
        let new_offset = t.br_offset + 1;
        if (new_offset as i64) == cap_len {
            // Backref complete — advance pc, reset br_offset.
            add_thread(
                &mut ws.nxt,
                &mut ws.vn,
                (t.pc + 1) as i32,
                prog,
                s,
                pos + 1,
                flags,
                &t.saves,
            );
        } else {
            // Continue same pc next step with offset bumped. Direct
            // insert bypasses visited (different state from any
            // fresh entrant at pc).
            ws.nxt.push(Thread {
                pc: t.pc,
                br_offset: new_offset,
                u_skip: 0,
                saves: t.saves,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::compile;
    use crate::parser::Parser;
    use crate::program::Inst;
    use crate::vm::{MatchResult, search_from};

    fn build(pat: &str, flags: u8) -> Program {
        let mut p = Parser::new(pat.as_bytes(), flags);
        let root = p.parse().expect("parse failed");
        let mut prog = Program::new();
        compile(&mut prog, &root);
        prog.emit(Inst::match_accept());
        prog
    }

    fn matches(pat: &str, hay: &str, flags: u8) -> Option<MatchResult> {
        let prog = build(pat, flags);
        search_from(&prog, hay.as_bytes(), 0, flags)
    }

    #[test]
    fn class_digit_matches() {
        let r = matches("\\d+", "abc123def", 0).expect("hit");
        assert_eq!(r.start, 3);
        assert_eq!(r.end, 6);
    }

    #[test]
    fn negated_class_matches_non_member() {
        let r = matches("[^0-9]+", "  abc1", 0).expect("hit");
        assert_eq!(r.start, 0);
        assert_eq!(r.end, 5);
    }

    #[test]
    fn lookahead_zero_width() {
        let r = matches("foo(?=bar)", "foobar", 0).expect("hit");
        // Lookahead is zero-width: match consumes only "foo".
        assert_eq!(r.start, 0);
        assert_eq!(r.end, 3);
    }

    #[test]
    fn negative_lookahead_excludes() {
        assert!(matches("foo(?!bar)", "foobar", 0).is_none());
        assert!(matches("foo(?!bar)", "foobaz", 0).is_some());
    }

    #[test]
    fn lookbehind_matches() {
        let r = matches("(?<=\\$)\\d+", "x$42y", 0).expect("hit");
        assert_eq!(r.start, 2);
        assert_eq!(r.end, 4);
    }

    #[test]
    fn negative_lookbehind_excludes() {
        assert!(matches("(?<!a)b", "ab", 0).is_none());
        assert!(matches("(?<!a)b", "xb", 0).is_some());
    }

    #[test]
    fn backref_matches_repeated_capture() {
        let r = matches("(a+)\\1", "aaaa", 0).expect("hit");
        // (a+)\1 — greedy aa + aa.
        assert_eq!(r.start, 0);
        assert_eq!(r.end, 4);
    }

    #[test]
    fn word_boundary() {
        let r = matches("\\bword\\b", "the word here", 0).expect("hit");
        assert_eq!(r.start, 4);
        assert_eq!(r.end, 8);
        // No boundary inside a word.
        assert!(matches("\\bord", "word", 0).is_none());
    }

    #[test]
    fn dot_does_not_match_newline_by_default() {
        assert!(matches("a.b", "a\nb", 0).is_none());
    }

    #[test]
    fn dotall_flag_makes_dot_match_newline() {
        let r = matches("a.b", "a\nb", RE_FLAG_S).expect("hit");
        assert_eq!(r.start, 0);
        assert_eq!(r.end, 3);
    }

    #[test]
    fn nested_capture_groups() {
        let r = matches("((a)(b))", "ab", 0).expect("hit");
        // Group 1 = "ab", group 2 = "a", group 3 = "b".
        assert_eq!(r.saves[2], 0);
        assert_eq!(r.saves[3], 2);
        assert_eq!(r.saves[4], 0);
        assert_eq!(r.saves[5], 1);
        assert_eq!(r.saves[6], 1);
        assert_eq!(r.saves[7], 2);
    }

    #[test]
    fn empty_capture_backref_matches() {
        // Empty capture group — backref to it should match
        // epsilon-style.
        let r = matches("(a?)\\1b", "b", 0).expect("hit");
        assert_eq!(r.start, 0);
        assert_eq!(r.end, 1);
    }
}
