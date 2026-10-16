//! The two lookups in `instanceof` that answer from a NAME and a
//! TYPE alone.
//!
//! Neither takes a `LowerCtx`, neither emits an instruction, and
//! that is the seam: everything left in the parent decides how to
//! ASK the question at run time, while these two decide when it
//! does not have to be asked at all (`try_compile_time_fold`) and
//! which builtin prototype a name stands for
//! (`builtin_proto_tag`).
//!
//! Keeping them here also keeps the two tables where a reader
//! looking for "what does this class name mean" finds both at
//! once, next to the tag tables in the `tags` sibling.
//!
//! Pure position — both items are byte-for-byte what they were in
//! the parent.

use crate::ssa::Type;

pub(super) fn try_compile_time_fold(actual_ty: Type, class_name: &str) -> Option<bool> {
    if matches!(actual_ty, Type::Any) {
        return match class_name {
            // RFC 20260716 刀 2 — `x instanceof {Number,String,Boolean}`
            // for Any x now dispatches at runtime against
            // Tag::*Wrapper; Symbol joined 2026-08-19 (`Object(sym)`
            // mints a Tag::SymbolWrapper = 24 cell, surfaced by the
            // deepEqual.js boxed-symbol self-test). BigInt stays
            // compile-time false — `Object(bigint)` performs no
            // boxing today, so no wrapper cell can ever exist.
            "BigInt" => Some(false),
            _ => None,
        };
    }
    match (class_name, &actual_ty) {
        ("Array", Type::Arr(_)) => Some(true),
        ("Array", _) => Some(false),
        ("Function", Type::Closure(_)) | ("Function", Type::FnSig(_)) => Some(true),
        ("Function", _) => Some(false),
        ("Object", Type::Obj(_)) => Some(true),
        ("Object", Type::Arr(_)) => Some(true),
        ("Object", Type::Date) => Some(true),
        ("Object", Type::RegExp) => Some(true),
        ("Object", Type::Promise) => Some(true),
        ("Object", Type::Map | Type::Set | Type::WeakMap | Type::WeakSet) => Some(true),
        ("Object", Type::MapIter | Type::ArrIter) => Some(true),
        ("Object", Type::Closure(_)) | ("Object", Type::FnSig(_)) => Some(true),
        ("Object", _) => Some(false),
        ("Number" | "String" | "Boolean" | "BigInt" | "Symbol", _) => Some(false),
        ("Date", Type::Date) => Some(true),
        ("Date", _) => Some(false),
        ("RegExp", Type::RegExp) => Some(true),
        ("RegExp", _) => Some(false),
        ("Promise", Type::Promise) => Some(true),
        ("Promise", _) => Some(false),
        ("Map", Type::Map) => Some(true),
        ("Map", _) => Some(false),
        ("Set", Type::Set) => Some(true),
        ("Set", _) => Some(false),
        ("WeakMap", Type::WeakMap) => Some(true),
        ("WeakMap", _) => Some(false),
        ("WeakSet", Type::WeakSet) => Some(true),
        ("WeakSet", _) => Some(false),
        ("WeakRef", Type::WeakRef) => Some(true),
        ("WeakRef", _) => Some(false),
        _ => None,
    }
}

pub(super) fn builtin_proto_tag(class_name: &str) -> Option<i64> {
    Some(match class_name {
        "Number" => 0,
        "Array" => 2,
        "String" => 3,
        "Boolean" => 4,
        "Symbol" => 5,
        "RegExp" => 7,
        "Date" => 8,
        "Promise" => 10,
        "Map" => 11,
        "Set" => 12,
        "Function" => 13,
        "WeakMap" => 16,
        "WeakSet" => 17,
        "WeakRef" => 18,
        "ArrayBuffer" => 19,
        "DataView" => 32,
        "FinalizationRegistry" => torajs_rc::builtin_proto::FINREG_PROTO_TAG as i64,
        _ => return None,
    })
}
