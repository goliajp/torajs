//! Equality fast-path pair for `LowerCtx<'a>` extracted from
//! `ssa_lower.rs` chunk 390 — Path A.3-batch11.
//!
//! Two `try_*` methods that lower_binop's Eq/Neq dispatcher consults
//! before falling through to the generic value-equality path:
//!
//! - `try_fold_constructor_eq(op, left, right)` — V3-18 m2.e AST-level
//!   fold of `<prim>.constructor === <Ctor>` /
//!   `<Ctor> === <prim>.constructor`. Recognizes bare `Number` /
//!   `String` / `Boolean` / `BigInt` / `Symbol` / `Object` / `Array`
//!   ctor idents against the receiver's static SSA type and returns
//!   a `ConstBool` result. Bare ctor idents can't be lowered as
//!   values (tora has no first-class fn ref for namespace ctors), so
//!   this pattern must be caught pre-lower.
//! - `try_inline_str_eq_with_literal(op, left, right)` — perf fast
//!   path for `expr === "literal"` / `expr !== "literal"` where the
//!   literal is short (≤16 bytes) + ASCII-only (any byte > 0x7F bails
//!   out to `__torajs_str_eq` for encoding-aware compare). Skips the
//!   C-runtime fn-call by emitting inline byte-by-byte compare via
//!   `emit_inline_str_eq_bytes` (str-byte-helpers sibling).
//!
//! Method bodies preserved byte-for-byte; the sibling reaches LowerCtx
//! fields via `impl<'a> super::LowerCtx<'a>`, so call sites
//! (`ssa_lower_binop.rs:53` and `:60`) need zero edits.

use crate::ast::{BinOp as AstBinOp, Expr, ExprId};
use crate::ssa::{BinOp as SsaBinOp, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

impl<'a> LowerCtx<'a> {
    /// V3-18 m2.e — fold `<prim>.constructor === <Ctor>` /
    /// `<Ctor> === <prim>.constructor` at AST level. Used as a
    /// pre-lower pattern match since tora has no first-class
    /// function ref for namespace ctors (bare `Number` / `String`
    /// etc can't be lowered as a value).
    pub(crate) fn try_fold_constructor_eq(
        &mut self,
        op: AstBinOp,
        left: ExprId,
        right: ExprId,
    ) -> Option<Operand> {
        // Identify a (prim_constructor_member, ctor_ident) pair
        // regardless of which side is which.
        fn prim_type_tag(t: Type) -> Option<&'static str> {
            match t {
                Type::I64 | Type::F64 | Type::I32 => Some("Number"),
                Type::Str | Type::Substr => Some("String"),
                Type::Bool => Some("Boolean"),
                Type::BigInt => Some("BigInt"),
                Type::Symbol => Some("Symbol"),
                Type::Obj(_) => Some("Object"),
                Type::Arr(_) => Some("Array"),
                _ => None,
            }
        }
        let l_expr = self.ast.get_expr(left).clone();
        let r_expr = self.ast.get_expr(right).clone();
        let (member_expr, ctor_name): (Expr, String) = match (l_expr, r_expr) {
            (Expr::Member { obj, name }, Expr::Ident(c)) if name == "constructor" => {
                (self.ast.get_expr(obj).clone(), c)
            }
            (Expr::Ident(c), Expr::Member { obj, name }) if name == "constructor" => {
                (self.ast.get_expr(obj).clone(), c)
            }
            _ => return None,
        };
        // Resolve the receiver's static SSA type.
        let recv_ty = match member_expr {
            Expr::Ident(ref n) => {
                let info = self.locals.get(n)?;
                info.ty
            }
            _ => return None,
        };
        // The fold's whole premise is a NAMESPACE ctor ident (those
        // can't lower as values); a user class name is a real runtime
        // value the generic path compares correctly — bail, don't
        // fold `c.constructor === MyC` to false.
        const NS_CTORS: &[&str] = &[
            "Number", "String", "Boolean", "BigInt", "Symbol", "Object", "Array",
        ];
        if !NS_CTORS.contains(&ctor_name.as_str()) {
            return None;
        }
        // A named-class instance is Obj-typed too, but its
        // `constructor` is the class object, not `Object` — only an
        // anonymous object-literal shape folds.
        if let Type::Obj(sid) = recv_ty
            && self
                .class_name_to_tag
                .keys()
                .any(|cname| matches!(self.aliases.get(cname), Some(Type::Obj(s)) if *s == sid))
        {
            return None;
        }
        let actual = prim_type_tag(recv_ty)?;
        let matches_ctor = actual == ctor_name.as_str();
        let result = match op {
            AstBinOp::Eq => matches_ctor,
            AstBinOp::Neq => !matches_ctor,
            _ => return None,
        };
        Some(Operand::ConstBool(result))
    }

    /// Perf fast-path for `expr === "literal"` / `expr !== "literal"`
    /// where the literal is short (≤16 bytes). Returns `None` if the
    /// pattern doesn't match (caller falls through to the generic
    /// str_eq path). For switch-on-string the equivalent inline emit
    /// happens directly inside `Stmt::Switch` lowering — see there.
    pub(crate) fn try_inline_str_eq_with_literal(
        &mut self,
        op: AstBinOp,
        left: ExprId,
        right: ExprId,
    ) -> Option<Operand> {
        let (lit_bytes, other_eid) = match (
            self.ast.get_expr(left).clone(),
            self.ast.get_expr(right).clone(),
        ) {
            (Expr::String(s), _) => (s.into_bytes(), right),
            (_, Expr::String(s)) => (s.into_bytes(), left),
            _ => return None,
        };
        if lit_bytes.len() > 16 {
            return None;
        }
        // P11.1-S2.3 — the inline `str === <literal>` path
        // compares the runtime Str's `length` field against
        // `lit_bytes.len()` (the literal's UTF-8 byte count) and
        // then walks the runtime Str's payload byte-by-byte
        // against the literal. Post-S2 the runtime Str's
        // `length` is a code unit count, not a byte count: for
        // an ASCII-only Latin-1 payload code unit == byte so
        // both numbers + bytes coincide, but for any non-ASCII
        // codepoint they diverge (UTF-16 Str length is half its
        // byte count; the literal's UTF-8 byte length doesn't
        // match the Latin-1 byte length either). Bail out of
        // the inline arm whenever the literal carries any byte
        // > 0x7F so the runtime `__torajs_str_eq` does the
        // encoding-aware compare instead.
        if lit_bytes.iter().any(|&b| b > 0x7F) {
            return None;
        }
        // RFC 20260707-undefined-sentinel-repr chunk 1 — a
        // nullable-str operand (missed exec/match capture slot may
        // be NULL) must not reach the inline byte walk (raw len
        // load at offset 8 SIGSEGVs). Decline pre-lower so the
        // generic path calls the null-guarded `__torajs_str_eq`
        // (identity compare on NULL → false vs any literal).
        if crate::ssa_lower_nullable_guard::is_nullable_str_source(self, other_eid) {
            return None;
        }
        // RFC 20260707 residual chunk — a string-index operand
        // (`s[i] === "lit"`) may hold the Substr-shaped undefined
        // sentinel, whose view text is "undefined": the inline byte
        // walk would content-match it. Decline to the generic path
        // (identity-aware `substr_eq_str`).
        if crate::ssa_lower_nullable_guard::is_undefable_substr_source(self, other_eid) {
            return None;
        }
        let other = self.lower_expr(other_eid);
        let other_ty = self.operand_ty(&other);
        if other_ty != Type::Str && other_ty != Type::Substr {
            // RFC 20260705 chunk 559 — decline-after-lower: the
            // generic binop path re-lowers both sides, so park the
            // emission for the take-once reuse in lower_expr
            // (`step() === "foo"` with a non-Str step() must
            // evaluate step() exactly once; mirror of chunk 555's
            // receiver hint).
            self.redispatch_lowered = Some((other_eid, other));
            return None;
        }
        let r = self.emit_inline_str_eq_bytes(other, &lit_bytes);
        // Chunk 750 — mirror of lower_binop's fresh-owned drop dance:
        // this early-return path skipped it, stranding one fresh cell
        // per compare (`round() === "lit"` leaked its call result —
        // 15.9MB/300k churn). The byte walk only borrows; a fresh-
        // owned operand's ownership ends here. Ident / Member / Index
        // sources stay with their binding.
        if other_ty.is_refcounted() && self.expr_is_fresh_owned(other_eid) {
            self.emit_drop_value(other.clone(), other_ty);
        }
        // For !==, flip via xor.
        if matches!(op, AstBinOp::Neq) {
            let r_v = match r {
                Operand::Value(v) => v,
                _ => unreachable!(),
            };
            let n = self.f.append_inst(
                self.cur_block,
                InstKind::BinOp(SsaBinOp::Xor, Operand::Value(r_v), Operand::ConstBool(true)),
                Type::Bool,
                None,
            );
            Some(Operand::Value(n))
        } else {
            Some(r)
        }
    }
}
