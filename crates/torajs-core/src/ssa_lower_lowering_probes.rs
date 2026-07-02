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
        !matches!(
            self.ast.get_expr(eid),
            Expr::Ident(_)
                | Expr::Member { .. }
                | Expr::Index { .. }
                | Expr::OptChain { .. }
                | Expr::This
                | Expr::String(_)
        )
    }

    /// T-10.c (v0.4.0) — cheap AST-shape probe for Array literal
    /// heterogeneity. Returns true iff the literal mixes DIFFERENT
    /// static-known kinds (Number vs String vs Bool vs Null among
    /// LITERAL elements only). Non-literal elements (Identifier,
    /// Call, Member, BinOp, ...) are treated as "kind unknown" and
    /// don't trigger the Any path — those route through the regular
    /// homogeneous codegen which already understands them. This
    /// means `[1, 'a', true]` → Any, but `[1, x, 3]` (where x is an
    /// `i64` ident) → regular Array<I64>. Matching the operand types
    /// of mixed expressions to the Any path is T-10.d work.
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
                _ => None, // unknown kind — fall back to homogeneous path
            }
        }
        let mut anchor: Option<u8> = None;
        for &eid in ids {
            if let Some(k) = classify(self.ast, eid) {
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
