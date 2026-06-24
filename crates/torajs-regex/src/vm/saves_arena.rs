//! Capture-save row pool backing [`super::Thread::saves_id`].
//!
//! V0.2 P14-S12 introduced the arena (Thread shrinks 528 → 24 bytes).
//! V0.2 P14-S13 makes `stride` per-Program: `2 * (n_captures + 1)`
//! (slots 0/1 hold the whole-match start/end; slots `2*i` / `2*i+1`
//! hold capture group `i`'s start/end). For the common case of zero
//! or one user capture group, stride is 2 or 4 — vs. the pre-S13
//! fixed `REGEX_SAVE_SLOTS = 64`, that shrinks each `Op::Save`
//! `alloc_clone` row copy from 512 bytes to 16-32 bytes.

use crate::program::Program;
use alloc::vec::Vec;

/// Pool of capture-save rows. Each `alloc_*` returns a `u32` handle
/// indexing into a flat `Vec<i64>` of `stride`-wide rows.
#[derive(Debug)]
pub struct SavesArena {
    pub data: Vec<i64>,
    /// Width of each row in slots (`i64` count). Always
    /// `<= REGEX_SAVE_SLOTS`. Caller `out_saves` buffer stays a
    /// fixed `[i64; REGEX_SAVE_SLOTS]` so high-slot reads against a
    /// match without that many captures return the caller's `-1`
    /// init sentinel — `vm_match_at`'s writeback only fills
    /// `arena.get(id)[..stride]`.
    pub stride: usize,
}

impl SavesArena {
    pub fn with_capacity_and_stride(rows: usize, stride: usize) -> Self {
        Self {
            data: Vec::with_capacity(rows * stride),
            stride,
        }
    }

    pub fn reset(&mut self) {
        self.data.clear();
    }

    /// Allocate a fresh row initialised to `-1` (sentinel for
    /// "not captured"). Returns the row's handle.
    pub fn alloc_empty(&mut self) -> u32 {
        let stride = self.stride;
        let id = (self.data.len() / stride) as u32;
        let new_len = self.data.len() + stride;
        self.data.resize(new_len, -1);
        id
    }

    /// Allocate a fresh row pre-populated with a copy of `src_id`'s
    /// row. Used by `Op::Save` to fork a per-branch saves snapshot.
    pub fn alloc_clone(&mut self, src_id: u32) -> u32 {
        let stride = self.stride;
        let src_start = (src_id as usize) * stride;
        let new_start = self.data.len();
        let new_id = (new_start / stride) as u32;
        self.data.resize(new_start + stride, -1);
        self.data
            .copy_within(src_start..src_start + stride, new_start);
        new_id
    }

    /// Read-only access to a row.
    pub fn get(&self, id: u32) -> &[i64] {
        let stride = self.stride;
        let start = (id as usize) * stride;
        &self.data[start..start + stride]
    }

    /// Write a single slot.
    pub fn write_slot(&mut self, id: u32, slot: usize, val: i64) {
        let start = (id as usize) * self.stride;
        self.data[start + slot] = val;
    }
}

/// Scan a Program for the highest `OP_SAVE` slot reference and derive
/// the per-row stride of the saves arena (slot 0/1 = whole match, slot
/// `2*i`/`2*i+1` = capture group `i`). Returns the row width in `i64`
/// slots. Programs without any `OP_SAVE` get a stride of `2` (enough
/// to hold the implicit whole-match slots if SSA-lower later adds
/// them); the true minimum useful stride is 0 but a non-zero stride
/// keeps `alloc_*` from degenerating into no-op rows for the
/// never-takes-this-branch case.
pub(super) fn detect_stride(prog: &Program) -> usize {
    let mut max_slot: i32 = -1;
    for inst in &prog.insts {
        if inst.op == crate::program::Op::Save as u8 && inst.a > max_slot {
            max_slot = inst.a;
        }
    }
    if max_slot < 0 {
        2
    } else {
        (max_slot as usize) + 1
    }
}
