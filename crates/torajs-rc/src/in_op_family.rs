//! Which builtin prototype an `in` receiver's chain face starts at —
//! split out of [`crate::in_op_any`] (file-size hard limit; the
//! parent keeps the kernel, this file keeps "which family").

use core::ffi::c_void;

/// Heap `Tag` → builtin-proto family tag (`torajs-rc/builtin_proto.rs`
/// order: Number=0 Object=1 Array=2 String=3 Boolean=4 … RegExp=7
/// Date=8 Promise=10 Map=11 Set=12 Function=13). `None` for cells
/// with no builtin prototype on their chain (iterators, weak
/// collections, accessor pairs) — the chain face answers false.
pub(crate) fn proto_family_of(ptr: *const c_void, type_tag: u16) -> Option<i64> {
    use crate::Tag;
    use crate::builtin_proto::{ARRAY_PROTO_TAG, FUNCTION_PROTO_TAG, OBJECT_PROTO_TAG};
    let t = type_tag;
    let family = if t == Tag::Obj as u16 || t == Tag::DynObj as u16 {
        // A struct's own-class prototype face is a recorded boundary
        // (module doc); the Object root still answers the universal
        // names.
        OBJECT_PROTO_TAG as i64
    } else if t == Tag::Arr as u16 {
        ARRAY_PROTO_TAG as i64
    } else if t == Tag::Closure as u16 {
        FUNCTION_PROTO_TAG as i64
    } else if t == Tag::RegExp as u16 {
        7
    } else if t == Tag::Date as u16 {
        8
    } else if t == Tag::Promise as u16 {
        10
    } else if t == Tag::Map as u16 {
        11
    } else if t == Tag::Set as u16 {
        12
    } else if t == Tag::NumberWrapper as u16 {
        0
    } else if t == Tag::StringWrapper as u16 {
        3
    } else if t == Tag::BooleanWrapper as u16 {
        4
    } else if t == Tag::WeakMap as u16 {
        16
    } else if t == Tag::WeakSet as u16 {
        17
    } else if t == Tag::WeakRef as u16 {
        18
    } else if t == Tag::MapIter as u16 || t == Tag::ArrIter as u16 || t == Tag::IterHelper as u16 {
        // §23.1.5.2 — an iterator hangs off its PER-FAMILY prototype,
        // which the cell itself has to name (an ArrIter over a
        // character array is a STRING iterator), and that in turn
        // hangs off %Iterator.prototype%. Without a row here `"next"
        // in [1].values()` answered false.
        unsafe { crate::builtin_proto::iter_proto::iter_cell_proto_tag(ptr, t) }
    } else {
        return None;
    };
    if family < 0 {
        return None;
    }
    Some(family)
}
