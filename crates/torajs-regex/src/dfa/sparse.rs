//! PC-set containers for the subset construction: a sparse set for
//! the ε-closure / byte-step scratch and an open-addressing map from
//! the sorted state key to the pool index. Both replace the
//! `BTreeSet<usize>` / `BTreeMap<StateKey, u32>` pair the BFS used to
//! rebuild per byte; neither changes which states are produced.

use alloc::collections::BTreeSet;
use alloc::vec;
use alloc::vec::Vec;

/// Anything the ε-closure walk can insert PCs into.
pub(super) trait PcSet {
    /// Returns true when `pc` was not already present.
    fn insert_pc(&mut self, pc: usize) -> bool;
}

impl PcSet for BTreeSet<usize> {
    fn insert_pc(&mut self, pc: usize) -> bool {
        self.insert(pc)
    }
}

/// Set of PCs below a fixed bound. Membership, insert and clear are
/// O(1); iteration order is insertion order (Briggs–Torczon).
pub(super) struct SparseSet {
    dense: Vec<u32>,
    sparse: Vec<u32>,
}

impl SparseSet {
    pub(super) fn new(bound: usize) -> Self {
        Self {
            dense: Vec::with_capacity(bound),
            sparse: vec![0; bound],
        }
    }

    pub(super) fn clear(&mut self) {
        self.dense.clear();
    }

    pub(super) fn as_slice(&self) -> &[u32] {
        &self.dense
    }

    /// Members in ascending order, written over `out`.
    pub(super) fn sorted_into(&self, out: &mut Vec<u32>) {
        out.clear();
        out.extend_from_slice(&self.dense);
        out.sort_unstable();
    }
}

impl PcSet for SparseSet {
    // `pc` is below the bound: every caller guards with `< prog.len()`
    fn insert_pc(&mut self, pc: usize) -> bool {
        let slot = self.sparse[pc] as usize;
        if slot < self.dense.len() && self.dense[slot] as usize == pc {
            return false;
        }
        self.sparse[pc] = self.dense.len() as u32;
        self.dense.push(pc as u32);
        true
    }
}

/// Map from state key to pool index. The keys themselves live in the
/// pool (`keys[idx]`); the table only stores `idx + 1` per slot, 0
/// meaning empty. Linear probing, grown at half load.
pub(super) struct KeyMap {
    slots: Vec<u32>,
    len: usize,
}

fn hash_key(key: &[u32]) -> u64 {
    let mut h: u64 = key.len() as u64;
    for &w in key {
        h = (h.rotate_left(5) ^ w as u64).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
    h
}

impl KeyMap {
    pub(super) fn new() -> Self {
        Self {
            slots: vec![0; 64],
            len: 0,
        }
    }

    /// `Ok(idx)` when `key` is present, `Err(slot)` with the empty slot
    /// it would occupy otherwise. Grows first so the slot stays valid
    /// for the `insert_at` that follows without any intervening insert.
    pub(super) fn find(&mut self, keys: &[Vec<u32>], key: &[u32]) -> Result<u32, usize> {
        if self.len * 2 >= self.slots.len() {
            self.grow(keys);
        }
        let mask = self.slots.len() - 1;
        let mut slot = hash_key(key) as usize & mask;
        loop {
            let v = self.slots[slot];
            if v == 0 {
                return Err(slot);
            }
            if keys[(v - 1) as usize].as_slice() == key {
                return Ok(v - 1);
            }
            slot = (slot + 1) & mask;
        }
    }

    pub(super) fn insert_at(&mut self, slot: usize, idx: u32) {
        self.slots[slot] = idx + 1;
        self.len += 1;
    }

    fn grow(&mut self, keys: &[Vec<u32>]) {
        let grown = vec![0; self.slots.len() * 2];
        let old = core::mem::replace(&mut self.slots, grown);
        let mask = self.slots.len() - 1;
        for v in old {
            if v == 0 {
                continue;
            }
            let mut slot = hash_key(&keys[(v - 1) as usize]) as usize & mask;
            while self.slots[slot] != 0 {
                slot = (slot + 1) & mask;
            }
            self.slots[slot] = v;
        }
    }
}
