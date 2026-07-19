//! `FnToClosureCollector::walk_expr` — the expression half of the
//! store-site walk, carved out of `ast_collect_fn_closure.rs` when
//! the rotation-128 shadow stack pushed that file past the 500-line
//! hard limit (the stmt half + collector struct stay there; module
//! doc lists every axis).

use crate::ast::{Ast, BinOp, Expr, ExprId, is_fn_like_ann};
use crate::ast_collect_fn_closure::{FnToClosureCollector, is_fn_like_field_ann};

impl<'a> FnToClosureCollector<'a> {
    /// Walk an Expr looking for nested store-sites (Call args,
    /// assigns into `any` bindings, nested ObjectLits, etc.).
    /// `Expr::Call` arm — the callee/arg boxing decisions plus the
    /// recursive walk.
    fn walk_call(&mut self, callee: &ExprId, args: &[ExprId]) {
        // L3b #8 — a bare top-FnDecl Ident passed where the
        // callee's declared param is `any`-annotated boxes
        // into the any world; wrap it. Fn-typed params stay
        // raw FnSig (direct dispatch preserved), and rest /
        // excess positions fall through untouched.
        //
        // RFC 20260708-variadic chunk 3 — a variadic-
        // annotated param (`(...args: E[]) => R`, encoded
        // `__rest(` in the fn-like ann) holds a Closure repr
        // slot and calls through the boxed dual entry; a raw
        // FnSig there has neither env cell nor adapter, so
        // it wraps the same way.
        if let Expr::Ident(cname) = self.ast.get_expr(*callee)
            && let Some((params, _)) = self.fn_sigs.get(cname)
        {
            for (i, arg) in args.iter().enumerate() {
                if params.get(i).is_some_and(|p| {
                    p.type_ann.as_deref().is_some_and(|a| {
                        a.trim() == "any" || (is_fn_like_ann(a) && a.contains("__rest("))
                    })
                }) {
                    self.try_mark(*arg);
                }
            }
        }
        // `<key> in <fn>` — the parser rewrites the binary op to a
        // synthetic `__torajs_in_op(key, obj)` call; a top-FnDecl
        // Ident on the rhs is a value use (the lowering boxes the
        // closure cell into the Any kernels), so it wraps.
        if let Expr::Ident(cname) = self.ast.get_expr(*callee)
            && cname == "__torajs_in_op"
            && args.len() == 2
        {
            self.try_mark(args[1]);
        }
        // Chunk 617 — replace-cb argument site (see module
        // doc): the runtime's functional-replaceValue lane
        // needs a closure cell, so a named top fn wraps.
        if let Expr::Member { name: mname, .. } = self.ast.get_expr(*callee)
            && matches!(mname.as_str(), "replace" | "replaceAll")
            && args.len() >= 2
        {
            self.try_mark(args[1]);
        }
        // Chunk 733 — `fns.push(top_fn)` / `fns.unshift(top_fn)`
        // where `fns` was declared with a fn-typed array ann:
        // the element slot is Closure-repr.
        if let Expr::Member { obj, name: mname } = self.ast.get_expr(*callee)
            && matches!(mname.as_str(), "push" | "unshift")
            && let Expr::Ident(oname) = self.ast.get_expr(*obj)
            && self.fn_arr_bindings.contains(oname)
        {
            for &arg in args {
                self.try_mark(arg);
            }
        }
        // Chunk 733 — an array-literal argument whose matching
        // declared param carries a fn-typed array ann:
        // `takeOps([top_fn])`.
        if let Expr::Ident(cname) = self.ast.get_expr(*callee)
            && let Some((params, _)) = self.fn_sigs.get(cname)
        {
            for (i, arg) in args.iter().enumerate() {
                if params
                    .get(i)
                    .is_some_and(|p| p.type_ann.as_deref().is_some_and(crate::ast::is_fn_arr_ann))
                {
                    self.mark_array_lit_elems(*arg);
                }
            }
        }
        // Chunk 793 — an ObjectLit argument whose matching
        // declared param resolves to a struct shape (named
        // TypeDecl or inline object type) with fn-like
        // fields: `take({ k: top_fn })` stores into a
        // Closure-repr slot, so the bare named-fn field
        // value wraps (the raw FnSig would be CallIndirect'd
        // as an env block — SIGBUS).
        if let Expr::Ident(cname) = self.ast.get_expr(*callee)
            && let Some((params, _)) = self.fn_sigs.get(cname)
        {
            for (i, arg) in args.iter().enumerate() {
                if let Some(field_anns) = params
                    .get(i)
                    .and_then(|p| p.type_ann.as_deref())
                    .and_then(|a| self.resolve_field_anns(a))
                {
                    self.mark_objlit_fn_fields(*arg, &field_anns);
                }
            }
        }
        self.walk_expr(*callee);
        for arg in args {
            self.walk_expr(*arg);
        }
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
            Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => self.walk_expr(*obj),
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
                for e in eids {
                    self.walk_expr(*e);
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
                    self.walk_expr(*a);
                }
            }
            _ => {}
        }
    }
}
