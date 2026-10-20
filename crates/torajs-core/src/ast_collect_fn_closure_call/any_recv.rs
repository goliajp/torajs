//! The receiver axis of the call walk: a top-FnDecl argument of a
//! member call whose receiver the walk cannot type, and the one
//! receiver shape among those whose method it can still name, a chain
//! call over a `Promise` static.

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
        let Expr::Member { obj, name } = self.ast.get_expr(*callee) else {
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
            return self.mark_typed_recv_reaction_args(name, args);
        }
        let raw_chain_site = matches!(name.as_str(), "then" | "catch")
            && args.len() == 1
            && self.promise_chain(*obj);
        // r292 — generator / async-generator factory idents wrap too:
        // the canonical forward cell carries the G2 reflection faces
        // (gen-proto install at mint + FLAG_FN_GENERATOR), so the
        // any-method lane can box them (`GeneratorPrototype.next
        // .call(g)` — the this-val-not-generator family).
        for &arg in args {
            if raw_chain_site && self.raw_reaction_handler(arg) {
                self.target_fn_args(&[arg]);
            } else {
                self.try_mark(arg);
            }
        }
    }

    /// A typed ident receiver keeps its function arguments raw, but a
    /// `.then` / `.catch` handler reaches the reaction kernels, which
    /// call a raw handler with exactly one argument: one that declares
    /// more parameters (`(a, b?)`) would read a slot nobody passed, so
    /// it takes the forwarder cell the kernels call with the full
    /// argument list, as on an untyped chain.
    fn mark_typed_recv_reaction_args(&mut self, name: &str, args: &[ExprId]) {
        if !matches!(name, "then" | "catch") {
            return self.target_fn_args(args);
        }
        for &arg in args {
            if self.raw_reaction_handler(arg) {
                self.target_fn_args(&[arg]);
            } else {
                self.try_mark(arg);
            }
        }
    }

    /// `Promise.resolve(x)` and the other statics, or a `.then` /
    /// `.catch` / `.finally` call over one: the lowering answers these
    /// receivers as built-in promises by their shape alone, so a
    /// handler they take reaches the typed chain kernels, not the
    /// any-method lane. A program that writes the static or the
    /// prototype method sends the call down that lane, which boxes its
    /// arguments, so the handler keeps its cell there.
    fn promise_chain(&self, recv: ExprId) -> bool {
        let Expr::Call { callee, .. } = self.ast.get_expr(recv) else {
            return false;
        };
        let Expr::Member { obj, name } = self.ast.get_expr(*callee) else {
            return false;
        };
        match name.as_str() {
            "then" | "catch" | "finally" => {
                !self.proto_shadow.shadows("Promise", name) && self.promise_chain(*obj)
            }
            "resolve" | "reject" | "all" | "race" | "any" | "allSettled" => {
                matches!(self.ast.get_expr(*obj), Expr::Ident(ns) if ns == "Promise")
                    && !self.proto_shadow.writes_static("Promise", name)
                    && !self.fn_sigs.contains_key("Promise")
                    && !self.shadowed.iter().any(|s| s.contains("Promise"))
            }
            _ => false,
        }
    }

    /// A top-level function the reaction kernels can call through its
    /// bare address. §27.2.2.1 NewPromiseReactionJob calls the handler
    /// with `this` undefined and exactly one argument, and a raw call
    /// passes that one argument and nothing else: so at most one
    /// parameter, neither rest nor defaulted (both need the call site
    /// to fill in what it did not pass). A body reading `this` or
    /// `arguments`, and an untyped parameter, are wrapped by their own
    /// axes of the same call walk, and the wrap stands.
    fn raw_reaction_handler(&self, eid: ExprId) -> bool {
        let Expr::Ident(f) = self.ast.get_expr(eid) else {
            return false;
        };
        if !self.names_plain_fn(eid) || self.shadowed.iter().any(|s| s.contains(f)) {
            return false;
        }
        self.fn_sigs.get(f).is_some_and(|(params, _, _)| {
            params.len() <= 1 && params.iter().all(|p| !p.is_rest && p.default.is_none())
        })
    }
}
