//! Stringify + helper methods on `Type` / `BinOp` / `IPred` / `FPred`.
//!
//! These are small `pub fn as_str(self) -> &'static str` + a few
//! query predicates (`is_copy`, `is_refcounted`, `is_pointer_shaped`
//! on Type) — all data-driven match arms over the enum variant.
//!
//! Extracted from `ssa.rs` (2026-05-25, god-file decomp batch 15).

use super::{BinOp, FPred, IPred, Type};

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

    /// V3-05 — true if the SSA value is an i64-wide pointer slot
    /// (heap-owned refcounted types + raw Ptr + bare Promise / Symbol /
    /// any other heap handle). Used by ObjectLit's permissive layout
    /// match so a literal `null` Ptr field maps onto a registered
    /// pointer-shaped class field of any specific tag.
    pub fn is_pointer_shaped(self) -> bool {
        self.is_refcounted() || matches!(self, Type::Ptr)
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
