//! `__torajs_regex_compile` — port of `runtime_regex.c` L1445-1512.
//!
//! Drives the full compile pipeline:
//! 1. Decode pattern + flag-string bytes (Str payloads).
//! 2. Parse with [`crate::parser::Parser`] to an AST.
//! 3. Resolve `\k<name>` + validate `\N` via
//!    [`crate::resolve::resolve_backrefs`].
//! 4. Build named-capture name table for `.groups` construction at
//!    match time.
//! 5. Compile to bytecode via [`crate::compiler::compile`].
//! 6. On parse failure: emit a never-match stub program + mark
//!    `rejected = 1` so the surface methods abort with
//!    "not yet supported:" rather than producing wrong matches.
//!
//! The resulting `RegExp` is heap-allocated via
//! `Box::into_raw(Box::new(...))` so the universal header sits at
//! offset 0 of the allocation block. `__torajs_value_drop_heap`'s
//! tag dispatch on `header.type_tag = TAG_REGEX` routes drops back
//! to [`super::lifecycle::__torajs_regex_drop`].

use alloc::{boxed::Box, vec::Vec};
use core::ffi::c_void;

use super::{HeapHeader, RegExp, TAG_REGEX, str_slice};
use crate::compiler::compile;
use crate::flags::parse_flags;
use crate::parser::Parser;
use crate::program::{Inst, Program};
use crate::resolve::resolve_backrefs;

/// # Safety
///
/// `pattern_str` and `flags_str` must point at live `Str *` heap
/// objects (refcount > 0; well-formed header). The returned
/// pointer is heap-allocated with refcount = 1 and type_tag =
/// `TAG_REGEX`; release with `__torajs_regex_drop`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_compile(
    pattern_str: *const c_void,
    flags_str: *const c_void,
) -> *mut c_void {
    let pat = unsafe { str_slice(pattern_str) };
    let fl = unsafe { str_slice(flags_str) };

    let flag_bits = parse_flags(&fl);
    let src_bytes = pat.clone();

    let mut parser = Parser::new(&pat, flag_bits);
    let parse_result = parser.parse();
    let n_captures = parser.n_captures;

    // Build a Vec<Vec<u8>> snapshot of the name table for later
    // attach_groups; the parser's names are still valid here.
    let names_snapshot = parser.names.clone();

    // Resolve backrefs against the now-known capture count + name
    // table. Failure here promotes the regex to `rejected`.
    let mut root_ok = match parse_result {
        Some(mut root) => {
            if resolve_backrefs(&mut root, &names_snapshot, n_captures) {
                Some(root)
            } else {
                None
            }
        }
        None => None,
    };

    // Persist named-capture table for `.groups` construction at
    // match time. Owned `Vec<Vec<u8>>` survives the parser drop —
    // matches the C port's malloc+memcpy.
    let mut capture_names: Vec<Vec<u8>> = Vec::with_capacity(n_captures + 1);
    let mut n_named_captures: i32 = 0;
    if root_ok.is_some() && n_captures > 0 {
        // Index 0 reserved.
        capture_names.push(Vec::new());
        for i in 1..=n_captures {
            let name = names_snapshot.get(i).cloned().unwrap_or_default();
            if !name.is_empty() {
                n_named_captures += 1;
            }
            capture_names.push(name);
        }
    }

    // Compile + emit terminator. On parse failure, emit a
    // never-match stub (OP_CHAR 0xff + OP_MATCH) so the `.test()`
    // path returns false silently.
    let mut prog = Program::new();
    let rejected = if let Some(root) = root_ok.take() {
        compile(&mut prog, &root);
        prog.emit(Inst::match_accept());
        0u8
    } else {
        prog.emit(Inst::char_lit(0xff));
        prog.emit(Inst::match_accept());
        1u8
    };
    // V0.2 P14-S17 — finalise saves stride on the top-level Program
    // (sub_progs are finalised at `add_sub` time inside `compile_*`).
    prog.compute_saves_stride();

    // V0.2 P14-S2 perf — detect a literal-byte prefix anchor.
    // Walk the emitted bytecode forward, skipping zero-width
    // bookkeeping ops (`Save` for `(...)` capture brackets,
    // `AnchorB` for `^`, `WBound` etc), until we either hit an
    // `OP_CHAR(b)` (set anchor) or a byte-consuming op of a
    // different shape (`AnyChar` / `Class` / `Split` / ... → no
    // anchor). The i flag invalidates the byte comparison
    // (memchr can't match both cases without a lookup table) so
    // skip the anchor entirely there. The u flag is fine — leading
    // `Char` ops only emit ASCII bytes; non-ASCII literals decode
    // to `Class` at parse time which lands in the "different
    // shape" branch.
    if rejected == 0 && flag_bits & crate::parser::RE_FLAG_I == 0 {
        for inst in &prog.insts {
            match crate::program::Op::from_u8(inst.op) {
                Some(crate::program::Op::Save)
                | Some(crate::program::Op::AnchorB)
                | Some(crate::program::Op::AnchorE)
                | Some(crate::program::Op::WBound)
                | Some(crate::program::Op::NWBound) => continue,
                Some(crate::program::Op::Char) => {
                    prog.prefix_byte = Some(inst.ch);
                    break;
                }
                _ => break,
            }
        }
    }

    let re = Box::new(RegExp {
        header: HeapHeader {
            refcount: 1,
            type_tag: TAG_REGEX,
            flags: 0,
        },
        flags: flag_bits,
        rejected,
        _pad: [0; 2],
        n_captures: n_captures as i32,
        prog,
        src_bytes,
        capture_names,
        n_named_captures,
        last_index: 0,
        // V0.2 P14-S8 — lazy-init Pike VM workspace cache.
        workspace_cache: core::cell::UnsafeCell::new(None),
    });
    Box::into_raw(re) as *mut c_void
}
