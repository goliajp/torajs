//! Lowering-strategy probe helpers for `LowerCtx<'a>` extracted from
//! `ssa_lower.rs` chunk 385 — Path A.3-batch6.
//!
//! Three small "peek-and-decide" helpers used at lowering entry points
//! to pick between fast-path and regular codegen:
//!
//! - `try_resolve_type_ann(ann)` — M6.3: parse a type annotation via
//!   `parse_type` and normalize `Type::Void` to `None`. Lets LetDecl
//!   fast-paths (JSON.parse, fromEntries, Bun.file(p).json()) skip to
//!   the regular flow when the slot has no usable type info.
//! - `expr_is_fresh_owned(eid)` — decide whether an expression's lowered
//!   Operand represents a freshly-allocated owned value the caller must
//!   drop. False for borrow-shaped exprs (Ident / Member / Index /
//!   OptChain / This) and for `String` literals (rc-noop via
//!   STATIC_LITERAL). Used by Expr::BinOp's post-call drop pass.
//! - `array_literal_is_heterogeneous(ids)` — T-10.c: cheap AST-shape
//!   probe returning true iff the literal mixes DIFFERENT static-known
//!   kinds (Number / String / Bool / Null / nested Array / negated
//!   Number). Non-literal elements count as "kind unknown" and don't
//!   trigger the Any path.
//!
//! Method bodies are byte-for-byte preserved from the source; the
//! sibling reaches LowerCtx fields via `impl<'a> super::LowerCtx<'a>`,
//! so call sites need zero edits.

use crate::ast::{Ast, Expr, ExprId};
use crate::ssa::Type;
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_parse_type::parse_type;

impl<'a> LowerCtx<'a> {
    /// M6.3 — wrapper around `parse_type` that returns `None` when the
    /// annotation is missing or doesn't resolve to a concrete Type
    /// the JSON parser knows how to handle. Lets the LetDecl fast-
    /// path skip to the regular flow when the slot has no usable
    /// type info.
    pub(crate) fn try_resolve_type_ann(&mut self, ann: Option<&str>) -> Option<Type> {
        let ann = ann?;
        let ty = parse_type(
            Some(ann),
            self.aliases,
            self.arr_layouts,
            self.fn_sigs,
            self.generic_struct_decls,
            self.struct_layouts,
            self.inst_memo,
        );
        if matches!(ty, Type::Void) {
            return None;
        }
        Some(ty)
    }

    /// True when an expression's lowered Operand represents a freshly-
    /// allocated owned value the surrounding lowering site must drop.
    /// False for borrow-shaped exprs (Ident / Member / Index / OptChain
    /// / This — source binding owns the heap) and for string literals
    /// (`Expr::String(_)`: post-P-rpn lowers to `StaticStrRef`, rc-noop
    /// via STATIC_LITERAL; emitting `__torajs_str_drop`'s BL still
    /// clobbers caller-saved X0 and silently destroyed `n + "x"`-style
    /// ret values). Used by Expr::BinOp's post-call drop pass.
    pub(crate) fn expr_is_fresh_owned(&self, eid: ExprId) -> bool {
        // Peel value-transparent `As` wrappers before judging: the
        // cast is IDENTITY for an Any source, a fresh IMMEDIATE box
        // for a primitive (whose drop is a payload-gated no-op), and
        // a bare pass-through for a heap source — in all three the
        // INNER read decides who owns the heap. `ssa_lower_object_lit
        // ::lower_regular_field` peels for exactly this reason.
        // Unpeeled, an `Ident as any` answered fresh-owned and the
        // BinOp drop pass released the binding's only stake:
        // `a === (b as any)` over two symbols freed `b` before its
        // scope drop, and the scope drop then read the dead header
        // (rotation 384; `symbol-wrapper-object-coerce-001.ts` was
        // the fixture, green on the gate the whole time).
        let mut eid = eid;
        while let Expr::As { expr, .. } = self.ast.get_expr(eid) {
            eid = *expr;
        }
        // Chunk 637 — a Member read whose owned-receiver lowering
        // detached the result (see `ssa_lower_member::lower`) IS
        // fresh-owned; every other Member stays a borrow. Chunk 717
        // — the any-member lanes (literal-key Index, OptChain /
        // OptIndex hit paths, Closure expando reads) answer owned on
        // every arm and record their eid the same way; unrecorded
        // reads of these shapes stay borrows.
        if matches!(
            self.ast.get_expr(eid),
            Expr::Member { .. }
                | Expr::Index { .. }
                | Expr::OptChain { .. }
                | Expr::OptIndex { .. }
        ) {
            return self.owned_member_reads.contains(&eid);
        }
        !matches!(
            self.ast.get_expr(eid),
            Expr::Ident(_) | Expr::This | Expr::String(_)
        )
    }

    /// T-10.c (v0.4.0) — probe for Array literal heterogeneity.
    /// Returns true iff the literal mixes DIFFERENT static-known
    /// kinds. Literal elements classify by AST shape (Number vs
    /// String vs Bool vs Null vs nested Array vs negated Number);
    /// non-literal elements (Identifier, Call, New, ObjectLit,
    /// Member, ...) classify by their checker type (T-10.d close,
    /// 2026-07-05): pre-fix they were skipped entirely, so
    /// `[{ k: 1 }, 2]` anchored on the number and lowered as a
    /// typed 8-byte-slot array — the obj pointer and the int shared
    /// one slot interpretation and every downstream reader
    /// (index, print, drop) silently mis-decoded (null / bare
    /// pointer digits / 2e-314 / scope-drop SIGSEGV). Elements the
    /// checker can't pin either (`any`, unions, missing entries)
    /// still fall back to the homogeneous path: `[1, x, 3]` with
    /// `x: number` stays a typed Array<I64>.
    pub(crate) fn array_literal_is_heterogeneous(&self, ids: &[ExprId]) -> bool {
        // Recursive — `Unary{Neg, Number(...)}` like `-3.14` keeps the
        // inner Number's kind so `[-3.14, 'x']` correctly flags as
        // heterogeneous (F64-kind vs Str-kind). Same for `+x` /
        // `~bits` if those ever appear inside an Array literal.
        fn classify(ast: &Ast, eid: ExprId) -> Option<u8> {
            match ast.get_expr(eid) {
                // W4 — int and fract literals share the number kind:
                // `[2, 1.5]` is a typed F64-elem array (the width
                // analysis seeds the literal's elem class and the
                // ArrayLit lowering coerces the int members), not an
                // Array<Any>. check.rs agrees (both are TS Number).
                Expr::Number(_) => Some(1),
                Expr::String(_) => Some(3),
                Expr::Bool(_) => Some(4),
                Expr::Null => Some(5),
                // S129-3 — nested Array literal counts as its own
                // kind so `[[1,2], 6]` (array + scalar) classifies
                // as heterogeneous → Array<Any> codegen. Pre-fix
                // nested arrays returned None, leaving the anchor
                // pinned to the scalar's kind; the array slots then
                // got raw-stored as i64 ptrs into a typed Array<T>,
                // breaking arr_flat_any's NaN-box decode. Homogeneous
                // nested literals (`[[1,2],[3,4]]`) still anchor to
                // the same kind = 2 → typed Array<Array<T>>.
                Expr::Array(_) => Some(2),
                Expr::Unary { expr, .. } => classify(ast, *expr),
                _ => None, // no syntactic kind — checker type below
            }
        }
        // Checker-type classes. Scalar/array kinds share the
        // syntactic values so `[1, x, 3]` (x: number) stays
        // homogeneous; heap kinds get distinct values so any
        // cross-kind mix routes to the Array<Any> path.
        fn classify_checked(t: &crate::check::Type) -> Option<u8> {
            use crate::check::Type as C;
            match t {
                C::Number => Some(1),
                C::Array(_) => Some(2),
                C::String => Some(3),
                C::Boolean => Some(4),
                C::Null => Some(5),
                C::Struct(_) | C::ClassRef(_) | C::Object(_) => Some(10),
                C::Map => Some(11),
                C::Set => Some(12),
                C::Function(..) => Some(13),
                C::BigInt => Some(14),
                C::WeakRef => Some(15),
                C::WeakMap => Some(16),
                C::WeakSet => Some(17),
                C::RegExp => Some(18),
                C::Date => Some(19),
                C::Symbol => Some(20),
                C::Promise(_) => Some(21),
                C::MapIter => Some(22),
                C::ArrIter => Some(23),
                // Any / unions / Nullable / TypeVar / Undefined /
                // missing — unknown, keep the homogeneous fallback.
                _ => None,
            }
        }
        let mut anchor: Option<u8> = None;
        for &eid in ids {
            let k = classify(self.ast, eid)
                .or_else(|| self.expr_types.get(&eid).and_then(classify_checked));
            if let Some(k) = k {
                match anchor {
                    None => anchor = Some(k),
                    Some(a) if a != k => return true,
                    _ => {}
                }
            }
        }
        false
    }
}

impl<'a> LowerCtx<'a> {
    /// Holes X+Y (rotation 231) — do the recorded heap-element types
    /// disagree in a way `array_literal_is_heterogeneous`'s kind
    /// probe cannot see? Struct-family (Struct / ClassRef) elements
    /// disagree on any recorded-type inequality (distinct layouts);
    /// array-family elements disagree on any inner-type inequality
    /// (rotation 260 — Any-ness alone missed `[[1,2], ["a","b"]]`:
    /// Array(Number) vs Array(String) share neither slot repr nor
    /// elem-kind chain, so the typed lane raw-read the Str column's
    /// pointers as I64). Elements with no recorded type
    /// (mono-specialized clones) contribute nothing — the
    /// pre-existing lanes keep them.
    pub(crate) fn heap_elem_types_disagree(&self, ids: &[ExprId]) -> bool {
        use crate::check::Type as C;
        let mut struct_anchor: Option<&C> = None;
        let mut ary_anchor: Option<&C> = None;
        for id in ids {
            match self.expr_types.get(id) {
                Some(t @ (C::Struct(_) | C::ClassRef(_))) => match struct_anchor {
                    None => struct_anchor = Some(t),
                    Some(a) if a != t => return true,
                    _ => {}
                },
                Some(C::Array(inner)) => match ary_anchor {
                    None => ary_anchor = Some(inner),
                    Some(a) if a != &**inner => return true,
                    _ => {}
                },
                _ => {}
            }
        }
        false
    }
}
