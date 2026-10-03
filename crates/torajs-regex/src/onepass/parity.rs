//! The Pike VM side of the matcher parity tests: the programs both
//! sides run, the VM's answer, and the transcoding of a UTF-16
//! payload the VM reads.

use alloc::vec;
use alloc::vec::Vec;

use crate::compiler::compile;
use crate::parser::{Parser, RE_FLAG_Y};
use crate::program::{Inst, Program};
use crate::vm::Workspace;
use crate::vm::match_at::vm_match_at;

/// the program the Pike VM runs (UTF-8 haystack) and the one the
/// matcher is built from (code units); they differ only where a
/// class is unsafe for the byte walk
pub(super) fn programs(pat: &str, flags: u8) -> (Program, Program) {
    let mut p = Parser::new(pat.as_bytes(), flags);
    let root = p.parse().expect("parse failed");
    let mut vm = Program::new();
    compile(&mut vm, &root, flags);
    vm.emit(Inst::match_accept());
    vm.has_save = vm.any_save();
    vm.finalize_backref_caps();
    let mut raw = Program::new();
    raw.raw_classes = true;
    compile(&mut raw, &root, flags);
    raw.emit(Inst::match_accept());
    (vm, raw)
}

/// the Pike VM's leftmost-first answer from byte `from`: start, end, row
pub(super) fn pike(
    prog: &Program,
    s: &[u8],
    from: usize,
    flags: u8,
) -> Option<(i64, i64, Vec<i64>)> {
    let mut ws = Workspace::for_program(prog);
    let mut st = from;
    while st <= s.len() {
        let mut row = vec![-1i64; ws.arena.stride];
        let end = vm_match_at(prog, s, st as i64, flags, &mut ws, Some(&mut row), -1);
        if end >= 0 {
            row[0] = st as i64;
            row[1] = end;
            return Some((st as i64, end, row));
        }
        if flags & RE_FLAG_Y != 0 {
            break;
        }
        // the VM scans code points of its UTF-8 form
        st += 1;
        while st < s.len() && s[st] & 0xC0 == 0x80 {
            st += 1;
        }
    }
    None
}

/// The runtime's transcoding of a UTF-16 payload (`str_helpers`):
/// WTF-8 code units, or code points with the pairs merged under `u`.
/// Also the unit index of every byte offset the VM can answer.
pub(super) fn transcode(units: &[u16], merge: bool) -> (Vec<u8>, Vec<i64>) {
    let mut out = Vec::new();
    let mut unit_of = Vec::new();
    let mut i = 0;
    while i < units.len() {
        let cu = units[i] as u32;
        let (cp, w) = if merge
            && (0xD800..=0xDBFF).contains(&cu)
            && i + 1 < units.len()
            && (0xDC00..=0xDFFF).contains(&(units[i + 1] as u32))
        {
            (
                0x10000 + ((cu - 0xD800) << 10) + (units[i + 1] as u32 - 0xDC00),
                2,
            )
        } else {
            (cu, 1)
        };
        let mut buf = [0u8; 4];
        let n = crate::utf8::utf8_encode_cp(cp as i32, &mut buf);
        for (k, &b) in buf[..n].iter().enumerate() {
            unit_of.push(if k == 0 { i as i64 } else { -1 });
            out.push(b);
        }
        i += w;
    }
    unit_of.push(units.len() as i64);
    (out, unit_of)
}
