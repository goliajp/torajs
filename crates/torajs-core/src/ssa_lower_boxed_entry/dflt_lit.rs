//! S2.39/S3.8 — the qualifying literal-default family: the enum both
//! table builders classify into, and the `(tag, bits)` encoding the
//! `__torajs_anyv_or_default` kernel consumes. Split from the parent
//! adapter-synthesis module (file-size line); the parent re-exports,
//! so consumer paths are unchanged.

use std::collections::HashMap;

use crate::ssa::{self, FuncId};

/// S2.39 — one qualifying literal default (`b = 39` / `flag = true` /
/// `d = "d"`), substituted by the adapter when the argv slot arrives
/// undefined.
#[derive(Clone)]
pub(crate) enum DfltLit {
    Num(f64),
    Bool(bool),
    /// A string literal default whose UTF-8 body fits the ShortStr
    /// inline encoding (≤ 5 bytes) — stored as the complete prebaked
    /// nanbox bit pattern.
    Str(u64),
    /// A string literal default past the ShortStr cap. No compile-time
    /// constant box exists — the substitution mints an interned
    /// `.rodata` global (rc no-op via FLAG_STATIC_LITERAL) and passes
    /// its pointer bits under the kernel's Heap tag. The terminal
    /// interns through LowerCtx; the adapter's unbox pushes
    /// `module.strings` directly (synthesis precedes every per-fn
    /// body lower, so the StringId is final at mint time).
    StrLong(torajs_wtf8::Wtf8Buf),
}

/// S3.8 — the Pass-1 per-fn literal-default map threaded to the
/// direct-call terminal (sig-aligned positions; only fns with at
/// least one qualifying literal have an entry).
pub(crate) type FnDfltLits = HashMap<FuncId, Vec<Option<DfltLit>>>;

impl DfltLit {
    /// The one qualifying-literal classifier both table builders
    /// share (Pass 1's `dflt_lits_of_params` and the boxed-adapter
    /// targets loop). An expression default may reference prior
    /// params and needs real callee-side evaluation — only these
    /// self-contained literal shapes qualify.
    pub(crate) fn of_default_expr(e: &crate::ast::Expr) -> Option<DfltLit> {
        match e {
            crate::ast::Expr::Number(n) => Some(DfltLit::Num(*n)),
            crate::ast::Expr::Bool(b) => Some(DfltLit::Bool(*b)),
            crate::ast::Expr::String(s) => Some(
                crate::short_str_encode::encode_short_str_literal(s.as_bytes())
                    .map(DfltLit::Str)
                    .unwrap_or_else(|| DfltLit::StrLong(s.clone())),
            ),
            _ => None,
        }
    }

    /// The `(tag, bits)` pair `__torajs_anyv_or_default` bakes the
    /// literal as. Integral numbers box as tag-2 i64, fractional as
    /// tag-3 f64 bits — the same split the typed-return boxing uses.
    /// Short strings pass tag 6: their `bits` ARE the complete
    /// prebaked box, no pair decode. `None` for [`DfltLit::StrLong`]
    /// — its box is an interned global only the terminal's LowerCtx
    /// can mint ([`DfltLit`] doc). Shared by the adapter's per-slot
    /// unbox and the direct-call terminal's Any-arg coercion (S3.8).
    pub(crate) fn box_encoding(&self) -> Option<(i64, ssa::Operand)> {
        match *self {
            DfltLit::Num(n) if n.fract() == 0.0 && n.abs() < 9.0e15 => {
                Some((2i64, ssa::Operand::ConstI64(n as i64)))
            }
            DfltLit::Num(n) => Some((3i64, ssa::Operand::ConstI64(n.to_bits() as i64))),
            DfltLit::Bool(b) => Some((1i64, ssa::Operand::ConstI64(i64::from(b)))),
            DfltLit::Str(bits) => Some((6i64, ssa::Operand::ConstI64(bits as i64))),
            DfltLit::StrLong(_) => None,
        }
    }
}
