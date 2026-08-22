//! `vm_match_at` — inner Pike NFA loop. Port of
//! `runtime_regex.c` L1779-2011.
//!
//! Drives the cur/nxt thread-list swap one input position at a time,
//! dispatching input-consuming ops (`CHAR`, `ANYCHAR`, `CLASS`,
//! `BACKREF`) and recording `MATCH` outcomes. Epsilon ops are
//! resolved transitively by [`add_thread`](super::dispatch::add_thread).

use super::{Workspace, char_eq};
use crate::parser::{RE_FLAG_S, unicode_mode};
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
///   commits (used by the DFA hit path's second-pass capture
///   extraction, `search::dfa_hit_result`).
#[allow(clippy::too_many_arguments)]
pub fn vm_match_at(
    prog: &Program,
    s: &[u8],
    start_pos: i64,
    flags: u8,
    ws: &mut Workspace,
    mut out_saves: Option<&mut [i64]>,
    end_target: i64,
) -> i64 {
    let slen = s.len() as i64;

    ws.cur.clear();
    ws.cur.step_id = ws.next_step_id();
    ws.vc.begin_step();
    ws.arena.reset();
    let seed_saves_id = ws.arena.alloc_empty();
    // Seed PC=0 through the epsilon expander.
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

    let mut end_pos: i64 = -1;

    let mut pos = start_pos;
    while pos <= slen {
        if end_target >= 0 && pos > end_target {
            break;
        }
        ws.nxt.clear();
        ws.nxt.step_id = ws.next_step_id();
        ws.vn.begin_step();
        let mut saw_match_this_step = false;
        let mut ti = 0;
        // Iterate cur via index — body may push into cur (BACKREF
        // epsilon hop on empty capture) and we want to see those.
        while ti < ws.cur.list.len() && !saw_match_this_step {
            // V0.2 P14-S12 — saves arena. The pre-S10 shape cloned the
            // full 528-byte Thread; S10 borrow-split removed the clone
            // by passing the saves array by ref through NLL field-split
            // borrows. S12 collapses the saves payload entirely: Thread
            // is now 24 bytes (pc + br_offset + u_skip + saves_id), so
            // `nxt.push(Thread{..})` inside `add_thread`'s terminal arm
            // memcpys 24 bytes instead of 528. The capture-save rows
            // live in `ws.arena`; the per-Thread `saves_id` indexes
            // into it. Hot-loop arms forward `saves_id` by value (u32);
            // only `Op::Save` allocates a fresh row via `alloc_clone`.
            let t_pc = ws.cur.list[ti].pc;
            let t_br_offset = ws.cur.list[ti].br_offset;
            let t_u_skip = ws.cur.list[ti].u_skip;
            let t_saves_id = ws.cur.list[ti].saves_id;
            ti += 1;
            if t_u_skip > 0 {
                // u_skip defer — pass-through to nxt with skip
                // decremented. Bypasses the visited table on
                // purpose so the deferred thread isn't dropped.
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
                    if pos < slen && char_eq(ins.ch, s[pos as usize], ins.pad as u8) {
                        add_thread(
                            &mut ws.nxt,
                            &mut ws.vn,
                            &mut ws.arena,
                            (t_pc + 1) as i32,
                            prog,
                            s,
                            pos + 1,
                            flags,
                            t_saves_id,
                        );
                    }
                }
                Op::AnyChar => dispatch_anychar(ws, prog, s, pos, flags, t_pc, t_saves_id),
                Op::Class => dispatch_class(ws, prog, s, pos, flags, t_pc, t_saves_id),
                Op::Backref => {
                    // BACKREF needs scalar Thread fields + saves_id;
                    // handle_backref reads saves via the arena and may
                    // schedule into ws.cur (alias-free via field-split).
                    let t = Thread {
                        pc: t_pc,
                        br_offset: t_br_offset,
                        u_skip: t_u_skip,
                        saves_id: t_saves_id,
                    };
                    handle_backref(prog, s, pos, flags, &t, ins.a, ws);
                }
                Op::Match => {
                    // Normal mode: leftmost-first wins; stop scanning
                    // this step. Length-restricted (DFA capture
                    // extraction): only commit if pos meets
                    // end_target; otherwise the thread is dead but
                    // keep scanning — other threads may extend
                    // further via nxt and MATCH later.
                    if end_target < 0 || pos == end_target {
                        saw_match_this_step = true;
                        end_pos = pos;
                        if let Some(ref mut o) = out_saves {
                            // Arena row and caller buffer are both
                            // `stride`-wide (chunk 801 — the caller
                            // allocates `vec![-1; stride]`); min-len
                            // copy keeps any tail slots at the
                            // caller's sentinel value.
                            let row = ws.arena.get(t_saves_id);
                            let n = row.len().min(o.len());
                            (*o)[..n].copy_from_slice(&row[..n]);
                        }
                    }
                }
                _ => {}
            }
        }
        // Swap cur <-> nxt + their visited tables.
        core::mem::swap(&mut ws.cur, &mut ws.nxt);
        core::mem::swap(&mut ws.vc, &mut ws.vn);
        if ws.cur.is_empty() {
            break;
        }
        pos += 1;
    }

    if let Some(e) = match_at_eoi(prog, ws, slen, end_target, out_saves) {
        end_pos = e;
    }
    end_pos
}

/// Schedule `pc` at `pos_next` into `ws.nxt`, marking the scheduled
/// thread(s) with `u_skip = adv - 1` when the consumed unit was a
/// multi-byte code point (u-flag cp step) — they sit (adv-1) outer
/// steps before dispatching. Direction-neutral (`pos_next` is
/// computed by the caller), so the reverse VM
/// ([`super::match_at_rev`]) reuses it as-is.
#[allow(clippy::too_many_arguments)]
pub(super) fn add_thread_adv(
    ws: &mut Workspace,
    pc: i32,
    prog: &Program,
    s: &[u8],
    pos_next: i64,
    flags: u8,
    saves_id: u32,
    adv: i64,
) {
    let n_before = ws.nxt.list.len();
    add_thread(
        &mut ws.nxt,
        &mut ws.vn,
        &mut ws.arena,
        pc,
        prog,
        s,
        pos_next,
        flags,
        saves_id,
    );
    if adv > 1 {
        let skip = (adv - 1) as i32;
        for th in &mut ws.nxt.list[n_before..] {
            th.u_skip = skip;
        }
    }
}

/// OP_ANYCHAR consume. Under u flag `.` consumes 1 code point;
/// schedule the destination thread(s) with u_skip = adv-1 so they
/// sit (adv-1) outer steps before dispatching pc+1. dotAll is the
/// instruction's baked `pad` s-bit (regexp-modifiers), not the
/// global flag word.
fn dispatch_anychar(
    ws: &mut Workspace,
    prog: &Program,
    s: &[u8],
    pos: i64,
    flags: u8,
    t_pc: usize,
    t_saves_id: u32,
) {
    let slen = s.len() as i64;
    let ins = prog.insts[t_pc];
    if pos < slen && (ins.pad as u8 & RE_FLAG_S != 0 || s[pos as usize] != b'\n') {
        // One character, in either mode — see the matching note in
        // `dfa::step`. Without this the non-u match boundary landed
        // inside a multi-byte character.
        let mut adv: i64 = 1;
        let ul = utf8_len_for(s[pos as usize]) as i64;
        if ul >= 1 && pos + ul <= slen {
            adv = ul;
        }
        add_thread_adv(
            ws,
            (t_pc + 1) as i32,
            prog,
            s,
            pos + adv,
            flags,
            t_saves_id,
            adv,
        );
    }
}

/// OP_CLASS consume. chunk 10d — byte-only leaf classes (emitted by
/// `utf8_class_expand` when rewriting a u-flag unsafe class into a
/// byte-level Alt-of-Concat) step a single haystack byte regardless
/// of `u`. The cp-aware path stays for hand-written classes whose
/// semantics expect "decode one cp at the cursor".
///
/// ignoreCase goes through `CharClass::test_fold` — the pre-negate
/// ASCII case-pair fold shared with the DFA byte-step (the VM
/// previously skipped the fold entirely, a silent-wrong on every
/// VM-served `/[a-z]/i` shape: lookaround / backref programs and the
/// DFA-hit second-pass capture extraction).
fn dispatch_class(
    ws: &mut Workspace,
    prog: &Program,
    s: &[u8],
    pos: i64,
    flags: u8,
    t_pc: usize,
    t_saves_id: u32,
) {
    let slen = s.len() as i64;
    if pos >= slen {
        return;
    }
    let ins = prog.insts[t_pc];
    let cc = &prog.classes[ins.a as usize];
    let ci = ins.pad as u8 & crate::parser::RE_FLAG_I != 0;
    let mut adv: i64 = 1;
    let matched;
    if cc.byte_only {
        matched = cc.test_fold(s[pos as usize], ci);
    } else if unicode_mode(flags) {
        let ul = utf8_len_for(s[pos as usize]) as i64;
        if ul >= 1 && pos + ul <= slen {
            let (cp, dec_len) = utf8_decode_cp(&s[pos as usize..]);
            adv = if dec_len > 0 { dec_len as i64 } else { ul };
            matched = cc.test_cp_fold(cp, ci);
        } else {
            matched = cc.test_fold(s[pos as usize], ci);
        }
    } else {
        matched = cc.test_fold(s[pos as usize], ci);
    }
    if matched {
        add_thread_adv(
            ws,
            (t_pc + 1) as i32,
            prog,
            s,
            pos + adv,
            flags,
            t_saves_id,
            adv,
        );
    }
}

/// End-of-input acceptance — any thread sitting on MATCH after the
/// outer loop (with no pending u_skip) also accepts. Writes the
/// winning thread's saves into `out_saves` like the in-loop arm.
fn match_at_eoi(
    prog: &Program,
    ws: &Workspace,
    slen: i64,
    end_target: i64,
    mut out_saves: Option<&mut [i64]>,
) -> Option<i64> {
    for ti in 0..ws.cur.list.len() {
        let t_pc = ws.cur.list[ti].pc;
        let t_u_skip = ws.cur.list[ti].u_skip;
        let t_saves_id = ws.cur.list[ti].saves_id;
        if prog.insts[t_pc].op == Op::Match as u8
            && t_u_skip == 0
            && (end_target < 0 || slen == end_target)
        {
            if let Some(ref mut o) = out_saves {
                let row = ws.arena.get(t_saves_id);
                let n = row.len().min(o.len());
                (*o)[..n].copy_from_slice(&row[..n]);
            }
            return Some(slen);
        }
    }
    None
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
        // Epsilon-style — schedule pc+1 in cur at the same pos. The
        // outer while-loop re-reads cur.list.len() each iteration so
        // the freshly-pushed thread is visited; visited-table dedup
        // prevents infinite insertion.
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
    if pos < slen
        && char_eq(
            s[(cs + t.br_offset as i64) as usize],
            s[pos as usize],
            prog.insts[t.pc].pad as u8,
        )
    {
        let new_offset = t.br_offset + 1;
        if (new_offset as i64) == cap_len {
            // Backref complete — advance pc, reset br_offset.
            add_thread(
                &mut ws.nxt,
                &mut ws.vn,
                &mut ws.arena,
                (t.pc + 1) as i32,
                prog,
                s,
                pos + 1,
                flags,
                t.saves_id,
            );
        } else {
            // Continue same pc next step with offset bumped. Direct
            // insert bypasses visited (different state from any
            // fresh entrant at pc).
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
    use crate::compiler::compile;
    use crate::parser::Parser;
    use crate::program::Inst;
    use crate::vm::{MatchResult, search_from};

    fn build(pat: &str, flags: u8) -> Program {
        let mut p = Parser::new(pat.as_bytes(), flags);
        let root = p.parse().expect("parse failed");
        let mut prog = Program::new();
        compile(&mut prog, &root, flags);
        prog.emit(Inst::match_accept());
        // Mirror `regex/compile.rs` / `vm/mod.rs::tests::build` —
        // `prog.has_save` is the compile-time cache of
        // `insts.iter().any(...Op::Save)`. Production callers always
        // set it; the chunk-7.7-step12 Round 3 Phase B sub-batch 5
        // attack #R-A3 (NoSaves/WithSaves split) wires the `search_*`
        // entry points to gate the 512-byte saves init on this field,
        // so tests that build a Program manually must set it too —
        // otherwise capture-group fixtures like `((a)(b))` take the
        // NoSaves fast path and surface all-`-1` saves.
        prog.has_save = prog
            .insts
            .iter()
            .any(|ins| ins.op == crate::program::Op::Save as u8);
        prog.finalize_backref_caps();
        prog
    }

    fn matches(pat: &str, hay: &str, flags: u8) -> Option<MatchResult> {
        let prog = build(pat, flags);
        search_from(&prog, hay.as_bytes(), 0, flags, None)
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
        assert_eq!(r.saves()[2], 0);
        assert_eq!(r.saves()[3], 2);
        assert_eq!(r.saves()[4], 0);
        assert_eq!(r.saves()[5], 1);
        assert_eq!(r.saves()[6], 1);
        assert_eq!(r.saves()[7], 2);
    }

    #[test]
    fn empty_capture_backref_matches() {
        // Empty capture group — backref to it should match
        // epsilon-style.
        let r = matches("(a?)\\1b", "b", 0).expect("hit");
        assert_eq!(r.start, 0);
        assert_eq!(r.end, 1);
    }

    #[test]
    fn backref_shorter_capture_fallback_survives_dedup() {
        // RFC 20260721-string-proto-cluster knife 9 (G7) —
        // `/^(a+)\1*,\1+$/` on "10 a's , 15 a's" needs group 1 to
        // fall back to 5 a's: the greedy 10-capture thread dies at
        // `\1+` over 15 a's (15 % 10 != 0), and PC-only visited
        // dedup used to swallow the shorter-capture candidates at
        // the shared post-group PCs → whole-pattern no-match.
        let r = matches("^(a+)\\1*,\\1+$", "aaaaaaaaaa,aaaaaaaaaaaaaaa", 0).expect("hit");
        assert_eq!(r.start, 0);
        assert_eq!(r.end, 26);
        // Group 1 committed the longest viable prefix: "aaaaa".
        assert_eq!(r.saves()[2], 0);
        assert_eq!(r.saves()[3], 5);
    }

    #[test]
    fn backref_greedy_priority_kept_under_span_dedup() {
        // Span-keyed dedup must not disturb leftmost-first greed:
        // `(a+)\1` on "aaaa" still commits the longest split (2+2).
        let r = matches("(a+)\\1$", "aaaa", 0).expect("hit");
        assert_eq!(r.saves()[2], 0);
        assert_eq!(r.saves()[3], 2);
    }
}
