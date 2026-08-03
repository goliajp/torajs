//! `FnToClosureCollector::walk_expr` — the expression half of the
//! store-site walk, carved out of `ast_collect_fn_closure.rs` when
//! the rotation-128 shadow stack pushed that file past the 500-line
//! hard limit (the stmt half + collector struct stay there; module
//! doc lists every axis). The `Expr::Call` arm lives in
//! `ast_collect_fn_closure_call.rs` since rotation 291.

use crate::ast::{Ast, BinOp, Expr, ExprId};
use crate::ast_collect_fn_closure::{FnToClosureCollector, is_fn_like_field_ann};

impl<'a> FnToClosureCollector<'a> {
    /// Generator / async-generator factory fns carry their own
    /// fn-value reflection substrate (RFC 20260713 blade 5:
    /// `generator_factory_classes` wires `.prototype` /
    /// getPrototypeOf / instanceof to the `__Gen_*` class) —
    /// wrapping one in a plain `__forward_*` closure severs the
    /// %GeneratorFunction% / %AsyncGeneratorFunction% intrinsic
    /// chain (gate generator-fn-intrinsics-001 caught exactly
    /// that), so the cluster-#4 axes skip them.
    pub(crate) fn is_generator_family_ident(&self, eid: ExprId) -> bool {
        matches!(self.ast.get_expr(eid), Expr::Ident(n)
            if self.ast.generator_factory_classes.contains_key(n)
                || self.ast.async_generator_fns.contains(n))
    }

    /// A top-FnDecl Ident whose sig gained the hidden `__this` first
    /// param from bind_this_param — its raw FnSig arity no longer
    /// matches the user-visible one, so value uses must wrap.
    pub(crate) fn is_this_promoted_ident(&self, eid: ExprId) -> bool {
        matches!(self.ast.get_expr(eid), Expr::Ident(n)
            if self.fn_sigs.get(n).is_some_and(
                |(params, _, _)| params.first().is_some_and(|p| p.name == "__this")))
    }

    /// A top-FnDecl Ident with un-annotated params — the later
    /// `desugar_implicit_generics` pass turns those into `__T<N>`
    /// TypeVars, so the fn has no concrete raw-FnSig instance a value
    /// use could carry. Its forwarder does: the shim's params inherit
    /// the un-annotated shape, the closure-shape arm defaults them to
    /// `any`, and the forwarding DIRECT call is a mono site that
    /// instantiates the generic at all-any.
    pub(crate) fn is_untyped_plain_fn_ident(&self, eid: ExprId) -> bool {
        matches!(self.ast.get_expr(eid), Expr::Ident(n)
            if self.fn_sigs.get(n).is_some_and(
                |(params, _, _)| params.iter().any(|p| p.type_ann.is_none() && !p.is_rest)))
    }

    /// `Expr::Assign` arm.
    fn walk_assign(&mut self, target: &ExprId, value: &ExprId) {
        // L3b #8 — `f = name` where `f` was declared `any`;
        // RFC 20260709 chunk 4 — `cb = name` where `cb` is a
        // Closure-repr top-level binding.
        if let Expr::Ident(tname) = self.ast.get_expr(*target)
            && (self.any_bindings.contains(tname) || self.closure_bindings.contains(tname))
        {
            self.try_mark(*value);
        }
        // Chunk 733 — `fns[i] = top_fn` where `fns` was
        // declared with a fn-typed array ann (Closure-repr
        // element slot).
        if let Expr::Index { obj, .. } = self.ast.get_expr(*target)
            && let Expr::Ident(oname) = self.ast.get_expr(*obj)
            && self.fn_arr_bindings.contains(oname)
        {
            self.try_mark(*value);
        }
        // Chunk 783 — `o.cb = top_fn` where `o` was declared
        // with a struct type whose field `cb` is fn-typed
        // (Closure-repr slot after the retag pass): the bare
        // named-fn RHS wraps, same as the objlit-field
        // store-site above. Without it the assign lane stores
        // a raw FnSig and the narrowed call CallIndirects
        // into the fn body as if it were an env block
        // (SIGBUS). Chunk 789 — the receiver resolves through
        // Member chains too (`h.o.cb = top_fn`).
        if let Expr::Member { obj, name: fname } = self.ast.get_expr(*target)
            && let Some(field_anns) = self.resolve_receiver_fields(*obj)
            && field_anns
                .get(fname)
                .is_some_and(|a| is_fn_like_field_ann(a))
        {
            self.try_mark(*value);
        }
        self.walk_expr(*target);
        self.walk_expr(*value);
    }

    /// `Expr::BinOp` arm.
    fn walk_binop(&mut self, op: &BinOp, left: &ExprId, right: &ExprId) {
        // Eq-operand axis (RFC 20260717-namedfn-canonical-cell
        // chunk 2) — a bare top-FnDecl Ident compared with
        // ==/===/!=/!== is a VALUE use of the fn object, so it
        // must answer the canonical singleton cell (pre-fix it
        // lowered to the raw FnSig code address and
        // `t === getX` was always false against the cell every
        // other value site answers). Non-eq operators keep the
        // plain recursion — no fn belongs in arithmetic.
        if matches!(
            op,
            crate::ast::BinOp::Eq
                | crate::ast::BinOp::Neq
                | crate::ast::BinOp::LooseEq
                | crate::ast::BinOp::LooseNeq
        ) {
            // `as` layers are value pass-throughs — strip so
            // `(getX as any) === getX` marks the inner Ident
            // (the rewrite lands there; the As then forwards
            // the canonical cell unchanged).
            let strip_as = |ast: &Ast, mut e: ExprId| {
                while let Expr::As { expr, .. } = ast.get_expr(e) {
                    e = *expr;
                }
                e
            };
            let l = strip_as(self.ast, *left);
            if !self.try_mark(l) {
                self.walk_expr(*left);
            }
            let r = strip_as(self.ast, *right);
            if !self.try_mark(r) {
                self.walk_expr(*right);
            }
        } else {
            self.walk_expr(*left);
            self.walk_expr(*right);
        }
    }

    pub(crate) fn walk_expr(&mut self, eid: ExprId) {
        match self.ast.get_expr(eid) {
            Expr::Call { callee, args } => {
                let (callee, args) = (*callee, args.clone());
                self.walk_call(&callee, &args);
            }
            Expr::Member { obj, name } => {
                // G9 (rotation 178) — the member-BASE axis, prototype
                // face only: `decl.prototype` on the FnSig lane is a
                // raw vaddr with no cell identity, so the read must
                // ride the canonical `__fncell_` closure singleton
                // (expando writes on it survive; `a.prototype` through
                // an any binding sees the same cell). name/length keep
                // their static reflection arms — the base stays raw.
                // Generator factories and async forms stay raw too:
                // their `.prototype` rides dedicated static arms
                // (`__proto___Gen_<name>` / no own prototype) that
                // only fire on a bare Ident base.
                let (obj, name) = (*obj, name.clone());
                let plain_fn_base = matches!(self.ast.get_expr(obj), Expr::Ident(n)
                    if !self.ast.generator_factory_classes.contains_key(n)
                        && !self.ast.async_fns.contains(n)
                        && !self.ast.async_generator_fns.contains(n));
                if name != "prototype" || !plain_fn_base || !self.try_mark(obj) {
                    self.walk_expr(obj);
                }
            }
            Expr::OptChain { obj, .. } => self.walk_expr(*obj),
            Expr::OptIndex { obj, index } => {
                self.walk_expr(*obj);
                self.walk_expr(*index);
            }
            Expr::OptCall { callee, args } => {
                self.walk_expr(*callee);
                for a in args.clone() {
                    self.walk_expr(a);
                }
            }
            Expr::Index { obj, index } => {
                self.walk_expr(*obj);
                self.walk_expr(*index);
            }
            Expr::Assign { target, value } => {
                let (target, value) = (*target, *value);
                self.walk_assign(&target, &value);
            }
            Expr::BinOp { op, left, right } => {
                let (op, left, right) = (*op, *left, *right);
                self.walk_binop(&op, &left, &right);
            }
            Expr::Unary { expr, .. }
            | Expr::TypeOf { expr }
            | Expr::Spread { expr }
            | Expr::InstanceOf { expr, .. }
            | Expr::As { expr, .. } => self.walk_expr(*expr),
            Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            } => {
                self.walk_expr(*cond);
                self.walk_expr(*then_branch);
                self.walk_expr(*else_branch);
            }
            Expr::Sequence { left, right }
            | Expr::Nullish {
                lhs: left,
                rhs: right,
            } => {
                self.walk_expr(*left);
                self.walk_expr(*right);
            }
            Expr::Array(eids) => {
                // Untyped array literal (r290) — same posture as the
                // bare ObjectLit field below: a top-FnDecl Ident
                // element has no surrounding annotation to key off,
                // and the raw FnSig slot rejects at every any-boxing
                // site (`[a, b].forEach(cb)` — the box_to_any FnSig
                // sweep cluster). The wrap routes the element through
                // the closure construction site; a pure fn-arr
                // consumer calls the closure the same way.
                for e in eids {
                    if !self.try_mark(*e) {
                        self.walk_expr(*e);
                    }
                }
            }
            Expr::ObjectLit { fields } => {
                // Untyped ObjectLit. A struct fn-field slot is
                // Closure-repr in EVERY ann-minting lane (named
                // TypeDecl / ClassDecl, the parser's `__inlobj(`, and
                // the return-type inferrer's `__inlobj(` — all route
                // through `retag_field_fn_ann`), so a bare top-FnDecl
                // Ident field value needs the forwarder wrap here too,
                // with no surrounding annotation to key off. Pre-fix
                // this arm only recursed: `function make() { return {
                // g: top } }` built a FnSig slot while the inferred
                // return ann said Closure, and the field call rejected
                // the bare ptr ("value is not a function").
                for (_, feid) in fields {
                    if !self.try_mark(*feid) {
                        self.walk_expr(*feid);
                    }
                }
            }
            Expr::PostIncr { target, .. } => self.walk_expr(*target),
            Expr::New { args, .. } | Expr::Super { args } => {
                for a in args {
                    // r292 — a fn-name construct argument boxes at
                    // the ctor boundary (`new Object(func)` —
                    // S15.2.2.1_A2_T6), which a raw FnSig can't; the
                    // canonical cell keeps the `===` faces.
                    if !self.try_mark(*a) {
                        self.walk_expr(*a);
                    }
                }
            }
            // The runtime-construct form route_non_class_new minted
            // (`new Object(x)` — Object is not a declared class);
            // same ctor-boundary boxing as Expr::New. The walk had no
            // arm at all before r292 — args were invisible.
            Expr::NewDynamic { callee, args } => {
                // r293 — the CALLEE boxes at the same boundary: a
                // static-method fn-name callee (`new Error.isError()`
                // — desugared to the bare `__sm_Error__isError`
                // Ident) reaches `__torajs_anyv_construct` as an any,
                // which a raw FnSig can't box. The wrapped cell is a
                // closure, and the kernel's IsConstructor answers
                // false for closures — the spec TypeError, same as
                // the any-bound alias form already gives. `as` layers
                // are value pass-throughs (eq-operand axis
                // precedent) — `new (E as any)()` marks the inner
                // ident.
                let mut inner = *callee;
                while let Expr::As { expr, .. } = self.ast.get_expr(inner) {
                    inner = *expr;
                }
                if !self.try_mark(inner) {
                    self.walk_expr(*callee);
                }
                for a in args {
                    if !self.try_mark(*a) {
                        self.walk_expr(*a);
                    }
                }
            }
            _ => {}
        }
    }
}
