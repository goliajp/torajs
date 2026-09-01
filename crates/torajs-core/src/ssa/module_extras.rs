//! `impl StringLiteral` (build-time Latin-1 / UTF-16 encoding picker)
//! and the `demo_fib40` hand-built fixture. Extracted from
//! `module_methods.rs` to keep that file under the 500-prod-LOC
//! file-size hard limit (`rules/common/file-size.md`). Pure
//! mechanical pull, no semantic change.

use super::module_methods::{Module, StringLiteral};
use super::{BinOp, FuncId, Function, IPred, InstKind, Operand, Terminator, Type};

impl StringLiteral {
    /// Latin-1 wrapper for an already-encoded byte buffer where
    /// every byte is ≤ 0xFF (trivially true for `&[u8]`). Used by
    /// the obj-field-access fname matching path where the fname
    /// originates as an ASCII identifier `String` and the
    /// `StringRef` raw-byte consumer (`__torajs_str_eq_cstr`)
    /// needs the bytes verbatim.
    pub fn from_latin1_bytes(bytes: Vec<u8>) -> Self {
        let length = bytes.len() as u32;
        Self {
            bytes,
            is_latin1: true,
            length,
        }
    }

    /// P11.1-S2-a — build-time encoding decision for a string
    /// literal source string.
    ///
    /// Scans the source string's codepoints once. If every
    /// codepoint fits in Latin-1 (≤ 0xFF) the payload is encoded
    /// as one byte per code unit, matching the pre-S1 byte-Str
    /// layout exactly for the ASCII subset every existing fixture
    /// uses. Otherwise the payload is encoded as UTF-16 little-
    /// endian; codepoints in the BMP get a single u16 unit,
    /// codepoints in the supplementary planes (> 0xFFFF) split
    /// into a surrogate pair (high U+D800-U+DBFF + low
    /// U+DC00-U+DFFF) so `.length` matches ES `String.length` and
    /// `.charCodeAt(i)` indexes by code unit per spec §6.1.4.
    ///
    /// Mirrors V8 `String::Flatten`'s OneByte / TwoByte encoding
    /// split and SpiderMonkey `JSLinearString` Latin-1 / TwoByte
    /// representation, but lifted to AOT compile time — no
    /// startup atomization pass needed (the literal table is
    /// materialized directly into `.rodata`).
    pub fn encode_from_str(s: &str) -> Self {
        Self::encode_from_wtf8(torajs_wtf8::Wtf8::new(s))
    }

    /// Same, from the WTF-8 spelling a literal carries out of the
    /// lexer: a lone surrogate is one BMP code unit and lands in the
    /// UTF-16 payload as itself.
    pub fn encode_from_wtf8(s: &torajs_wtf8::Wtf8) -> Self {
        // Single pass: track max codepoint to pick encoding
        // without a second walk.
        let mut max_cp: u32 = 0;
        for cp in s.code_points() {
            if cp > max_cp {
                max_cp = cp;
            }
        }
        let is_latin1 = max_cp <= 0xFF;
        if is_latin1 {
            // Every codepoint fits in one byte; re-encode as
            // Latin-1 (one byte per code unit, identical to the
            // codepoint numeric value). For pure ASCII payloads
            // this is also identical to the source UTF-8 bytes —
            // every existing fixture lands here and stays
            // byte-identical.
            let bytes: Vec<u8> = s.code_points().map(|cp| cp as u8).collect();
            let length = bytes.len() as u32;
            Self {
                bytes,
                is_latin1: true,
                length,
            }
        } else {
            let mut bytes: Vec<u8> = Vec::with_capacity(s.len() * 2);
            let mut length: u32 = 0;
            for cp in s.code_points() {
                if cp <= 0xFFFF {
                    // BMP — single u16 code unit, little-endian.
                    bytes.extend_from_slice(&(cp as u16).to_le_bytes());
                    length += 1;
                } else {
                    // Supplementary plane (> 0xFFFF, ≤ 0x10FFFF
                    // per Unicode). Surrogate pair encode per
                    // UTF-16 spec:
                    //   cp' = cp - 0x10000  (0..0xFFFFF, 20 bits)
                    //   hi  = 0xD800 | (cp' >> 10)  (top 10 bits)
                    //   lo  = 0xDC00 | (cp' & 0x3FF) (bottom 10)
                    let cp_off = cp - 0x10000;
                    let hi = 0xD800 | ((cp_off >> 10) as u16);
                    let lo = 0xDC00 | ((cp_off & 0x3FF) as u16);
                    bytes.extend_from_slice(&hi.to_le_bytes());
                    bytes.extend_from_slice(&lo.to_le_bytes());
                    length += 2; // surrogate pair = 2 code units
                }
            }
            Self {
                bytes,
                is_latin1: false,
                length,
            }
        }
    }
}

/// Hand-built fib(n: i64) -> i64 module — the same shape the retired
/// LLVM-gate spike (labs/0002, removed with the inkwell backend)
/// emitted as LLVM IR. Used by `tr ssa-demo` to validate the IR types
/// + pretty printer before the lowerer (step 2) existed.
pub fn demo_fib40() -> Module {
    let mut m = Module::default();
    let mut fib = Function::new("fib", Type::I64);
    let n = fib.add_param(Type::I64, "n");
    let bb_entry = fib.add_block();
    let bb_base = fib.add_block();
    let bb_recurse = fib.add_block();

    // bb_entry:  %t = icmp slt %n, 2;  cond_br %t, bb_base, bb_recurse
    let t = fib.append_inst(
        bb_entry,
        InstKind::ICmp(IPred::Slt, Operand::Value(n), Operand::ConstI64(2)),
        Type::Bool,
        Some("t"),
    );
    fib.set_term(
        bb_entry,
        Terminator::CondBr {
            cond: Operand::Value(t),
            then_blk: bb_base,
            else_blk: bb_recurse,
        },
    );

    // bb_base:   ret %n
    fib.set_term(bb_base, Terminator::Ret(Some(Operand::Value(n))));

    // bb_recurse: %a = sub %n, 1
    //             %r1 = call fib(%a)
    //             %b = sub %n, 2
    //             %r2 = call fib(%b)
    //             %s = add %r1, %r2
    //             ret %s
    let a = fib.append_inst(
        bb_recurse,
        InstKind::BinOp(BinOp::Sub, Operand::Value(n), Operand::ConstI64(1)),
        Type::I64,
        Some("a"),
    );
    let fib_id = FuncId(0); // first function in this module
    let r1 = fib.append_inst(
        bb_recurse,
        InstKind::Call(fib_id, vec![Operand::Value(a)]),
        Type::I64,
        Some("r1"),
    );
    let b = fib.append_inst(
        bb_recurse,
        InstKind::BinOp(BinOp::Sub, Operand::Value(n), Operand::ConstI64(2)),
        Type::I64,
        Some("b"),
    );
    let r2 = fib.append_inst(
        bb_recurse,
        InstKind::Call(fib_id, vec![Operand::Value(b)]),
        Type::I64,
        Some("r2"),
    );
    let s = fib.append_inst(
        bb_recurse,
        InstKind::BinOp(BinOp::Add, Operand::Value(r1), Operand::Value(r2)),
        Type::I64,
        Some("s"),
    );
    fib.set_term(bb_recurse, Terminator::Ret(Some(Operand::Value(s))));

    m.add_function(fib);
    m
}
