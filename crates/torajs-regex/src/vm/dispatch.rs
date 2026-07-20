//! Epsilon-op expansion + lookaround probes — port of
//! `runtime_regex.c` L1648-1777.
//!
//! `add_thread` walks the epsilon transitions reachable from a PC
//! and seeds the resulting "real" waiting-for-input PCs into the
//! caller's [`ThreadList`]. Each thread carries its own snapshot of
//! the capture-save state; SPLIT forks each get a fresh copy so a
//! SAVE in one branch doesn't leak into the other.
//!
//! `sub_probe` / `sub_probe_rev` run a sub-Program (the body of a
//! lookahead / lookbehind) to satisfy the zero-width assertion at
//! the parent's add_thread call site. Lookbehind bodies are
//! reverse-compiled ([`crate::compiler::compile_rev`]) and probed
//! leftwards by the reverse Pike VM. All probes allocate their own
//! workspace because they recurse — they can't share the parent's.

use super::{SavesArena, ThreadList, VisitedTable, Workspace};
use crate::parser::{RE_FLAG_M, is_word_byte};
use crate::program::{Op, Program};
use crate::vm::Thread;
use crate::vm::match_at::vm_match_at;
use crate::vm::match_at_rev::vm_match_at_rev;
use alloc::vec;
use alloc::vec::Vec;

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
    if prog.backref_caps.is_empty() {
        if vt.visited[upc] == tl.step_id {
            return;
        }
        vt.visited[upc] = tl.step_id;
    } else {
        // Backref-carrying program — dedup on (pc, spans of every
        // backref-referenced group). PC-only first-write-wins would
        // swallow viable lower-priority capture candidates whose
        // greedy sibling dies later (`/^(a+)\1*,\1+$/`). Epsilon
        // cycles still terminate: `Op::Save` writes the fixed
        // current `pos`, so the key domain at one position is
        // finite and a second lap collides here.
        let row = arena.get(saves_id);
        let mut key = Vec::with_capacity(2 * prog.backref_caps.len());
        for &g in &prog.backref_caps {
            let s_slot = (2 * g) as usize;
            key.push(row.get(s_slot).copied().unwrap_or(-1));
            key.push(row.get(s_slot + 1).copied().unwrap_or(-1));
        }
        if !vt.seen.insert((upc, key)) {
            return;
        }
    }
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
            // multiline is the instruction's baked `pad` m-bit
            // (regexp-modifiers), not the global flag word.
            let ml = ins.pad as u8 & RE_FLAG_M != 0;
            let ok = pos == 0 || (ml && pos > 0 && s[(pos - 1) as usize] == b'\n');
            if ok {
                add_thread(tl, vt, arena, pc + 1, prog, s, pos, flags, saves_id);
            }
        }
        Op::AnchorE => {
            let ml = ins.pad as u8 & RE_FLAG_M != 0;
            let ok = pos == slen || (ml && pos < slen && s[pos as usize] == b'\n');
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
            // Chunk 801 — stride-sized row (the fixed 64-slot stack
            // buffer scaled with the retired capture cap); slot
            // indices are global so the parent stride bounds every
            // sub-program SAVE.
            let mut sub_saves = vec![-1i64; arena.stride];
            if sub_probe_saving(sub, s, pos, flags, &mut sub_saves) {
                // ES §22.2.2 — a successful lookahead KEEPS the
                // captures its body wrote (`/(?=(a+))/` answers
                // group 1). Slots are global, so merge them into
                // this thread's row.
                let merged = merge_sub_saves(arena, saves_id, &sub_saves);
                add_thread(tl, vt, arena, pc + 1, prog, s, pos, flags, merged);
            }
        }
        Op::NegLookahead => {
            let sub = &prog.sub_progs[ins.a as usize];
            if !sub_probe(sub, s, pos, flags) {
                add_thread(tl, vt, arena, pc + 1, prog, s, pos, flags, saves_id);
            }
        }
        Op::Lookbehind => {
            // The body is reverse-compiled (ES §22.2.2 MatchReverse;
            // V8 irregexp shape) and probed leftwards from `pos`. A
            // successful lookbehind KEEPS the captures its body wrote
            // (`/(?<=(\d+))px/` answers group 1) — merged into this
            // thread's row exactly like the lookahead arm.
            let sub = &prog.sub_progs[ins.a as usize];
            let mut sub_saves = vec![-1i64; arena.stride];
            if sub_probe_rev_saving(sub, s, pos, flags, &mut sub_saves) {
                let merged = merge_sub_saves(arena, saves_id, &sub_saves);
                add_thread(tl, vt, arena, pc + 1, prog, s, pos, flags, merged);
            }
        }
        Op::NegLookbehind => {
            let sub = &prog.sub_progs[ins.a as usize];
            if !sub_probe_rev(sub, s, pos, flags) {
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
/// `pos`? Used by the NEGATIVE lookahead (a failed assertion never
/// contributes captures per ES §22.2.2). Allocates its own workspace.
pub fn sub_probe(sub: &Program, s: &[u8], pos: i64, flags: u8) -> bool {
    if sub.is_empty() {
        return true;
    }
    let mut ws = Workspace::for_program(sub);
    let end = vm_match_at(sub, s, pos, flags, &mut ws, None, -1);
    end >= 0
}

/// [`sub_probe`] with capture write-back — the positive lookahead
/// keeps its body's SAVEs. `out` must be pre-filled with `-1`.
fn sub_probe_saving(sub: &Program, s: &[u8], pos: i64, flags: u8, out: &mut [i64]) -> bool {
    if sub.is_empty() {
        return true;
    }
    let mut ws = Workspace::for_program(sub);
    vm_match_at(sub, s, pos, flags, &mut ws, Some(out), -1) >= 0
}

/// Merge the written (non-`-1`) slots of a successful lookaround
/// body into `saves_id`, forking a fresh row on the first hit so
/// sibling threads keep their own snapshots. Bodies without
/// captures (all `-1`) forward the original handle — zero cost.
/// Slot indices are global (parser-assigned across the whole
/// pattern) and `detect_stride` counts sub-program SAVEs, so every
/// written slot fits the parent row.
fn merge_sub_saves(arena: &mut SavesArena, saves_id: u32, sub_saves: &[i64]) -> u32 {
    let stride = arena.stride;
    let mut merged = saves_id;
    let mut forked = false;
    for (slot, &val) in sub_saves.iter().enumerate().take(stride) {
        if val < 0 {
            continue;
        }
        if !forked {
            merged = arena.alloc_clone(saves_id);
            forked = true;
        }
        arena.write_slot(merged, slot, val);
    }
    merged
}

/// `sub_probe_rev` — does the reverse-compiled sub-program have ANY
/// match ENDING at `pos`? Used by the NEGATIVE lookbehind (a failed
/// assertion never contributes captures per ES §22.2.2). Replaces
/// the retired `sub_probe_ending_at` forward start-scan (O(pos ×
/// sub_len)) with a single leftwards run (O(sub_len)).
pub fn sub_probe_rev(sub: &Program, s: &[u8], pos: i64, flags: u8) -> bool {
    if sub.is_empty() {
        return true;
    }
    let mut ws = Workspace::for_program(sub);
    vm_match_at_rev(sub, s, pos, flags, &mut ws, None) >= 0
}

/// [`sub_probe_rev`] with capture write-back — the positive
/// lookbehind keeps its body's SAVEs. `out` must be pre-filled with
/// `-1`.
fn sub_probe_rev_saving(sub: &Program, s: &[u8], pos: i64, flags: u8, out: &mut [i64]) -> bool {
    if sub.is_empty() {
        return true;
    }
    let mut ws = Workspace::for_program(sub);
    vm_match_at_rev(sub, s, pos, flags, &mut ws, Some(out)) >= 0
}
