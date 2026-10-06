//! Routing the unit a state read to the edge that takes it. Edges that
//! write the same slots on the way to the same state share one body,
//! so a state's edges fall into a few classes, each a set of ranges.
//!
//! Over bytes, a class of more than two ranges (`\w`, `[A-Za-z0-9]`, a
//! property's Latin-1 projection) is one load from a 256-byte map in
//! read-only data, nonzero at the class's bytes. A 256-bit map is an
//! eighth of the size but costs four more instructions a byte (shift,
//! mask, shift, mask), and the map is shared by every walk and probe
//! of the module that tests the same class.
//! Fewer ranges, and every class over 16-bit units, test range by
//! range. A state that takes no map (over 16-bit units, no class past
//! two ranges, or more than [`CLASS_MAX`] classes) tests its edges one
//! after the other, or past [`CHAIN_MAX`] edges searches them as a
//! tree.

use crate::ssa::{BlockId, IPred, InstKind, Operand, StringLiteral, Terminator, Type};
use crate::ssa::{StringId, ValueId};
use torajs_regex::onepass::{Edge, Variant};

use super::Emit;
use super::dispatch::CHAIN_MAX;

/// Most classes a state over bytes tests one after the other.
const CLASS_MAX: usize = 4;

/// The maps a walk reads: interned in the module's string table, and
/// addressed once in the entry block so the loops keep the address in
/// a register.
pub(super) struct Maps<'a> {
    strings: &'a mut Vec<StringLiteral>,
    entry: BlockId,
    addressed: Vec<(StringId, ValueId)>,
}

impl<'a> Maps<'a> {
    pub(super) fn new(strings: &'a mut Vec<StringLiteral>, entry: BlockId) -> Self {
        Self {
            strings,
            entry,
            addressed: Vec::new(),
        }
    }
}

/// One class: its ranges and the index of its first edge.
struct Class {
    ranges: Vec<(u32, u32)>,
    first: usize,
}

impl Emit<'_> {
    /// From `cur`, where `u` is the unit at `pos` (spanning `step`
    /// units), take the edge of variant `v` of state `i` that holds
    /// `u`, or branch to `fail`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn dispatch_edges(
        &mut self,
        cur: BlockId,
        u: ValueId,
        pos: ValueId,
        step: Operand,
        v: &Variant,
        i: usize,
        fail: BlockId,
    ) {
        let classes = classes(&v.edges);
        let mapped = !self.ir.wide
            && classes.len() <= CLASS_MAX
            && classes.iter().any(|c| c.ranges.len() > 2);
        if mapped {
            let mut cur = cur;
            for (k, class) in classes.iter().enumerate() {
                let taken = self.f.add_block();
                self.take_edge(taken, pos, &v.edges[class.first], step, i);
                let next = if k + 1 == classes.len() {
                    fail
                } else {
                    self.f.add_block()
                };
                self.class_test(cur, u, &class.ranges, taken, next);
                cur = next;
            }
            return;
        }
        if v.edges.len() > CHAIN_MAX {
            let mut taken: Vec<BlockId> = Vec::with_capacity(v.edges.len());
            for (k, edge) in v.edges.iter().enumerate() {
                let blk = match v.edges[..k].iter().position(|e| same_body(e, edge)) {
                    Some(first) => taken[first],
                    None => {
                        let blk = self.f.add_block();
                        self.take_edge(blk, pos, edge, step, i);
                        blk
                    }
                };
                taken.push(blk);
            }
            self.search(cur, u, &v.edges, &taken, fail);
            return;
        }
        let mut cur = cur;
        for (j, edge) in v.edges.iter().enumerate() {
            let taken = self.f.add_block();
            let next = if j + 1 == v.edges.len() {
                fail
            } else {
                self.f.add_block()
            };
            if edge.lo == 0 && edge.hi == self.ir.unit_max() {
                self.f.set_term(cur, Terminator::Br(taken));
            } else {
                let cond = self.in_range(cur, u, edge.lo, edge.hi);
                self.cond_br(cur, cond, taken, next);
            }
            self.take_edge(taken, pos, edge, step, i);
            cur = next;
        }
    }

    /// From `blk`, branch to `yes` when `u` lies in one of `ranges`,
    /// else to `no`: one map load over bytes when there are more than
    /// two ranges, else a compare per range.
    pub(super) fn class_test(
        &mut self,
        blk: BlockId,
        u: ValueId,
        ranges: &[(u32, u32)],
        yes: BlockId,
        no: BlockId,
    ) {
        if !self.ir.wide && ranges.len() > 2 {
            let base = self.byte_map(ranges);
            let byte = self.f.append_inst(
                blk,
                InstKind::LoadU8Dyn(Operand::Value(base), Operand::Value(u)),
                Type::I64,
                None,
            );
            let c = self.icmp(blk, IPred::Ne, byte, Operand::ConstI64(0));
            self.cond_br(blk, c, yes, no);
            return;
        }
        let mut cur = blk;
        for (j, &(lo, hi)) in ranges.iter().enumerate() {
            let next = if j + 1 == ranges.len() {
                no
            } else {
                self.f.add_block()
            };
            if lo == 0 && hi == self.ir.unit_max() {
                self.f.set_term(cur, Terminator::Br(yes));
                return;
            }
            let c = self.in_range(cur, u, lo, hi);
            self.cond_br(cur, c, yes, next);
            cur = next;
        }
    }

    /// The address of the byte map of `ranges`.
    fn byte_map(&mut self, ranges: &[(u32, u32)]) -> ValueId {
        let mut bits = vec![0u8; 256];
        for &(lo, hi) in ranges {
            for b in lo..=hi.min(0xFF) {
                bits[b as usize] = 1;
            }
        }
        self.rodata(bits)
    }

    /// `words` loaded once in the entry block from read-only data, so a
    /// loop keeps them in registers instead of building each 64-bit
    /// constant at every use.
    pub(super) fn entry_words(&mut self, words: &[u64]) -> Vec<ValueId> {
        let base = self.rodata(words.iter().flat_map(|w| w.to_le_bytes()).collect());
        let entry = self.maps.entry;
        (0..words.len())
            .map(|k| {
                let at = InstKind::Load(Type::I64, Operand::Value(base), 8 * k as u64);
                self.f.append_inst(entry, at, Type::I64, None)
            })
            .collect()
    }

    /// The address of `bits` in read-only data, shared with every other
    /// use of the same bytes in the module.
    fn rodata(&mut self, bits: Vec<u8>) -> ValueId {
        let maps = &mut self.maps;
        let found = maps
            .strings
            .iter()
            .position(|s| s.is_latin1 && s.bytes == bits);
        let id = StringId(found.unwrap_or_else(|| {
            maps.strings.push(StringLiteral::from_latin1_bytes(bits));
            maps.strings.len() - 1
        }) as u32);
        if let Some(&(_, v)) = maps.addressed.iter().find(|&&(s, _)| s == id) {
            return v;
        }
        let v = self
            .f
            .append_inst(maps.entry, InstKind::StringRef(id), Type::Ptr, None);
        maps.addressed.push((id, v));
        v
    }
}

fn same_body(a: &Edge, b: &Edge) -> bool {
    a.target == b.target && a.saves == b.saves && a.clears == b.clears
}

/// `edges` grouped by body, in the order each body first appears.
fn classes(edges: &[Edge]) -> Vec<Class> {
    let mut out: Vec<Class> = Vec::new();
    for (k, e) in edges.iter().enumerate() {
        match out.iter_mut().find(|c| same_body(&edges[c.first], e)) {
            Some(c) => c.ranges.push((e.lo, e.hi)),
            None => out.push(Class {
                ranges: vec![(e.lo, e.hi)],
                first: k,
            }),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::walk;
    use crate::ssa_lower_regex_bake::matcher::matcher_ir;

    fn maps(pattern: &str, flags: &str, wide: bool) -> Vec<Vec<u8>> {
        let ir = matcher_ir(pattern, flags, wide).unwrap();
        let mut strings = Vec::new();
        walk("m", &ir, &mut strings);
        strings.into_iter().map(|s| s.bytes).collect()
    }

    fn has(map: &[u8], b: u8) -> bool {
        map[b as usize] != 0
    }

    #[test]
    fn a_byte_class_past_two_ranges_reads_one_shared_map() {
        // the loop's class and the start state's class are one map
        let m = maps(r"\w+", "", false);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].len(), 256);
        for b in 0..=255u8 {
            assert_eq!(has(&m[0], b), b.is_ascii_alphanumeric() || b == b'_', "{b}");
        }
        // a word boundary probes the same bits
        assert_eq!(maps(r"\bword\b", "", false), m);
    }

    #[test]
    fn two_ranges_and_wide_walks_keep_their_compares() {
        assert!(maps(r"[A-Za-z]+", "", false).is_empty());
        assert!(maps(r"\w+", "", true).is_empty());
        assert!(maps(r"\bword\b", "", true).is_empty());
    }

    #[test]
    fn a_latin1_property_projection_is_mapped() {
        let m = maps(r"\p{L}+", "u", false);
        assert_eq!(m.len(), 1);
        for b in [b'a', b'Z', 0xAA, 0xB5, 0xC0, 0xFF] {
            assert!(has(&m[0], b), "{b:#x}");
        }
        for b in [b'0', b' ', 0xD7, 0xF7] {
            assert!(!has(&m[0], b), "{b:#x}");
        }
    }
}
