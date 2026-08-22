//! Byte-step transitions — chunks 8.7 / 10a / 10b.
//!
//! Given an ε-closed PC set + the cursor byte + active flag bits,
//! return `(ready, deferred)`: `ready` is the set of PCs advanced
//! one byte and now sitting at the next cursor; `deferred[i]` is
//! the set of PCs that consumed a multi-byte UTF-8 first byte
//! under `RE_FLAG_U` (`Op::AnyChar`) and must wait `i + 1`
//! continuation bytes before becoming ready. For non-u-flag
//! patterns `deferred` is always all-empty so the BFS state pool
//! dedup matches the pre-10b shape.
//!
//! Split out of `dfa/mod.rs` (chunk 10d follow-up) to keep the
//! parent module under the 500-line HARD limit as more substrate
//! lands. `byte_step` / `byte_step_full` are the only inline
//! functions here; `epsilon_closure` lives in [`super::ctx`] and the
//! BFS driver in [`super::build`].

use alloc::collections::BTreeSet;

use crate::program::{Op, Program};

/// One byte-transition step over an ε-closed PC set, returning only
/// the ready PCs (those that advance to `cursor + 1`). Thin wrapper
/// around [`byte_step_full`] discarding the deferred buckets — used
/// by callers that don't model u-flag multi-byte deferral.
///
/// `Op::Char` uses [`crate::vm::char_eq`] (case-pair under the
/// inst's baked i-bit); `Op::AnyChar` advances on any byte except
/// `\n` (0x0A) when the inst's s-bit is unset (chunk 10a — the JS
/// spec says `.` does not match line terminators unless `s` is set);
/// `Op::Class` goes through `CharClass::test_fold` (pre-negate
/// case-pair fold). Other ops are terminal — ε via the BFS's closure pass,
/// lookaround / backref filtered upstream in [`super::analyze`].
/// Out-of-range PCs (defensive) and `pc + 1` past program end are
/// silently dropped.
pub fn byte_step(prog: &Program, states: &BTreeSet<usize>, byte: u8) -> BTreeSet<usize> {
    byte_step_full(prog, states, byte).0
}

/// Full byte-step: returns `(ready, deferred)` where `deferred[i]`
/// is the set of PCs scheduled to become ready in `i + 1`
/// continuation bytes (chunk 10b — `Op::AnyChar` schedules the
/// post-`.` PC behind the UTF-8 multi-byte tail).
pub fn byte_step_full(
    prog: &Program,
    states: &BTreeSet<usize>,
    byte: u8,
) -> (BTreeSet<usize>, [BTreeSet<usize>; 3]) {
    let mut ready: BTreeSet<usize> = BTreeSet::new();
    let mut deferred: [BTreeSet<usize>; 3] = Default::default();
    let plen = prog.len();
    for &pc in states.iter() {
        if pc >= plen {
            continue;
        }
        let ins = prog.insts[pc];
        let op = match Op::from_u8(ins.op) {
            Some(o) => o,
            None => continue,
        };
        match op {
            Op::Char => {
                if crate::vm::char_eq(ins.ch, byte, ins.pad as u8) {
                    let n = pc + 1;
                    if n < plen {
                        ready.insert(n);
                    }
                }
            }
            Op::AnyChar => {
                let s_flag = ins.pad as u8 & crate::parser::RE_FLAG_S != 0;
                if !(s_flag || byte != b'\n') {
                    continue;
                }
                let n = pc + 1;
                if n >= plen {
                    continue;
                }
                // A multi-byte first byte parks PC behind the UTF-8
                // tail (matches NFA `utf8_len_for` defer in
                // `match_at`). ASCII / invalid first bytes (incl.
                // 0xC0/0xC1/0x80..0xBF/0xF5..0xFF) advance 1 byte so
                // the matcher's cursor keeps progressing.
                //
                // Not gated on the u flag: `.` means one character in
                // either mode, and stopping after the first byte of
                // one left the match boundary inside a character —
                // `"日X".match(/./)` took the string layer down with
                // it rather than answering `["日"]`.
                let u_skip = match byte {
                    0xC2..=0xDF => 1,
                    0xE0..=0xEF => 2,
                    0xF0..=0xF4 => 3,
                    _ => 0,
                };
                if u_skip == 0 {
                    ready.insert(n);
                } else {
                    deferred[u_skip - 1].insert(n);
                }
            }
            Op::Class => {
                let cls_idx = ins.a as usize;
                if cls_idx < prog.classes.len() {
                    let class = &prog.classes[cls_idx];
                    let ci = ins.pad as u8 & crate::parser::RE_FLAG_I != 0;
                    if class.test_fold(byte, ci) {
                        let n = pc + 1;
                        if n < plen {
                            ready.insert(n);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    (ready, deferred)
}
