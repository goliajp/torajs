//! The `name` / ctor-clause `length` of every builtin constructor —
//! carved out of the parent module when the proto-tag roster grew it
//! past the file-size limit (566-03). Verbatim move: the table, its
//! order contract and its spec citations are unchanged.

/// ES `name` / ctor-clause `length` of the builtin constructor
/// owning each proto tag (RFC 20260720-ctor-static-reflection 刀 3)
/// — the single source both the lowering's ctor-namespace member
/// fold and the runtime reflection probes read (bun-verified
/// 14/14). Lengths per the ctor clauses: §21.1.1 / §20.1.1 /
/// §23.1.1 / §22.1.1 / §20.3.1 / §20.4.1 (Symbol 0) / §21.2.1 /
/// §22.2.4 (RegExp 2) / §21.4.2 (Date 7) / §20.5.1 / §27.2.3 /
/// §24.1.1 (Map 0) / §24.2.2 (Set 0) / §20.2.1 / §27.7.1
/// (AsyncFunction 1) / §27.1.3.1 (Iterator 0) / §24.3.1
/// (WeakMap 0) / §24.4.1 (WeakSet 0) / §26.1.1 (WeakRef 1).
pub fn builtin_ctor_meta(tag: i64) -> Option<(&'static str, u32)> {
    Some(match tag {
        0 => ("Number", 1),
        1 => ("Object", 1),
        2 => ("Array", 1),
        3 => ("String", 1),
        4 => ("Boolean", 1),
        5 => ("Symbol", 0),
        6 => ("BigInt", 1),
        7 => ("RegExp", 2),
        8 => ("Date", 7),
        9 => ("Error", 1),
        10 => ("Promise", 1),
        11 => ("Map", 0),
        12 => ("Set", 0),
        13 => ("Function", 1),
        14 => ("AsyncFunction", 1),
        15 => ("Iterator", 0),
        16 => ("WeakMap", 0),
        17 => ("WeakSet", 0),
        18 => ("WeakRef", 1),
        // §25.1.4.1 ArrayBuffer takes (length, options) and declares
        // length 1; every §23.2.5 typed-array constructor declares 3.
        19 => ("ArrayBuffer", 1),
        20 => ("Int8Array", 3),
        21 => ("Uint8Array", 3),
        22 => ("Uint8ClampedArray", 3),
        23 => ("Int16Array", 3),
        24 => ("Uint16Array", 3),
        25 => ("Int32Array", 3),
        26 => ("Uint32Array", 3),
        27 => ("Float32Array", 3),
        28 => ("Float64Array", 3),
        29 => ("BigInt64Array", 3),
        30 => ("BigUint64Array", 3),
        31 => ("Float16Array", 3),
        // 565-01 — §25.3.2 DataView takes (buffer, byteOffset,
        // byteLength) and declares length 1. Its proto slot sits
        // after the per-kind block; without a row here it was the
        // one bound builtin constructor with no name, no ctor-clause
        // length, and the plain `[Function]` print form.
        32 => ("DataView", 1),
        _ => return None,
    })
}
