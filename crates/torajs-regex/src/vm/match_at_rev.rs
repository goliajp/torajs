//! `vm_match_at_rev` — reverse Pike NFA loop for lookbehind bodies
//! (ES §22.2.2 MatchReverse; V8 irregexp `read_backward` shape).
//!
//! Mirror of [`super::match_at::vm_match_at`] with the cursor walking
//! LEFT from the assertion point: the outer loop decrements `pos`,
//! consuming ops test `s[pos-1]` and step to `pos-1`, and a MATCH at
//! any `pos >= 0` commits (the body ran to completion — there is no
//! `end_target` notion; `s[j..start_pos]` is the body match). The
//! program must be compiled by [`crate::compiler::compile_rev`]
//! (Concat reversed, capture SAVE slots swapped); epsilon expansion
//! reuses [`super::dispatch::add_thread`] unchanged because every
//! epsilon op (SAVE / anchors / nested lookarounds) is defined purely
//! in terms of `pos`, not of travel direction.
//!
//! Leftmost-first priority is inherited unchanged: when MATCH fires,
//! lower-priority threads this step die, but higher-priority threads
//! already advanced into `nxt` keep extending leftwards and a later
//! (further-left) MATCH from them overrides — which is exactly how a
//! greedy `(?<=(\d+))` commits the LONGEST digit run.

use super::dispatch::add_thread;
use super::match_at::add_thread_adv;
use super::{Workspace, char_eq};
use crate::parser::{RE_FLAG_S, RE_FLAG_U};
use crate::program::{Op, Program};
use crate::utf8::utf8_decode_cp_before;
use crate::vm::Thread;

/// Try matching `prog` (reverse-compiled) ENDING at exactly
/// `start_pos`, scanning leftwards. Returns the body's start position
/// `j` on hit (so `s[j..start_pos]` is the matched range), or `-1` on
/// miss. On hit, also writes the winning thread's saves into
/// `out_saves` (if provided).
pub fn vm_match_at_rev(
    prog: &Program,
    s: &[u8],
    start_pos: i64,
    flags: u8,
    ws: &mut Workspace,
    mut out_saves: Option<&mut [i64]>,
) -> i64 {
    ws.cur.clear();
    ws.cur.step_id = ws.next_step_id();
    ws.arena.reset();
    let seed_saves_id = ws.arena.alloc_empty();
    add_thread(
        &mut ws.cur,
        &mut ws.vc,
        &mut ws.arena,
        0,
        prog,
        s,
        start_pos,
        flags,
        seed_saves_id,
    );

    let mut match_pos: i64 = -1;

    let mut pos = start_pos;
    while pos >= 0 {
        ws.nxt.clear();
        ws.nxt.step_id = ws.next_step_id();
        let mut saw_match_this_step = false;
        let mut ti = 0;
        // Iterate cur via index — body may push into cur (BACKREF
        // epsilon hop on empty capture) and we want to see those.
        while ti < ws.cur.list.len() && !saw_match_this_step {
            let t_pc = ws.cur.list[ti].pc;
            let t_br_offset = ws.cur.list[ti].br_offset;
            let t_u_skip = ws.cur.list[ti].u_skip;
            let t_saves_id = ws.cur.list[ti].saves_id;
            ti += 1;
            if t_u_skip > 0 {
                // u_skip defer — pass-through to nxt with skip
                // decremented (multi-byte cp consumed leftwards sits
                // adv-1 outer steps, same bookkeeping as forward).
                ws.nxt.push(Thread {
                    pc: t_pc,
                    br_offset: t_br_offset,
                    u_skip: t_u_skip - 1,
                    saves_id: t_saves_id,
                });
                continue;
            }
            let ins = prog.insts[t_pc];
            let Some(op) = Op::from_u8(ins.op) else {
                continue;
            };
            match op {
                Op::Char => {
                    if pos > 0 && char_eq(ins.ch, s[(pos - 1) as usize], flags) {
                        add_thread(
                            &mut ws.nxt,
                            &mut ws.vn,
                            &mut ws.arena,
                            (t_pc + 1) as i32,
                            prog,
                            s,
                            pos - 1,
                            flags,
                            t_saves_id,
                        );
                    }
                }
                Op::AnyChar => dispatch_anychar_rev(ws, prog, s, pos, flags, t_pc, t_saves_id),
                Op::Class => dispatch_class_rev(ws, prog, s, pos, flags, t_pc, t_saves_id),
                Op::Backref => {
                    let t = Thread {
                        pc: t_pc,
                        br_offset: t_br_offset,
                        u_skip: t_u_skip,
                        saves_id: t_saves_id,
                    };
                    handle_backref_rev(prog, s, pos, flags, &t, ins.a, ws);
                }
                Op::Match => {
                    // Body complete at this pos. Leftmost-first: cut
                    // lower-priority threads this step; higher-priority
                    // threads already in nxt may override further left.
                    saw_match_this_step = true;
                    match_pos = pos;
                    if let Some(ref mut o) = out_saves {
                        let row = ws.arena.get(t_saves_id);
                        let n = row.len().min(o.len());
                        (*o)[..n].copy_from_slice(&row[..n]);
                    }
                }
                _ => {}
            }
        }
        core::mem::swap(&mut ws.cur, &mut ws.nxt);
        core::mem::swap(&mut ws.vc, &mut ws.vn);
        if ws.cur.is_empty() {
            break;
        }
        pos -= 1;
    }
    // No begin-of-input tail handler: every valid dispatch happens
    // in-loop at pos >= 0 (a consume checks its landing target before
    // scheduling), so threads left in cur when pos went negative all
    // target pos < 0 — dead by construction.
    match_pos
}

/// OP_ANYCHAR reverse consume — the code point ENDING at `pos`.
fn dispatch_anychar_rev(
    ws: &mut Workspace,
    prog: &Program,
    s: &[u8],
    pos: i64,
    flags: u8,
    t_pc: usize,
    t_saves_id: u32,
) {
    if pos <= 0 {
        return;
    }
    let last = s[(pos - 1) as usize];
    let mut adv: i64 = 1;
    let mut cp = last as i32;
    if flags & RE_FLAG_U != 0 {
        let (dcp, dlen) = utf8_decode_cp_before(s, pos as usize);
        cp = dcp;
        adv = dlen as i64;
    }
    if flags & RE_FLAG_S == 0 && cp == '\n' as i32 {
        return;
    }
    add_thread_adv(
        ws,
        (t_pc + 1) as i32,
        prog,
        s,
        pos - adv,
        flags,
        t_saves_id,
        adv,
    );
}

/// OP_CLASS reverse consume. Byte-only leaf classes (chunk-10d
/// expansion output) test the single byte at `pos-1`; the cp-aware
/// path decodes the code point ending at `pos` under the u flag.
fn dispatch_class_rev(
    ws: &mut Workspace,
    prog: &Program,
    s: &[u8],
    pos: i64,
    flags: u8,
    t_pc: usize,
    t_saves_id: u32,
) {
    if pos <= 0 {
        return;
    }
    let ins = prog.insts[t_pc];
    let cc = &prog.classes[ins.a as usize];
    let last = s[(pos - 1) as usize];
    let mut adv: i64 = 1;
    let matched;
    if cc.byte_only {
        matched = cc.test(last);
    } else if flags & RE_FLAG_U != 0 {
        let (cp, dlen) = utf8_decode_cp_before(s, pos as usize);
        adv = dlen as i64;
        matched = cc.test_cp(cp);
    } else {
        matched = cc.test(last);
    }
    if matched {
        add_thread_adv(
            ws,
            (t_pc + 1) as i32,
            prog,
            s,
            pos - adv,
            flags,
            t_saves_id,
            adv,
        );
    }
}

/// OP_BACKREF reverse dispatch — the captured text must END at the
/// cursor, so bytes compare tail-first: `br_offset` counts matched
/// bytes from the capture's END. Undefined / empty captures hop
/// epsilon-style exactly like the forward VM (a lookbehind body
/// executes right-to-left, so `(?<=(\w)\1)`'s backref runs BEFORE
/// its group captures — undefined ⇒ trivially matches, the V8
/// behaviour — while `(?<=\1(\w))` sees a real capture).
fn handle_backref_rev(
    prog: &Program,
    s: &[u8],
    pos: i64,
    flags: u8,
    t: &Thread,
    cap_idx: i32,
    ws: &mut Workspace,
) {
    let slot_s = (2 * cap_idx) as usize;
    let slot_e = (2 * cap_idx + 1) as usize;
    let (cs, ce) = if cap_idx >= 1 {
        let row = ws.arena.get(t.saves_id);
        if slot_e < row.len() {
            (row[slot_s], row[slot_e])
        } else {
            (-1, -1)
        }
    } else {
        (-1, -1)
    };
    let cap_len = if cs < 0 || ce < 0 { 0 } else { ce - cs };
    if cap_len == 0 {
        // Epsilon-style — schedule pc+1 in cur at the same pos.
        add_thread(
            &mut ws.cur,
            &mut ws.vc,
            &mut ws.arena,
            (t.pc + 1) as i32,
            prog,
            s,
            pos,
            flags,
            t.saves_id,
        );
        return;
    }
    if pos > 0
        && char_eq(
            s[(ce - 1 - t.br_offset as i64) as usize],
            s[(pos - 1) as usize],
            flags,
        )
    {
        let new_offset = t.br_offset + 1;
        if (new_offset as i64) == cap_len {
            // Backref complete — advance pc, step left.
            add_thread(
                &mut ws.nxt,
                &mut ws.vn,
                &mut ws.arena,
                (t.pc + 1) as i32,
                prog,
                s,
                pos - 1,
                flags,
                t.saves_id,
            );
        } else {
            // Continue same pc next step with offset bumped.
            ws.nxt.push(Thread {
                pc: t.pc,
                br_offset: new_offset,
                u_skip: 0,
                saves_id: t.saves_id,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::compile_rev;
    use crate::parser::Parser;
    use crate::program::Inst;

    /// Build a reverse-compiled program for `pat` (as a lookbehind
    /// BODY — no implicit anchoring) and probe it ending at `pos`.
    fn build_rev(pat: &str, flags: u8) -> Program {
        let mut p = Parser::new(pat.as_bytes(), flags);
        let root = p.parse().expect("parse failed");
        let mut prog = Program::new();
        compile_rev(&mut prog, &root, flags);
        prog.emit(Inst::match_accept());
        prog.has_save = prog.any_save();
        prog
    }

    fn probe(pat: &str, hay: &str, pos: i64, flags: u8) -> (i64, [i64; 16]) {
        let prog = build_rev(pat, flags);
        let mut ws = Workspace::for_program(&prog);
        let mut saves = [-1i64; 16];
        let j = vm_match_at_rev(&prog, hay.as_bytes(), pos, flags, &mut ws, Some(&mut saves));
        (j, saves)
    }

    #[test]
    fn literal_reversed_matches_ending_at_pos() {
        // Body "ab" ending at pos 2 of "abc".
        let (j, _) = probe("ab", "abc", 2, 0);
        assert_eq!(j, 0);
        // Not ending at pos 3 ("bc" ≠ "ab").
        let (j, _) = probe("ab", "abc", 3, 0);
        assert_eq!(j, -1);
    }

    #[test]
    fn greedy_plus_commits_longest_run_and_captures() {
        // `(?<=(\d+))px` shape: body `(\d+)` ending at pos 2 of "30px"
        // — greedy must answer "30", not "0".
        let (j, saves) = probe("(\\d+)", "30px", 2, 0);
        assert_eq!(j, 0);
        assert_eq!(saves[2], 0);
        assert_eq!(saves[3], 2);
    }

    #[test]
    fn lazy_plus_commits_shortest_run() {
        let (j, saves) = probe("(\\d+?)", "30px", 2, 0);
        assert_eq!(j, 1);
        assert_eq!(saves[2], 1);
        assert_eq!(saves[3], 2);
    }

    #[test]
    fn alternation_first_branch_priority() {
        // `(ab|b)` ending at pos 2 of "abc": first alternative wins.
        let (j, saves) = probe("(ab|b)", "abc", 2, 0);
        assert_eq!(j, 0);
        assert_eq!((saves[2], saves[3]), (0, 2));
    }

    #[test]
    fn alternation_falls_back_when_first_fails() {
        // `(a|ab)` ending at pos 2 of "abc": branch `a` needs s[1]=='a'
        // — fails; branch `ab` matches. bun/V8 answer "ab".
        let (j, saves) = probe("(a|ab)", "abc", 2, 0);
        assert_eq!(j, 0);
        assert_eq!((saves[2], saves[3]), (0, 2));
    }

    #[test]
    fn alternation_first_branch_wins_even_if_shorter() {
        // `(b|ab)` ending at pos 2 of "abc": FIRST branch `b`
        // completes ⇒ wins over the longer second branch
        // (backtracking order, not longest-match).
        let (j, saves) = probe("(b|ab)", "abc", 2, 0);
        assert_eq!(j, 1);
        assert_eq!((saves[2], saves[3]), (1, 2));
    }

    #[test]
    fn anchor_and_star_capture_full_prefix() {
        // `^(a*)` ending at pos 3 of "aaab" — anchors are positional,
        // direction-free; greedy a* walks to pos 0 where ^ holds.
        let (j, saves) = probe("^(a*)", "aaab", 3, 0);
        assert_eq!(j, 0);
        assert_eq!((saves[2], saves[3]), (0, 3));
    }

    #[test]
    fn backref_after_group_in_reverse_order() {
        // `\1(\w)` ending at pos 2 of "aax": reverse execution runs
        // `(\w)` FIRST (captures s[1]='a'), then `\1` compares s[0].
        let (j, saves) = probe("\\1(\\w)", "aax", 2, 0);
        assert_eq!(j, 0);
        assert_eq!((saves[2], saves[3]), (1, 2));
    }

    #[test]
    fn backref_before_group_is_epsilon() {
        // `(\w)\1` in reverse runs `\1` first — group not yet captured
        // ⇒ epsilon hop (V8 behaviour). Body then just consumes one \w.
        let (j, saves) = probe("(\\w)\\1", "aax", 2, 0);
        assert_eq!(j, 1);
        assert_eq!((saves[2], saves[3]), (1, 2));
    }

    #[test]
    fn nested_groups_swap_slots_correctly() {
        // `((a)(b))` ending at pos 2 of "ab" — all three groups get
        // forward-oriented [start, end) pairs despite reverse walk.
        let (j, saves) = probe("((a)(b))", "ab", 2, 0);
        assert_eq!(j, 0);
        assert_eq!((saves[2], saves[3]), (0, 2));
        assert_eq!((saves[4], saves[5]), (0, 1));
        assert_eq!((saves[6], saves[7]), (1, 2));
    }

    #[test]
    fn uflag_anychar_steps_one_cp_leftwards() {
        // "a😀" — `.` under u ending at pos 5 consumes the 4-byte cp.
        let (j, _) = probe(".", "a😀", 5, RE_FLAG_U);
        assert_eq!(j, 1);
    }

    #[test]
    fn uflag_property_class_tests_cp() {
        // `\p{L}` (K-PROPERTY, cp-aware) ending at pos 3 of "中".
        let (j, _) = probe("\\p{L}", "中", 3, RE_FLAG_U);
        assert_eq!(j, 0);
    }

    #[test]
    fn uflag_expanded_class_consumes_bytes_reversed() {
        // `[^a]` under u triggers the chunk-10d Alt-of-Concat byte
        // expansion; rev compile reverses each Concat so the byte
        // sequence of a multi-byte cp is consumed right-to-left.
        let (j, _) = probe("[^a]", "中", 3, RE_FLAG_U);
        assert_eq!(j, 0);
        let (j, _) = probe("[^a]", "xa", 2, RE_FLAG_U);
        assert_eq!(j, -1);
    }

    #[test]
    fn word_boundary_positional_in_reverse() {
        // `\bword` ending at pos 8 of "the word" — \b at pos 4 holds.
        let (j, _) = probe("\\bword", "the word", 8, 0);
        assert_eq!(j, 4);
        let (j, _) = probe("\\bord", "the word", 8, 0);
        assert_eq!(j, -1);
    }

    #[test]
    fn miss_returns_minus_one() {
        let (j, saves) = probe("(\\d+)", "abcd", 2, 0);
        assert_eq!(j, -1);
        assert_eq!(saves[2], -1);
    }

    #[test]
    fn empty_body_matches_at_pos() {
        let (j, _) = probe("a?", "bc", 1, 0);
        assert_eq!(j, 1);
    }
}
