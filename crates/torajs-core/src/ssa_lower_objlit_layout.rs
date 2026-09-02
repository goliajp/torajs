//! ObjectLit layout resolution — registered-layout matching plus the
//! W5-follow-up numeric-width coercion (ann-width RFC §5.4 S3 face).
//!
//! An object literal's field types come from the lowered **values**
//! (`{sum: s}` with an F64 `s` carries an F64 slot), while every
//! reader resolves the **annotated** layout (`type S = {sum: number}`
//! registers `sum: I64` via parse_type). Before this module, a
//! width-mismatched literal silently auto-registered an anonymous
//! twin layout — writer stored f64 bits, reader loaded them as i64,
//! and the value came back as raw-bit garbage (repro S3 /
//! integration-004 after W5 widened accumulator cells to F64).
//!
//! Resolution order:
//! 1. exact match (modulo the `null`-literal Ptr-vs-pointer-shaped
//!    allowance) — the historical fast path, unchanged;
//! 2. numeric-coercible match: every field name matches and the only
//!    disagreements are F64-vs-I64 — the literal coerces to the
//!    registered width (`SiToFp` is lossless for |v| ≤ 2^53;
//!    `FpToSi` truncates a fractional value, which is the
//!    pre-existing W4 container-width debt, strictly better than
//!    bit-punning);
//! 3. auto-register an anonymous layout (V3-18 P2.4.c, unchanged).

use crate::ast::PropKey;
use crate::ssa::{BlockId, Function, InstKind, Operand, StructId, Type};

/// Pick (or register) the layout for an object literal whose lowered
/// field types are `field_tys`, then bring `field_tys` / `field_vals`
/// in line with the canonical registered widths so downstream
/// Store-typing emits the right slot types.
///
/// `declared_hint` (chunk 780) pins the annotated let-decl layout
/// when the init is a direct ObjectLit: two same-shaped literals
/// under different declared struct types (`Box<number>` vs
/// `Box<string>` generic instantiations both lower `{v:
/// undefined-ptr, label: str}`) would otherwise first-match into
/// whichever compatible layout registered first — the writer then
/// stores the wrong slot repr for the reader's declared layout.
/// The hint only wins when it is itself compatible; anything else
/// falls back to the scan (checker already rejected true
/// mismatches, so this is belt-and-braces, not a bypass).
pub(crate) fn resolve_objlit_layout(
    layouts: &mut Vec<Vec<(PropKey, Type)>>,
    f: &mut Function,
    cur_block: BlockId,
    field_tys: &mut [(PropKey, Type)],
    field_vals: &mut [Operand],
    declared_hint: Option<StructId>,
) -> StructId {
    let exact = |reg: &Vec<(PropKey, Type)>| -> bool {
        reg.len() == field_tys.len()
            && reg
                .iter()
                .zip(field_tys.iter())
                .all(|((rn, rt), (ln, lt))| {
                    rn == ln && (rt == lt || (*lt == Type::Ptr && rt.is_pointer_shaped()))
                })
    };
    let coercible = |reg: &Vec<(PropKey, Type)>| -> bool {
        reg.len() == field_tys.len()
            && reg
                .iter()
                .zip(field_tys.iter())
                .all(|((rn, rt), (ln, lt))| {
                    rn == ln
                        && (rt == lt
                            || (*lt == Type::Ptr && rt.is_pointer_shaped())
                            || (*lt == Type::F64 && *rt == Type::I64)
                            || (*lt == Type::I64 && *rt == Type::F64)
                            // RFC 20260710 C4 — a declared-Any slot
                            // (`__nullable(number|boolean)` optional
                            // field, plain `any` field) admits a raw
                            // scalar literal value; the caller boxes
                            // it after layout resolution (only a
                            // retype happens here).
                            || (*rt == Type::Any
                                && matches!(*lt, Type::I64 | Type::I32 | Type::F64 | Type::Bool)))
                })
    };

    // Unbox mirror of the C4 arm, HINT-ONLY: a declared slot T taking
    // an Any-valued field expr (`{ value: x }` with `x: any` under a
    // `{ value: number }` declared layout). The slot retypes to T here
    // and the ObjectLit caller unboxes the value after resolution —
    // same division of labor as C4's box direction. Deliberately NOT
    // part of `coercible`: in the un-hinted fallback scan this arm
    // would let an anon literal adsorb onto any same-named typed
    // layout and silently change existing layout selection.
    let coercible_unbox = |reg: &Vec<(PropKey, Type)>| -> bool {
        reg.len() == field_tys.len()
            && reg
                .iter()
                .zip(field_tys.iter())
                .all(|((rn, rt), (ln, lt))| {
                    rn == ln
                        && (rt == lt
                            || (*lt == Type::Ptr && rt.is_pointer_shaped())
                            || (*lt == Type::F64 && *rt == Type::I64)
                            || (*lt == Type::I64 && *rt == Type::F64)
                            || (*rt == Type::Any
                                && matches!(*lt, Type::I64 | Type::I32 | Type::F64 | Type::Bool))
                            || (*lt == Type::Any
                                && matches!(
                                    *rt,
                                    Type::I64 | Type::F64 | Type::Bool | Type::Str | Type::BigInt
                                )))
                })
    };
    let hinted = declared_hint.filter(|sid| {
        layouts
            .get(sid.0 as usize)
            .is_some_and(|reg| exact(reg) || coercible(reg) || coercible_unbox(reg))
    });
    let sid = match hinted.or_else(|| {
        layouts
            .iter()
            .position(exact)
            .or_else(|| layouts.iter().position(coercible))
            .map(|i| StructId(i as u32))
    }) {
        Some(sid) => sid,
        None => {
            let new_id = StructId(layouts.len() as u32);
            layouts.push(field_tys.to_vec());
            new_id
        }
    };

    // Canonicalize to the registered layout. A numeric width mismatch
    // emits the cast inline (case 2 above); everything else (the Ptr
    // null-literal allowance, the C4 declared-Any slot) just retypes
    // the slot — the ObjectLit caller boxes raw values behind
    // Any-typed slots after resolution (it owns the LowerCtx the
    // box helpers need).
    let canon = layouts[sid.0 as usize].clone();
    for (i, (_, reg_ty)) in canon.iter().enumerate() {
        let lit_ty = field_tys[i].1;
        if lit_ty == Type::F64 && *reg_ty == Type::I64 {
            let c = f.append_inst(
                cur_block,
                InstKind::FpToSi(field_vals[i].clone()),
                Type::I64,
                None,
            );
            field_vals[i] = Operand::Value(c);
        } else if lit_ty == Type::I64 && *reg_ty == Type::F64 {
            let c = f.append_inst(
                cur_block,
                InstKind::SiToFp(field_vals[i].clone()),
                Type::F64,
                None,
            );
            field_vals[i] = Operand::Value(c);
        }
        field_tys[i].1 = *reg_ty;
    }
    sid
}
