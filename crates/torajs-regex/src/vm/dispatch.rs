//! Epsilon-op expansion + lookaround probes — port of
//! `runtime_regex.c` L1648-1777.
//!
//! `add_thread` walks the epsilon transitions reachable from a PC
//! and seeds the resulting "real" waiting-for-input PCs into the
//! caller's [`ThreadList`]. Each thread carries its own snapshot of
//! the capture-save state; SPLIT forks each get a fresh copy so a
//! SAVE in one branch doesn't leak into the other.
//!
//! `sub_probe` / `sub_probe_ending_at` run a sub-Program (the body
//! of a lookahead / lookbehind) to satisfy the zero-width assertion
//! at the parent's add_thread call site. Both allocate their own
//! workspace because they recurse — they can't share the parent's.

use super::{SavesArena, ThreadList, VisitedTable, Workspace};
use crate::parser::{RE_FLAG_M, is_word_byte};
use crate::program::{Op, Program};
use crate::vm::Thread;
use crate::vm::match_at::vm_match_at;

/// Transitively expand epsilon ops reachable from `pc` and enqueue
/// the resulting waiting threads into `tl`. Visited-table dedup
/// keeps the recursion bounded (each PC processed once per step).
///
/// `saves_id` is an arena handle (V0.2 P14-S12): epsilon hops forward
/// the same handle (no copy); `Op::Save` allocates a fresh row via
/// `arena.alloc_clone` so the SPLIT-fork branch isolation invariant
/// from the pre-S12 inline-saves shape is preserved.
#[allow(clippy::too_many_arguments)]
pub(super) fn add_thread(
    tl: &mut ThreadList,
    vt: &mut VisitedTable,
    arena: &mut SavesArena,
    pc: i32,
    prog: &Program,
    s: &[u8],
    pos: i64,
    flags: u8,
    saves_id: u32,
) {
    if pc < 0 || (pc as usize) >= prog.len() {
        return;
    }
    let upc = pc as usize;
    if vt.visited[upc] == tl.step_id {
        return;
    }
    vt.visited[upc] = tl.step_id;
    let ins = prog.insts[upc];
    let op = match Op::from_u8(ins.op) {
        Some(o) => o,
        None => return, // unknown opcode — defensive
    };
    let slen = s.len() as i64;
    match op {
        Op::Jmp => add_thread(tl, vt, arena, ins.a, prog, s, pos, flags, saves_id),
        Op::Split => {
            add_thread(tl, vt, arena, ins.a, prog, s, pos, flags, saves_id);
            add_thread(tl, vt, arena, ins.b, prog, s, pos, flags, saves_id);
        }
        Op::Save => {
            let slot = ins.a;
            let new_id = arena.alloc_clone(saves_id);
            if slot >= 0 && (slot as usize) < arena.stride {
                arena.write_slot(new_id, slot as usize, pos);
            }
            add_thread(tl, vt, arena, pc + 1, prog, s, pos, flags, new_id);
        }
        Op::AnchorB => {
            let ok =
                pos == 0 || (flags & RE_FLAG_M != 0 && pos > 0 && s[(pos - 1) as usize] == b'\n');
            if ok {
                add_thread(tl, vt, arena, pc + 1, prog, s, pos, flags, saves_id);
            }
        }
        Op::AnchorE => {
            let ok =
                pos == slen || (flags & RE_FLAG_M != 0 && pos < slen && s[pos as usize] == b'\n');
            if ok {
                add_thread(tl, vt, arena, pc + 1, prog, s, pos, flags, saves_id);
            }
        }
        Op::WBound => {
            let left = pos > 0 && is_word_byte(s[(pos - 1) as usize]);
            let right = pos < slen && is_word_byte(s[pos as usize]);
            if left != right {
                add_thread(tl, vt, arena, pc + 1, prog, s, pos, flags, saves_id);
            }
        }
        Op::NWBound => {
            let left = pos > 0 && is_word_byte(s[(pos - 1) as usize]);
            let right = pos < slen && is_word_byte(s[pos as usize]);
            if left == right {
                add_thread(tl, vt, arena, pc + 1, prog, s, pos, flags, saves_id);
            }
        }
        Op::Lookahead => {
            let sub = &prog.sub_progs[ins.a as usize];
            if sub_probe(sub, s, pos, flags) {
                add_thread(tl, vt, arena, pc + 1, prog, s, pos, flags, saves_id);
            }
        }
        Op::NegLookahead => {
            let sub = &prog.sub_progs[ins.a as usize];
            if !sub_probe(sub, s, pos, flags) {
                add_thread(tl, vt, arena, pc + 1, prog, s, pos, flags, saves_id);
            }
        }
        Op::Lookbehind => {
            let sub = &prog.sub_progs[ins.a as usize];
            if sub_probe_ending_at(sub, s, pos, flags) {
                add_thread(tl, vt, arena, pc + 1, prog, s, pos, flags, saves_id);
            }
        }
        Op::NegLookbehind => {
            let sub = &prog.sub_progs[ins.a as usize];
            if !sub_probe_ending_at(sub, s, pos, flags) {
                add_thread(tl, vt, arena, pc + 1, prog, s, pos, flags, saves_id);
            }
        }
        // Real, input-consuming op — terminate the epsilon chain and
        // park the thread waiting for the inner-loop dispatcher.
        _ => {
            tl.push(Thread {
                pc: upc,
                br_offset: 0,
                u_skip: 0,
                saves_id,
            });
        }
    }
}

/// `sub_probe` — does the sub-program have ANY match starting at
/// `pos`? Used by lookahead. Allocates its own workspace.
pub fn sub_probe(sub: &Program, s: &[u8], pos: i64, flags: u8) -> bool {
    if sub.is_empty() {
        return true;
    }
    let mut ws = Workspace::for_program(sub);
    let end = vm_match_at(sub, s, pos, flags, &mut ws, None, -1);
    end >= 0
}

/// `sub_probe_ending_at` — does the sub-program have ANY match
/// `s[j..pos]` for some `0 ≤ j ≤ pos`? Used by lookbehind. O(pos ×
/// sub_len) worst case; in practice the body is short and the loop
/// bails on the first feasible j. Future P14+ perf path: compile
/// the body backwards and scan reverse (same approach as V8) —
/// localized to this fn; AST / op / parser stay put.
pub fn sub_probe_ending_at(sub: &Program, s: &[u8], pos: i64, flags: u8) -> bool {
    if sub.is_empty() {
        return true;
    }
    let mut ws = Workspace::for_program(sub);
    let mut j = pos;
    while j >= 0 {
        let end = vm_match_at(sub, s, j, flags, &mut ws, None, pos);
        if end == pos {
            return true;
        }
        j -= 1;
    }
    false
}
