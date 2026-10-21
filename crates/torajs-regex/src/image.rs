//! The build-time image of a [`Compiled`] literal: the compiler encodes
//! it next to the binary's string literals, and the literal's RegExp
//! construction decodes it instead of parsing the pattern.
//!
//! The image is a sequence of little-endian 32-bit words (byte strings
//! padded to a word), so every Unicode property table it carries sits
//! 4-aligned inside it; the link places the image 8-aligned. Decoding
//! hands those tables out in place as `&'static [UPropRange]`, which
//! keeps every class exactly as the parser built it (a table
//! reference is not the same class as the same ranges owned: the DFA
//! gate tells them apart).
//!
//! ```text
//! u32 byte length of the whole image
//! u32 flags | rejected << 8
//! u32 n_captures, u32 n_named_captures
//! u32 count, then per capture name: bytes
//! u32 count, then per pooled property table: u32 count + (lo, hi) pairs
//! program
//! ```
//!
//! A program is its bit facts and prefix byte, the whole-literal bytes
//! when present, its instructions, backreference captures, classes
//! (bitmap, negate / byte-only bits, pool indices, owned ranges) and
//! then its lookaround sub-programs, each in the same form.

use alloc::{boxed::Box, vec::Vec};

use crate::charclass::CharClass;
use crate::compiled::Compiled;
use crate::program::{Inst, Program};
use crate::ucd::UPropRange;

const CAN_DFA: u32 = 1;
const HAS_SAVE: u32 = 2;
const HAS_ML_ANCHOR_B: u32 = 4;
const RAW_CLASSES: u32 = 8;
const HAS_PREFIX: u32 = 16;
const HAS_LITERAL: u32 = 32;

const NEGATE: u32 = 1;
const BYTE_ONLY: u32 = 2;

/// Encode `c` into its image.
pub fn encode(c: &Compiled) -> Vec<u8> {
    let mut pool: Vec<&'static [UPropRange]> = Vec::new();
    collect_tables(&c.prog, &mut pool);
    let mut w = Writer(Vec::new());
    w.word(0);
    w.word(u32::from(c.flags) | u32::from(c.rejected) << 8);
    w.word(c.n_captures as u32);
    w.word(c.n_named_captures as u32);
    w.word(c.capture_names.len() as u32);
    for name in &c.capture_names {
        w.bytes(name);
    }
    w.word(pool.len() as u32);
    for t in &pool {
        w.ranges(t);
    }
    w.program(&c.prog, &pool);
    let len = w.0.len() as u32;
    w.0[..4].copy_from_slice(&len.to_le_bytes());
    w.0
}

/// Decode the image at `image`.
///
/// # Safety
///
/// `image` is 4-aligned, points at bytes [`encode`] produced, and
/// stays valid and unchanged for the rest of the process (the
/// decoded classes borrow their property tables from it).
pub unsafe fn decode(image: *const u8) -> Compiled {
    let len = u32::from_le_bytes(unsafe { *(image as *const [u8; 4]) }) as usize;
    let bytes: &'static [u8] = unsafe { core::slice::from_raw_parts(image, len) };
    let mut r = Reader { bytes, pos: 4 };
    let head = r.word();
    let n_captures = r.word() as i32;
    let n_named_captures = r.word() as i32;
    let capture_names = (0..r.word()).map(|_| r.bytes().to_vec()).collect();
    let pool: Vec<&'static [UPropRange]> = (0..r.word()).map(|_| r.ranges()).collect();
    let prog = r.program(&pool);
    Compiled {
        flags: head as u8,
        rejected: (head >> 8) as u8,
        n_captures,
        capture_names,
        n_named_captures,
        prog,
    }
}

fn collect_tables(prog: &Program, pool: &mut Vec<&'static [UPropRange]>) {
    for cc in &prog.classes {
        for &t in &cc.u_prop_tables {
            if pool_index(pool, t).is_none() {
                pool.push(t);
            }
        }
    }
    for sub in &prog.sub_progs {
        collect_tables(sub, pool);
    }
}

fn pool_index(pool: &[&'static [UPropRange]], t: &[UPropRange]) -> Option<usize> {
    pool.iter()
        .position(|p| core::ptr::eq(p.as_ptr(), t.as_ptr()) && p.len() == t.len())
}

struct Writer(Vec<u8>);

impl Writer {
    fn word(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    fn bytes(&mut self, b: &[u8]) {
        self.word(b.len() as u32);
        self.0.extend_from_slice(b);
        while !self.0.len().is_multiple_of(4) {
            self.0.push(0);
        }
    }

    fn ranges(&mut self, rs: &[UPropRange]) {
        self.word(rs.len() as u32);
        for r in rs {
            self.word(r.lo as u32);
            self.word(r.hi as u32);
        }
    }

    fn program(&mut self, p: &Program, pool: &[&'static [UPropRange]]) {
        let bits = [
            (p.can_dfa, CAN_DFA),
            (p.has_save, HAS_SAVE),
            (p.has_ml_anchor_b, HAS_ML_ANCHOR_B),
            (p.raw_classes, RAW_CLASSES),
            (p.prefix_byte.is_some(), HAS_PREFIX),
            (p.literal.is_some(), HAS_LITERAL),
        ]
        .iter()
        .filter(|(on, _)| *on)
        .fold(0, |acc, (_, bit)| acc | bit);
        self.word(bits);
        self.word(u32::from(p.prefix_byte.unwrap_or(0)));
        if let Some(lit) = &p.literal {
            self.bytes(lit);
        }
        self.word(p.insts.len() as u32);
        for i in &p.insts {
            self.word(u32::from(i.op) | u32::from(i.pad) << 16);
            self.word(i.a as u32);
            self.word(i.b as u32);
        }
        self.word(p.backref_caps.len() as u32);
        for &c in &p.backref_caps {
            self.word(c as u32);
        }
        self.word(p.classes.len() as u32);
        for cc in &p.classes {
            self.0.extend_from_slice(&cc.bits);
            let mut b = 0;
            if cc.negate {
                b |= NEGATE;
            }
            if cc.byte_only {
                b |= BYTE_ONLY;
            }
            self.word(b);
            self.word(cc.u_prop_tables.len() as u32);
            for t in &cc.u_prop_tables {
                let idx = pool_index(pool, t).expect("table pooled by collect_tables");
                self.word(idx as u32);
            }
            self.ranges(&cc.owned_ranges);
        }
        self.word(p.sub_progs.len() as u32);
        for sub in &p.sub_progs {
            self.program(sub, pool);
        }
    }
}

struct Reader {
    bytes: &'static [u8],
    pos: usize,
}

impl Reader {
    fn word(&mut self) -> u32 {
        let w = &self.bytes[self.pos..self.pos + 4];
        self.pos += 4;
        u32::from_le_bytes([w[0], w[1], w[2], w[3]])
    }

    fn bytes(&mut self) -> &'static [u8] {
        let n = self.word() as usize;
        let b = &self.bytes[self.pos..self.pos + n];
        self.pos += n.next_multiple_of(4);
        b
    }

    fn ranges(&mut self) -> &'static [UPropRange] {
        let n = self.word() as usize;
        let at = &self.bytes[self.pos..self.pos + n * 8];
        self.pos += n * 8;
        // SAFETY: `UPropRange` is two `#[repr(C)]` i32s; the range
        // words sit 4-aligned in an image that lives for the process
        // (the `decode` contract), and every bit pattern is a valid i32
        unsafe { core::slice::from_raw_parts(at.as_ptr() as *const UPropRange, n) }
    }

    fn program(&mut self, pool: &[&'static [UPropRange]]) -> Program {
        let mut p = Program::new();
        let bits = self.word();
        let prefix = self.word() as u8;
        p.can_dfa = bits & CAN_DFA != 0;
        p.has_save = bits & HAS_SAVE != 0;
        p.has_ml_anchor_b = bits & HAS_ML_ANCHOR_B != 0;
        p.raw_classes = bits & RAW_CLASSES != 0;
        p.prefix_byte = (bits & HAS_PREFIX != 0).then_some(prefix);
        if bits & HAS_LITERAL != 0 {
            p.literal = Some(self.bytes().to_vec());
        }
        p.insts = (0..self.word())
            .map(|_| {
                let head = self.word();
                Inst {
                    op: head as u8,
                    pad: (head >> 16) as u16,
                    a: self.word() as i32,
                    b: self.word() as i32,
                }
            })
            .collect();
        p.backref_caps = (0..self.word()).map(|_| self.word() as i32).collect();
        p.classes = (0..self.word()).map(|_| self.class(pool)).collect();
        p.sub_progs = (0..self.word())
            .map(|_| Box::new(self.program(pool)))
            .collect();
        p
    }

    fn class(&mut self, pool: &[&'static [UPropRange]]) -> CharClass {
        let mut cc = CharClass::new();
        cc.bits
            .copy_from_slice(&self.bytes[self.pos..self.pos + 32]);
        self.pos += 32;
        let b = self.word();
        cc.negate = b & NEGATE != 0;
        cc.byte_only = b & BYTE_ONLY != 0;
        cc.u_prop_tables = (0..self.word())
            .map(|_| pool[self.word() as usize])
            .collect();
        cc.owned_ranges = self.ranges().to_vec();
        cc
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiled::compile_pattern;
    use alloc::format;

    /// Decode an image the way the binary holds it: 8-aligned and
    /// never freed.
    fn round_trip(pat: &str, flags: &str) -> (Compiled, Compiled) {
        let c = compile_pattern(pat.as_bytes(), flags.as_bytes());
        let img = encode(&c);
        assert!(img.len().is_multiple_of(4));
        let mut words = alloc::vec![0u64; img.len().div_ceil(8)];
        let buf =
            unsafe { core::slice::from_raw_parts_mut(words.as_mut_ptr() as *mut u8, img.len()) };
        buf.copy_from_slice(&img);
        let leaked: &'static [u64] = Box::leak(core::mem::take(&mut words).into_boxed_slice());
        let d = unsafe { decode(leaked.as_ptr() as *const u8) };
        (c, d)
    }

    fn assert_same(pat: &str, flags: &str) {
        let (c, d) = round_trip(pat, flags);
        assert_eq!(format!("{c:?}"), format!("{d:?}"), "/{pat}/{flags}");
    }

    #[test]
    fn every_program_shape_survives_the_image() {
        for (pat, flags) in [
            ("hello", ""),
            (r"(?<year>\d{4})-(?<m>\d\d)", "d"),
            (r"\k<x>(?<x>a)|(b)\2", ""),
            (r"(?<=a)b(?!c)(?=(d))", "gi"),
            (r"^\p{L}+$", "um"),
            (r"\P{Script=Greek}[\p{N}--[0-9]]", "v"),
            (r".+\b", "sy"),
            ("[", ""),
            ("a", "uv"),
            ("a", "gg"),
        ] {
            assert_same(pat, flags);
        }
    }

    #[test]
    fn a_property_table_is_carried_once_and_read_in_place() {
        let (c, d) = round_trip(r"\p{L}x\p{L}", "u");
        let tables = |p: &Program| -> Vec<*const UPropRange> {
            p.classes
                .iter()
                .flat_map(|cc| cc.u_prop_tables.iter().map(|t| t.as_ptr()))
                .collect()
        };
        let (orig, back) = (tables(&c.prog), tables(&d.prog));
        assert_eq!(orig.len(), back.len());
        assert!(!back.is_empty());
        assert!(back.windows(2).all(|w| w[0] == w[1]));
        assert!(orig.iter().zip(&back).all(|(a, b)| a != b));
    }

    #[test]
    fn a_rejected_pattern_keeps_the_stub() {
        let (_, d) = round_trip("(", "");
        assert_eq!(d.rejected, 1);
        assert_eq!(d.prog.insts.len(), 2);
    }
}
