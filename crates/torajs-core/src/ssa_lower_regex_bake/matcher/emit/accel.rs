//! A state that loops on itself over every unit but a few and writes
//! nothing on the way (the body of `.+` / `[^x]*`) skips those units a
//! word at a time before its per-unit dispatch. The word is xored with
//! each unit that leaves the loop, and `(x - 0x01..) & !x & 0x80..`
//! marks the lanes that came out zero; its lowest set bit is exact (a
//! false mark only sits above a true one), so the walk resumes unit by
//! unit at the first unit that leaves. Under `pairs` the leaving units
//! are never surrogates, so a skip never stops inside a pair.

use crate::ssa::{BinOp, BlockId, IPred, InstKind, Operand, Terminator, Type};

use super::Emit;

/// Most units leaving a skipped loop: each costs four instructions a
/// word.
const ESCAPE_MAX: usize = 3;

impl Emit<'_> {
    /// From `blk`, the head of state `i`: skip the units the state
    /// loops on, a word at a time while a whole word is left. Answers
    /// the block the state's per-unit body begins in (`blk` itself
    /// when the state is not such a loop).
    pub(super) fn accelerate(&mut self, blk: BlockId, i: usize) -> BlockId {
        let Some(escapes) = self.escapes(i) else {
            return blk;
        };
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

        let pos = self.load(blk, self.pos_slot, 0);
        let end = self.binop(blk, BinOp::Add, pos, Operand::ConstI64(per_word));
        let fits = self.icmp(blk, IPred::Sle, end, Operand::Value(self.len));
        self.cond_br(blk, fits, read, body);

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
        let mut words: Vec<u64> = escapes.iter().map(|&e| u64::from(e) * ones).collect();
        words.push(ones);
        let words = self.entry_words(&words);
        let ones = *words.last().expect("the lane ones were pushed last");
        // per leaving unit: x = word ^ splat(e); (x - ones) & !x & highs,
        // the `!x & highs` spelled `(x & highs) ^ highs` so both take
        // logical immediates
        let mut found = None;
        for &splat in &words[..escapes.len()] {
            let x = self.binop(read, BinOp::Xor, word, Operand::Value(splat));
            let dec = self.binop(read, BinOp::Sub, x, Operand::Value(ones));
            let high = self.binop(read, BinOp::And, x, Operand::ConstI64(highs));
            let zero = self.binop(read, BinOp::Xor, high, Operand::ConstI64(highs));
            let z = self.binop(read, BinOp::And, dec, Operand::Value(zero));
            found = Some(match found {
                None => z,
                Some(m) => self.binop(read, BinOp::Or, m, Operand::Value(z)),
            });
        }
        let found = found.expect("a skipped loop has a unit that leaves it");
        let any = self.icmp(read, IPred::Ne, found, Operand::ConstI64(0));
        self.cond_br(read, any, hit, skip);

        self.store(skip, Operand::Value(end), self.pos_slot, 0);
        self.f.set_term(skip, Terminator::Br(blk));

        // the lane of the lowest mark: ctz = popcount(below the lowest bit)
        let neg = self
            .f
            .append_inst(hit, InstKind::Neg(Operand::Value(found)), Type::I64, None);
        let low = self.binop(hit, BinOp::And, found, Operand::Value(neg));
        let below = self.binop(hit, BinOp::Sub, low, Operand::ConstI64(1));
        let bits = self
            .f
            .append_inst(hit, InstKind::Ctpop(Operand::Value(below)), Type::I64, None);
        let shift = lane.trailing_zeros() as i64;
        let lanes = self.binop(hit, BinOp::LShr, bits, Operand::ConstI64(shift));
        let at = self.binop(hit, BinOp::Add, pos, Operand::Value(lanes));
        self.store(hit, Operand::Value(at), self.pos_slot, 0);
        self.f.set_term(hit, Terminator::Br(body));
        body
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
        let in_lane = |u: u32| u <= 0xFFFF && !(0xD800..=0xDFFF).contains(&u);
        (!out.is_empty() && out.iter().all(|&u| in_lane(u))).then_some(out)
    }
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
        for wide in [false, true] {
            // `.` under s loops on all but `c`; without s, `\n` and `\r`
            // leave too (over 16-bit units also U+2028 / U+2029: five)
            assert_eq!(word_loads(r"a.+c", "s", wide), 1);
            assert_eq!(word_loads(r"a[^c]*c", "", wide), 1);
            assert_eq!(word_loads(r"a.+c", "su", wide), 1);
        }
        assert_eq!(word_loads(r"a.+c", "", false), 1);
        assert_eq!(word_loads(r"a.+c", "", true), 0);
        // a loop on a class, an accepting loop
        assert_eq!(word_loads(r"\w+", "", false), 0);
        assert_eq!(word_loads(r"a.+", "s", false), 0);
    }
}
