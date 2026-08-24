//! `__torajs_jsb_stringify_shape` — whole-object JSON.stringify in
//! one call, driven by a compile-time shape descriptor.
//!
//! The per-field jsb lane (`ssa_lower_json_stringify/
//! composite_obj_jsb.rs`) spells a 4-field record as 11 cross-
//! archive calls per stringify — `jsb_new`, a `begin_field` +
//! `push_str_raw`(key) + `push_<ty>` triple per field, the brace
//! bytes, `finalize`. The S1-A2 decomposition
//! (`.claude/rfcs/20260824-s1a2-alloc-decomp`, stage S04-S07) put
//! that call fabric at ~40% of json-stringify self-time. The layout
//! is compile-time static, so the SSA emitter can hand the whole
//! plan over at once: this kernel walks a descriptor blob and runs
//! the same builder machinery in one call, every push a plain
//! in-crate (inlinable) call.
//!
//! ## Descriptor encoding
//!
//! The descriptor rides in an interned Str literal (Latin-1, so the
//! payload bytes are verbatim); every scalar is 7-bit so the blob
//! stays valid UTF-8 for the intern surface:
//!
//! ```text
//! [n_fields+1: u7]
//! n × [key_len+1: u7] [ty+1: u7] [slot_idx+1: u7] [key bytes]
//! ```
//!
//! Scalars are stored +1 (1..=128) so the blob never carries a NUL
//! byte. `ty`: 1 = I64, 2 = Bool, 3 = Str. `key bytes` are the pre-quoted
//! `"name":` spelling the per-field lane interned. The field lives
//! at `OBJ_HEADER_SIZE + slot_idx * 8` — the emitter rejects
//! layouts that overflow any u7 (or carry a non-ASCII key) and
//! keeps them on the per-field lane.
//!
//! Semantics are the per-field lane's, by construction: the same
//! `pending_sep` comma protocol, the same §25.5.2.4 step 8.b
//! undefined-Str key skip, the same quoted-lane escape walk.

use crate::json_builder::{
    __torajs_jsb_finalize, __torajs_jsb_new, __torajs_jsb_push_str_quoted, write_i64_into,
};
use crate::layout::{STR_DATA_OFF, STR_LEN_OFF};

/// Mirror of `torajs-core/src/ssa_lower.rs` `OBJ_HEADER_SIZE` (the
/// `arg_struct_coerce.rs` mirror in torajs-anyvalue is the same
/// pattern). Field i of a struct object lives at
/// `OBJ_HEADER_SIZE + i * 8`.
const OBJ_HEADER_SIZE: usize = 32;

/// One-call `JSON.stringify` for a primitive-only static layout.
///
/// # Safety
///
/// `desc` is a live Latin-1 Str block holding a well-formed
/// descriptor (the emitter is the only producer); `obj` is a live
/// struct object whose slots match the descriptor's types. Returns
/// a fresh refcount=1 Str.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_jsb_stringify_shape(desc: *const u8, obj: *const u8) -> *mut u8 {
    let desc_len = unsafe { (desc.add(STR_LEN_OFF) as *const u32).read() } as usize;
    let d = unsafe { core::slice::from_raw_parts(desc.add(STR_DATA_OFF), desc_len) };
    let n_fields = (d[0] - 1) as usize;
    // Static bytes + digits headroom; the builder grows on demand.
    let sb = unsafe { __torajs_jsb_new((desc_len + 2 + n_fields * 20) as u32) };
    // Reborrow `sb` per step — no `&mut` may live across the raw
    // `push_str_quoted` call below (aliasing).
    unsafe { (*sb).buf.push(b'{') };
    let mut p = 1usize;
    for _ in 0..n_fields {
        let key_len = (d[p] - 1) as usize;
        let ty = d[p + 1];
        let slot = (d[p + 2] - 1) as usize;
        let key = &d[p + 3..p + 3 + key_len];
        p += 3 + key_len;
        let field = unsafe { obj.add(OBJ_HEADER_SIZE + slot * 8) };
        match ty {
            1 => {
                let v = unsafe { (field as *const i64).read() };
                let b = unsafe { &mut *sb };
                begin_field(b);
                b.buf.extend_from_slice(key);
                write_i64_into(&mut b.buf, v);
            }
            2 => {
                // Bool slots guarantee only their low byte.
                let v = unsafe { field.read() };
                let b = unsafe { &mut *sb };
                begin_field(b);
                b.buf.extend_from_slice(key);
                b.buf
                    .extend_from_slice(if v != 0 { b"true" } else { b"false" });
            }
            _ => {
                let v = unsafe { (field as *const *const u8).read() };
                // §25.5.2.4 step 8.b — an undefined Str field skips
                // its key entirely (and leaves pending_sep alone).
                if crate::undef_sentinel::is_undef(v) {
                    continue;
                }
                {
                    let b = unsafe { &mut *sb };
                    begin_field(b);
                    b.buf.extend_from_slice(key);
                }
                unsafe { __torajs_jsb_push_str_quoted(sb, v) };
            }
        }
    }
    unsafe { (*sb).buf.push(b'}') };
    unsafe { __torajs_jsb_finalize(sb) }
}

/// The `begin_field` comma protocol, in-crate so it inlines.
#[inline]
fn begin_field(b: &mut crate::json_builder::JsonBuilder) {
    if b.pending_sep {
        b.buf.push(b',');
    }
    b.pending_sep = true;
}
