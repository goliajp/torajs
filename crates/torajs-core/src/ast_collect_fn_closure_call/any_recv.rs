//! The receiver axis of the call walk: a top-FnDecl argument of a
//! member call whose receiver the walk cannot type.

use crate::ast::{Expr, ExprId};
use crate::ast_collect_fn_closure::FnToClosureCollector;

impl FnToClosureCollector<'_> {
    /// RFC 20260729-fn-value-any V1 — a user top-FnDecl Ident
    /// argument of a member call whose receiver cannot be statically
    /// typed: an `any`-bound ident receiver (`p.then(done)`, p: any)
    /// or a chained expression receiver (`ref(3).next().then($DONE,
    /// $DONE)` — the t262 async harness tail on every async case,
    /// the whole box_to_any FnSig 720-cluster's shared trigger). The
    /// callee rides the runtime any-method lane, whose argv boxes
    /// into the any world — a raw FnSig has no arm there. A TYPED
    /// ident receiver keeps its args unwrapped so `xs.map(topFn)`
    /// hot paths stay on raw-FnSig direct dispatch, but its callee
    /// may declare a closure parameter, which the lowering fills with
    /// the function's forwarder cell: the forwarder is minted, the
    /// site left as written. A typed chained receiver
    /// (`getArr().map(topFn)`) only costs the wrap — closure values
    /// are legal fn-param arguments.
    pub(super) fn mark_any_recv_member_args(&mut self, callee: &ExprId, args: &[ExprId]) {
        let Expr::Member { obj, .. } = self.ast.get_expr(*callee) else {
            return;
        };
        let typed_ident_recv = matches!(
            self.ast.get_expr(*obj),
            Expr::Ident(n) if !self.any_bindings.contains(n)
                && !self.new_init_bindings.contains(n)
                // r293 — an `any`/untyped param of the enclosing fn
                // (a lifted arrow's `iter => iter.map(fn)`) is an
                // any receiver too.
                && !self.any_param_frames.iter().any(|f| f.contains(n))
        );
        if typed_ident_recv {
            return self.target_fn_args(args);
        }
        // r292 — generator / async-generator factory idents wrap too:
        // the canonical forward cell carries the G2 reflection faces
        // (gen-proto install at mint + FLAG_FN_GENERATOR), so the
        // any-method lane can box them (`GeneratorPrototype.next
        // .call(g)` — the this-val-not-generator family).
        for &arg in args {
            self.try_mark(arg);
        }
    }
}
