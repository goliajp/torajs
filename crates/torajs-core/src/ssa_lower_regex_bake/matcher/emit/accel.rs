//! A state that loops on itself over every unit but a few and writes
//! nothing on the way (the body of `.+` / `[^x]*`) skips those units a
//! word at a time before its per-unit dispatch. The word is xored with
//! each unit that leaves the loop, and `(x - 0x01..) & !x & 0x80..`
//! marks the lanes that came out zero; its lowest set bit is exact (a
//! false mark only sits above a true one), so the walk resumes unit by
//! unit at the first unit that leaves. Under `pairs` the leaving units
//! are never surrogates, so a skip never stops inside a pair.
//!
//! An unanchored scan skips the same way over every start whose unit
//! state 0 has no edge for (`/hello/i` begins only on `h` or `H`).

use crate::ssa::{BinOp, BlockId, IPred, InstKind, Operand, Terminator, Type, ValueId};

use super::Emit;

/// Most units leaving a skipped loop: each costs four instructions a
/// word.
const ESCAPE_MAX: usize = 3;

impl Emit<'_> {
    /// From `blk`, the head of state `i`: skip the units the state
    /// loops on. Answers the block the state's per-unit body begins in
    /// (`blk` itself when the state is not such a loop).
    pub(super) fn accelerate(&mut self, blk: BlockId, i: usize) -> BlockId {
        match self.escapes(i) {
            Some(escapes) => self.skip(blk, self.pos_slot, &escapes, true),
            None => blk,
        }
    }

    /// The block a failed attempt moves the start to before `scan`
    /// tries it, skipping every unit but `units` (`start_units`): an
    /// attempt there would die at once. The first attempt goes
    /// unskipped, so a match at the start of the subject pays nothing,
    /// and one pass skips one word: the scan loop is the word loop, so
    /// no loop nests in it to pull the block layout's chains away from
    /// the attempt.
    pub(super) fn skip_scan(&mut self, scan: BlockId, units: &[u32]) -> BlockId {
        let rescan = self.f.add_block();
        let body = self.skip(rescan, self.st_slot, units, false);
        self.f.set_term(body, Terminator::Br(scan));
        rescan
    }

    /// From `blk`, advance the position in `slot` to the first of
    /// `escapes`, a word at a time while a whole word is left; answers
    /// the block that goes on from there unit by unit. A `state` loop
    /// reads word after word and takes its constants from the entry
    /// block (it is entered again on every pass through its state);
    /// otherwise one word is read.
    fn skip(&mut self, blk: BlockId, slot: ValueId, escapes: &[u32], state: bool) -> BlockId {
        let lane: i64 = if self.ir.wide { 16 } else { 8 };
        let per_word = 64 / lane;
        let ones: u64 = if self.ir.wide {
            0x0001_0001_0001_0001
        } else {
            0x0101_0101_0101_0101
        };
        let highs = (ones << (lane - 1)) as i64;
        let body = self.f.add_block();
        let read = self.f.add_block();
        let skip = self.f.add_block();
        let hit = self.f.add_block();

        let pos = self.load(blk, slot, 0);
        let end = self.binop(blk, BinOp::Add, pos, Operand::ConstI64(per_word));
        let fits = self.icmp(blk, IPred::Sle, end, Operand::Value(self.len));
        self.cond_br(blk, fits, read, body);

        // A state loop loads the splatted units, the lane ones and
        // `index` (below) once in the entry block, so the loop keeps them
        // in registers instead of building each 64-bit constant at every
        // use. The scan's one word builds them in place: registers held
        // across the scan would cost every call a callee-saved spill.
        let splats: Vec<u64> = escapes.iter().map(|&e| u64::from(e) * ones).collect();
        let (splats, ones, index) = if state {
            // lane k of `index` holds its own index counted from the
            // top, so `index << (lane * k)` carries k in the top lane
            let index = if self.ir.wide {
                0x0000_0001_0002_0003
            } else {
                0x0001_0203_0405_0607
            };
            let words = self.entry_words(&[&splats[..], &[ones, index]].concat());
            let k = splats.len();
            let ops = words.iter().map(|&w| Operand::Value(w)).collect::<Vec<_>>();
            (ops[..k].to_vec(), ops[k], Some(words[k + 1]))
        } else {
            let ops = splats
                .iter()
                .map(|&w| Operand::ConstI64(w as i64))
                .collect();
            (ops, Operand::ConstI64(ones as i64), None)
        };

        let pos = self.load(read, slot, 0);
        let off = if self.ir.wide {
            self.binop(read, BinOp::Shl, pos, Operand::ConstI64(1))
        } else {
            pos
        };
        let word = self.f.append_inst(
            read,
            InstKind::LoadDyn(Type::I64, Operand::Value(self.units), Operand::Value(off)),
            Type::I64,
            None,
        );
        // per leaving unit: x = word ^ splat(e); (x - ones) & !x & highs,
        // the `!x & highs` spelled `(x & highs) ^ highs` so both take
        // logical immediates
        let mut found = None;
        for &splat in &splats {
            let x = self.binop(read, BinOp::Xor, word, splat);
            let high = self.binop(read, BinOp::And, x, Operand::ConstI64(highs));
            let zero = self.binop(read, BinOp::Xor, high, Operand::ConstI64(highs));
            let dec = self.binop(read, BinOp::Sub, x, ones);
            let z = self.binop(read, BinOp::And, dec, Operand::Value(zero));
            found = Some(match found {
                None => z,
                Some(m) => self.binop(read, BinOp::Or, m, Operand::Value(z)),
            });
        }
        let found = found.expect("a skipped loop has a unit that leaves it");
        let any = self.icmp(read, IPred::Ne, found, Operand::ConstI64(0));
        self.cond_br(read, any, hit, skip);

        // `end` is recomputed here, not carried out of `read`, to keep
        // one register free across the word test
        let end = self.binop(skip, BinOp::Add, pos, Operand::ConstI64(per_word));
        self.store(skip, Operand::Value(end), slot, 0);
        if state {
            let next = self.binop(skip, BinOp::Add, end, Operand::ConstI64(per_word));
            let fits = self.icmp(skip, IPred::Sle, next, Operand::Value(self.len));
            self.cond_br(skip, fits, read, body);
        } else {
            self.f.set_term(skip, Terminator::Br(body));
        }

        // the lane k of the lowest mark: isolate it, then either move it
        // to bit `lane * k` and read k off the top lane of `index` times
        // that, or (one register fewer, for the scan) count the bits
        // below it
        let neg = self
            .f
            .append_inst(hit, InstKind::Neg(Operand::Value(found)), Type::I64, None);
        let low = self.binop(hit, BinOp::And, found, Operand::Value(neg));
        let lanes = if let Some(index) = index {
            let bit = self.binop(hit, BinOp::LShr, low, Operand::ConstI64(lane - 1));
            let spread = self.binop(hit, BinOp::Mul, bit, Operand::Value(index));
            self.binop(hit, BinOp::LShr, spread, Operand::ConstI64(64 - lane))
        } else {
            let below = self.binop(hit, BinOp::Sub, low, Operand::ConstI64(1));
            let bits =
                self.f
                    .append_inst(hit, InstKind::Ctpop(Operand::Value(below)), Type::I64, None);
            let shift = lane.trailing_zeros() as i64;
            self.binop(hit, BinOp::LShr, bits, Operand::ConstI64(shift))
        };
        let at = self.binop(hit, BinOp::Add, pos, Operand::Value(lanes));
        self.store(hit, Operand::Value(at), slot, 0);
        self.f.set_term(hit, Terminator::Br(body));
        body
    }

    /// `words` loaded in the entry block from read-only data.
    fn entry_words(&mut self, words: &[u64]) -> Vec<ValueId> {
        let base = self.rodata(words.iter().flat_map(|w| w.to_le_bytes()).collect());
        let entry = self.maps.entry;
        (0..words.len())
            .map(|k| {
                let at = InstKind::Load(Type::I64, Operand::Value(base), 8 * k as u64);
                self.f.append_inst(entry, at, Type::I64, None)
            })
            .collect()
    }

    /// The units that leave state `i`'s loop, when the state has one
    /// variant, no accept, and loops on every other unit without a
    /// save or a clear.
    fn escapes(&self, i: usize) -> Option<Vec<u32>> {
        let node = &self.ir.states[i];
        if node.probes != 0 || node.variants.len() != 1 {
            return None;
        }
        let v = &node.variants[0];
        if v.accept.is_some() {
            return None;
        }
        let top = self.ir.unit_max() + 1;
        let loops = v
            .edges
            .iter()
            .filter(|e| e.target as usize == i && e.saves.is_empty() && e.clears.is_empty())
            .map(|e| (e.lo, e.hi));
        let mut out = Vec::new();
        let mut next = 0;
        for (lo, hi) in loops.chain([(top, top)]) {
            if (lo - next) as usize > ESCAPE_MAX - out.len() {
                return None;
            }
            out.extend(next..lo);
            next = hi + 1;
        }
        in_lane(out)
    }

    /// The units state 0 has an edge for, when an unanchored attempt
    /// can only begin on one of a few: state 0 has one variant and no
    /// accept.
    pub(super) fn start_units(&self) -> Option<Vec<u32>> {
        let node = &self.ir.states[0];
        if self.ir.anchored || node.probes != 0 || node.variants.len() != 1 {
            return None;
        }
        let v = &node.variants[0];
        if v.accept.is_some() {
            return None;
        }
        let mut out = Vec::new();
        for e in &v.edges {
            if (e.hi - e.lo) as usize >= ESCAPE_MAX - out.len() {
                return None;
            }
            out.extend(e.lo..=e.hi);
        }
        in_lane(out)
    }
}

/// `units` when there are some and each fits a 16-bit lane outside
/// the surrogates
fn in_lane(units: Vec<u32>) -> Option<Vec<u32>> {
    let fits = |u: u32| u <= 0xFFFF && !(0xD800..=0xDFFF).contains(&u);
    (!units.is_empty() && units.iter().all(|&u| fits(u))).then_some(units)
}

#[cfg(test)]
mod tests {
    use super::super::walk;
    use crate::ssa::InstKind;
    use crate::ssa_lower_regex_bake::matcher::matcher_ir;

    fn word_loads(pattern: &str, flags: &str, wide: bool) -> usize {
        let ir = matcher_ir(pattern, flags, wide).unwrap();
        let f = walk("m", &ir, &mut Vec::new());
        f.blocks
            .iter()
            .flat_map(|b| b.insts.iter())
            .filter(|i| matches!(i.kind, InstKind::LoadDyn(..)))
            .count()
    }

    #[test]
    fn a_loop_left_by_few_units_skips_a_word_at_a_time() {
        // each pattern begins on one unit (one skip for the scan); `.`
        // under s loops on all but `c`, without s `\n` and `\r` leave
        // too (over 16-bit units also U+2028 / U+2029: five)
        for wide in [false, true] {
            assert_eq!(word_loads(r"a.+c", "s", wide), 2);
            assert_eq!(word_loads(r"a[^c]*c", "", wide), 2);
            assert_eq!(word_loads(r"a.+c", "su", wide), 2);
        }
        assert_eq!(word_loads(r"a.+c", "", false), 2);
        assert_eq!(word_loads(r"a.+c", "", true), 1);
        // a loop on a class, an accepting loop
        assert_eq!(word_loads(r"\w+", "", false), 0);
        assert_eq!(word_loads(r"a.+", "s", false), 1);
    }

    #[test]
    fn a_scan_skips_to_the_few_units_an_attempt_can_begin_on() {
        for wide in [false, true] {
            assert_eq!(word_loads(r"hello", "i", wide), 1);
            assert_eq!(word_loads(r"[abc]x", "", wide), 1);
            // four units, a line-start probe, an accept before any unit,
            // a sticky attempt
            assert_eq!(word_loads(r"[a-d]x", "", wide), 0);
            assert_eq!(word_loads(r"^line", "m", wide), 0);
            assert_eq!(word_loads(r"x*", "", wide), 0);
            assert_eq!(word_loads(r"ab", "y", wide), 0);
        }
    }
}
