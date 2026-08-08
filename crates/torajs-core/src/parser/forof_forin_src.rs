//! for-in head source desugar — §14.7.5 (chunk B2/B3), split
//! verbatim from `try_parse_for_of.rs` (rotation 341 file-size:
//! the knife-3 using head pushed the host over the 500 line).

use super::*;

impl<'a> Parser<'a> {
    /// chunk B2 — for-in keys source: `Object.__forinKeys(raw_src)`,
    /// the parser-synthesized twin of `Object.keys` whose Any arm
    /// enumerates nothing on a null / undefined receiver (ES §14.7.5
    /// ForIn/OfHeadEvaluation step 3 short-circuits before ToObject)
    /// instead of throwing.
    pub(super) fn wrap_forin_keys_src(&mut self, raw_src: ExprId) -> ExprId {
        let object_id = self.ast.add_expr(Expr::Ident("Object".into()));
        let keys_member = self.ast.add_expr(Expr::Member {
            obj: object_id,
            name: "__forinKeys".into(),
        });
        self.ast.add_expr(Expr::Call {
            callee: keys_member,
            args: vec![raw_src],
        })
    }

    /// chunk B2/B3 — for-in source desugar. Hoists the head object
    /// ONCE to a fresh binding (`let __forin_obj_N = raw_src`) —
    /// §14.7.5 evaluates the head expression once, so the keys call
    /// and the per-iter mid-loop-delete guard must keep reading that
    /// snapshot even when the user reassigns the source binding
    /// inside the body — then wraps `Object.__forinKeys` over the
    /// hoisted binding so the body walks a `string[]`. Returns
    /// `(keys_src, obj Ident ExprId, hoist stmt)`.
    pub(super) fn make_forin_src(&mut self, raw_src: ExprId) -> (ExprId, ExprId, Stmt) {
        let id = self.mint_desugar_id();
        let name = format!("__forin_obj_{id}");
        let hoist = Stmt::LetDecl {
            mutable: false,
            name: name.clone(),
            type_ann: None,
            init: raw_src,
            is_var: false,
        };
        let obj_eid = self.ast.add_expr(Expr::Ident(name));
        (self.wrap_forin_keys_src(obj_eid), obj_eid, hoist)
    }
}
