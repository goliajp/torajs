//! The promise value-point width query: which width-table keys a
//! promise-typed expression's settled value is filed under, and the
//! awaited value's SSA type widened by them. The await, resolve and
//! combinator sites share it.

use crate::ast::{Expr, ExprId};
use crate::ssa::Type;

impl crate::ssa_lower::LowerCtx<'_> {
    /// ②.6b — candidate slot keys for a promise-typed `obj` expr,
    /// mirroring the analysis-side container_key_lookup shallow
    /// shapes: an Ident yields its slot keys (Local/Global and, in a
    /// named fn, the Param spelling — only the one the analysis
    /// populated answers); a direct call of a known fn the fn's Ret
    /// key (the analysis keys `await presolved(x)` through
    /// `Ret(presolved)`, NOT an Anon — the original Anon spelling
    /// read narrow and the let-store sitofp'd raw f64 bits); an
    /// Index projection (`await ps[i]`, r4) the Elem of every base
    /// candidate; anything else its Anon origin.
    /// The key the analysis filed a read of `n` from this fn under —
    /// `WidthTable::ident_keys`, the walk's own `resolve` replayed off
    /// its slot-name registry (param, else local, else the top-level
    /// binding, else a lifted closure's `Captured` broadcast). The
    /// lowering's slot table cannot answer this: a captured top-level
    /// `const h = async (n) => n + 0.5` sits in `locals` at prologue
    /// depth like a param, and asking `Local` / `Param` for it joined
    /// nothing — `await h(4)` read its f64 bits from a named async fn
    /// and from a lifted closure alike (rotation 642).
    fn promise_ident_keys(&self, n: &str) -> Vec<crate::num_width::SlotKey> {
        let fn_name = if self.is_main_fn {
            ""
        } else {
            self.f.name.as_str()
        };
        self.num_f64_slots.ident_keys(fn_name, n)
    }

    pub(crate) fn promise_obj_keys(&self, obj: ExprId) -> Vec<crate::num_width::SlotKey> {
        use crate::num_width::SlotKey;
        match self.ast.get_expr(obj) {
            Expr::Ident(n) => self.promise_ident_keys(n),
            Expr::Call { callee, .. } => {
                let fname = self.call_retargets.get(&obj).cloned().or_else(|| {
                    match self.ast.get_expr(*callee) {
                        Expr::Ident(n) if self.fn_table.contains_key(n) => Some(n.clone()),
                        _ => None,
                    }
                });
                if let Some(f) = fname {
                    return vec![SlotKey::Ret(f)];
                }
                // A call through an fn-VALUED name (`const h = async (n)
                // => …; await h(4)`): the analysis keys that result by
                // the value's `__ret` projection (F1 —
                // `container_key_lookup`'s indirect arm), which the
                // binding's init glued onto the lifted closure's Ret.
                // Keyed by its Anon origin instead, the read joined
                // nothing: a fractional `Promise<number>` came back as
                // its f64 bits, and a struct payload read at the parse
                // default while the closure had widened (rotation 642).
                // A Member callee keeps the Anon spelling — the analysis
                // routes those through its method table.
                if let Expr::Ident(n) = self.ast.get_expr(*callee) {
                    return self
                        .promise_ident_keys(n)
                        .into_iter()
                        .map(|k| SlotKey::Field(Box::new(k), "__ret".into()))
                        .collect();
                }
                vec![SlotKey::Anon(obj.0)]
            }
            Expr::Index { obj: base, .. } | Expr::OptIndex { obj: base, .. } => self
                .promise_obj_keys(*base)
                .into_iter()
                .map(|k| SlotKey::Elem(Box::new(k)))
                .collect(),
            // a field the program also stores into (`o.p = h`)
            Expr::Member { obj: base, name } => (self.promise_obj_keys(*base).into_iter())
                .map(|k| SlotKey::Field(Box::new(k), name.as_str().into()))
                .chain([SlotKey::Anon(obj.0)])
                .collect(),
            // the value the analysis keys these by (`container_key_of`)
            Expr::As { expr: e, .. } | Expr::Sequence { right: e, .. } => self.promise_obj_keys(*e),
            Expr::Assign { value, .. } => self.promise_obj_keys(*value),
            _ => vec![SlotKey::Anon(obj.0)],
        }
    }

    /// [`Self::promise_obj_keys`] of a read's operand: the keys the
    /// analysis must have read the same value point through.
    fn promise_read_keys(&self, obj: ExprId) -> Vec<crate::num_width::SlotKey> {
        let keys = self.promise_obj_keys(obj);
        #[cfg(torajs_checks)]
        assert!(
            self.num_f64_slots.await_point_agrees(obj, &keys),
            "await of expr {} in {}: lowering reads {keys:?}, not the value point the analysis read",
            obj.0,
            self.f.name,
        );
        keys
    }

    /// ②.6b — is the promise value point of `obj`'s expr f64? The
    /// `value` projection matches the parser's `await p` → `p.value`
    /// desugar spelling.
    pub(crate) fn promise_value_is_f64(&self, obj: ExprId) -> bool {
        self.promise_read_keys(obj)
            .iter()
            .any(|k| self.num_f64_slots.promise_value_is_f64(k))
    }

    /// ②.6b — width-face decode for the awaited value's SSA type.
    /// `Promise<number>` parses to the I64 default; when the value
    /// class floated, the slot carries f64 BITS and the await read
    /// must decode them (the caller's BitCastI64ToF64 arm).
    /// `Promise<number[]>` (Promise.all, r3) parses to the Arr(I64)
    /// default the same way: the combinator fills the result array
    /// from each source promise's raw value slot, so the element
    /// face must take the width table's value-point class, queried
    /// through the same candidate keys as the scalar flip.
    pub(crate) fn widen_promise_inner_ty(
        &mut self,
        inner_ssa_ty: Option<Type>,
        obj: ExprId,
    ) -> Option<Type> {
        use crate::num_width::SlotKey;
        match inner_ssa_ty {
            Some(Type::I64) if self.promise_value_is_f64(obj) => Some(Type::F64),
            Some(t @ Type::Arr(_)) => {
                let mut t = t;
                for k in self.promise_read_keys(obj) {
                    let pv = SlotKey::Field(Box::new(k), "value".into());
                    t = crate::ssa_lower_container_width::widen_arr_elem(
                        t,
                        None,
                        &pv,
                        self.num_f64_slots,
                        self.arr_layouts,
                        self.struct_layouts,
                        self.fn_sigs,
                    );
                }
                Some(t)
            }
            // A struct payload's faces are the struct's own fields.
            // Without this arm an async generator's `{value, done}`
            // step read back at the parse default while the literal
            // that built it had widened, so an integer yield came out
            // as its own f64 bit pattern (552-04). The sync lane never
            // showed it: its step struct arrives as a plain declared
            // return type and takes the ordinary width injection.
            //
            // The payload lives at the promise's VALUE point — the
            // key the Arr arm above already projects — not at the
            // promise key itself. Asked at `k`, the struct's `value`
            // field spelled the same key as the value point, so a
            // `{value, done}` step answered from the point instead of
            // from the field: F64 only while the point happened to
            // carry the literal that built it, and I64 once the
            // literal bound to a typed local first (rotation 642),
            // while the parse here — the checker's structural type,
            // alias name lost — never sees the nominal `__step_<gen>`
            // layout the writer widened.
            Some(t @ Type::Obj(_)) => {
                let mut t = t;
                for k in self.promise_read_keys(obj) {
                    let pv = SlotKey::Field(Box::new(k), "value".into());
                    t = crate::ssa_lower_container_width::widen_struct_fields(
                        t,
                        &pv,
                        self.num_f64_slots,
                        self.arr_layouts,
                        self.struct_layouts,
                        self.fn_sigs,
                    );
                }
                Some(t)
            }
            other => other,
        }
    }
}
