//! Stringify + helper methods on `Type` / `BinOp` / `IPred` / `FPred`.
//!
//! These are small `pub fn as_str(self) -> &'static str` + a few
//! query predicates (`is_copy`, `is_refcounted`, `is_pointer_shaped`
//! on Type) — all data-driven match arms over the enum variant.
//!
//! Extracted from `ssa.rs` (2026-05-25, god-file decomp batch 15).

use super::{BinOp, FPred, IPred, Operand, Type};
use std::hash::{Hash, Hasher};

impl Type {
    pub fn as_str(self) -> &'static str {
        match self {
            Type::I64 => "i64",
            Type::F64 => "f64",
            Type::I32 => "i32",
            Type::Bool => "bool",
            Type::Void => "void",
            Type::Ptr => "ptr",
            Type::Str => "str",
            Type::Substr => "substr",
            Type::RegExp => "regex",
            Type::Date => "date",
            Type::Obj(_) => "obj",
            Type::Arr(_) => "arr",
            Type::FnSig(_) => "fnsig",
            Type::Closure(_) => "closure",
            Type::Any => "any",
            Type::Symbol => "symbol",
            Type::Promise => "promise",
            Type::BigInt => "bigint",
            Type::WeakRef => "weakref",
            Type::WeakMap => "weakmap",
            Type::WeakSet => "weakset",
            Type::Map => "map",
            Type::Set => "set",
            Type::MapIter => "mapiter",
            Type::ArrIter => "arriter",
        }
    }

    /// Cheap-to-duplicate. Used by the lowerer to decide whether a binding
    /// read needs ownership tracking + Drop emission. Mirrors check.rs's
    /// `Type::is_copy()`. Today only `Str` is heap-owned at the SSA layer;
    /// arrays / objects join the non-Copy side as they land.
    pub fn is_copy(self) -> bool {
        matches!(
            self,
            Type::I64
                | Type::F64
                | Type::I32
                | Type::Bool
                | Type::Void
                | Type::FnSig(_)
                | Type::Ptr
        )
        // Str + Obj + Arr are heap-owned, affine.
        // FnSig is just a fn pointer — Copy semantics, no drop.
        // Closure is heap-owned (env block) — non-Copy.
        // Ptr is a raw pointer (env handles, drop-fn ptrs, null
        // sentinels) — non-owning, no drop. Bindings of `let x = null`
        // and similar pointer-shaped slots are POD by reference.
    }

    /// Phase B refcount: returns true if the heap object for this type
    /// begins with `__torajs_heap_header_t` (refcount@0, type_tag@4,
    /// flags@6). `__torajs_rc_inc` / `__torajs_rc_dec` are only safe
    /// to call on values of refcount-aware types.
    ///
    /// Phase 1: `Str`. Phase 2A: `Arr`. Phase 2B: `Obj`. Phase 2C:
    /// `Closure`. Phase Substr.A: `Substr` (also uses universal heap
    /// header; drop is view-aware — dec parent before free).
    pub fn is_refcounted(self) -> bool {
        matches!(
            self,
            Type::Str
                | Type::Substr
                | Type::Arr(_)
                | Type::Obj(_)
                | Type::Closure(_)
                | Type::RegExp
                | Type::Date
                | Type::Any
                | Type::Symbol
                | Type::Promise
                | Type::BigInt
                | Type::WeakRef
                | Type::WeakMap
                | Type::WeakSet
                | Type::Map
                | Type::Set
                | Type::MapIter
                | Type::ArrIter
        )
    }

    /// True when a slot of this type spells JS `undefined` with the
    /// generic immortal cell (`___TORAJS_UNDEF_CELL`, torajs-rc
    /// `undef_cell.rs`) — a bare `Tag::Undefined` header block.
    ///
    /// Every consumer of such a slot branches on the ADDRESS, never
    /// on content, which is why one cell serves the whole family:
    /// strict-eq compares it, drop stations guard on it, print / JSON
    /// probe identity, and the any boundary re-encodes it as
    /// ANY_UNDEF. So the question a consumer actually has is this one
    /// — not "is this an Obj, an Arr or a Closure", which is how it
    /// was spelled at nine sites before, each of them naming three of
    /// the fifteen members.
    ///
    /// Excluded, each for a reason of its own: `Str` and `Substr`
    /// have their own family oddballs (`str_undef_sentinel_for`),
    /// `FnSig` borrows the Str one (it is Copy, no rc traffic),
    /// `Any` carries the answer in its tag, and the scalar slots
    /// have no address to hand out.
    ///
    /// `Ptr` joins the cell set (RFC 20260710 C2b completion): a slot
    /// stays `Ptr` only when nothing but nullish literals were ever
    /// seen for it (`{r: undefined}` with no other type info), so its
    /// value is NULL (JS null) or this cell (JS undefined) — without
    /// the cell the write collapsed both to NULL and the runtime
    /// field reader (`field_slot_to_anyv_borrowed`, which already
    /// normalizes the sentinels for tags 4..=21) answered null for a
    /// written undefined.
    ///
    /// Exhaustive on purpose: a new SSA type must decide which side
    /// it is on rather than falling into a wildcard.
    pub fn spells_undef_with_generic_cell(self) -> bool {
        match self {
            Type::Obj(_)
            | Type::Arr(_)
            | Type::Closure(_)
            | Type::RegExp
            | Type::Date
            | Type::Symbol
            | Type::Promise
            | Type::BigInt
            | Type::WeakRef
            | Type::WeakMap
            | Type::WeakSet
            | Type::Map
            | Type::Set
            | Type::MapIter
            | Type::ArrIter
            | Type::Ptr => true,
            Type::Str | Type::Substr | Type::FnSig(_) | Type::Any => false,
            Type::I64 | Type::F64 | Type::I32 | Type::Bool | Type::Void => false,
        }
    }

    /// V3-05 — true if the SSA value is an i64-wide pointer slot
    /// (heap-owned refcounted types + raw Ptr + bare Promise / Symbol /
    /// any other heap handle). Used by ObjectLit's permissive layout
    /// match so a literal `null` Ptr field maps onto a registered
    /// pointer-shaped class field of any specific tag.
    /// RFC 20260710 C2a — FnSig joins: a fn-typed slot carries a code
    /// address, NULL, or the undefined sentinel (all pointers), and
    /// without it `{ cb: undefined }` failed the declared-layout match
    /// and registered a fresh anon (cb: Ptr) shape the member reader
    /// never saw.
    pub fn is_pointer_shaped(self) -> bool {
        self.is_refcounted() || matches!(self, Type::Ptr | Type::FnSig(_))
    }
}

impl BinOp {
    pub fn as_str(self) -> &'static str {
        match self {
            BinOp::Add => "add",
            BinOp::Sub => "sub",
            BinOp::Mul => "mul",
            BinOp::SDiv => "sdiv",
            BinOp::SRem => "srem",
            BinOp::And => "and",
            BinOp::Or => "or",
            BinOp::Xor => "xor",
            BinOp::Shl => "shl",
            BinOp::AShr => "ashr",
            BinOp::LShr => "lshr",
            BinOp::FAdd => "fadd",
            BinOp::FSub => "fsub",
            BinOp::FMul => "fmul",
            BinOp::FDiv => "fdiv",
            BinOp::FRem => "frem",
        }
    }
}

impl IPred {
    pub fn as_str(self) -> &'static str {
        match self {
            IPred::Eq => "eq",
            IPred::Ne => "ne",
            IPred::Slt => "slt",
            IPred::Sgt => "sgt",
            IPred::Sle => "sle",
            IPred::Sge => "sge",
        }
    }
}

impl FPred {
    pub fn as_str(self) -> &'static str {
        match self {
            FPred::Oeq => "oeq",
            FPred::One => "one",
            FPred::Olt => "olt",
            FPred::Ogt => "ogt",
            FPred::Ole => "ole",
            FPred::Oge => "oge",
            FPred::Une => "une",
        }
    }
}

// `Operand` carries an `f64` constant variant; the auto-derived
// `PartialEq` / `Hash` would either reject f64 entirely (Eq) or
// produce hashes that don't agree with equality (since `NaN != NaN`
// in IEEE 754). The egraph GVN map needs both `Eq` and a stable
// `Hash` so duplicate-constant detection collapses two `ConstF64(x)`
// uses into one e-class entry even when `x.is_nan()`.
//
// Treatment is the standard SSA-optimizer approach (Cranelift's
// `cranelift/codegen/src/ir/immediates.rs` does the same): compare
// and hash the IEEE 754 bit pattern via `f64::to_bits()`. Two NaNs
// with identical bit patterns are equal; +0.0 and -0.0 have distinct
// bit patterns and so hash distinctly, matching the conservative
// behaviour GVN needs (sign of zero is observable in JS via `1/x`).
impl PartialEq for Operand {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Operand::Value(a), Operand::Value(b)) => a == b,
            (Operand::ConstI64(a), Operand::ConstI64(b)) => a == b,
            (Operand::ConstI32(a), Operand::ConstI32(b)) => a == b,
            (Operand::ConstF64(a), Operand::ConstF64(b)) => a.to_bits() == b.to_bits(),
            (Operand::ConstBool(a), Operand::ConstBool(b)) => a == b,
            (Operand::ConstPtrNull, Operand::ConstPtrNull) => true,
            _ => false,
        }
    }
}

impl Eq for Operand {}

impl Hash for Operand {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Mix the discriminant first so two different variants holding
        // the same bit pattern (e.g. ConstI64(0) and ConstPtrNull) hash
        // distinctly.
        std::mem::discriminant(self).hash(state);
        match self {
            Operand::Value(v) => v.hash(state),
            Operand::ConstI64(x) => x.hash(state),
            Operand::ConstI32(x) => x.hash(state),
            Operand::ConstF64(x) => x.to_bits().hash(state),
            Operand::ConstBool(b) => b.hash(state),
            Operand::ConstPtrNull => {}
        }
    }
}
