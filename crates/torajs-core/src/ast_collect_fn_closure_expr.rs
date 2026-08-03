//! `FnToClosureCollector::walk_expr` — the expression half of the
//! store-site walk, carved out of `ast_collect_fn_closure.rs` when
//! the rotation-128 shadow stack pushed that file past the 500-line
//! hard limit (the stmt half + collector struct stay there; module
//! doc lists every axis).

use crate::ast::{Ast, BinOp, Expr, ExprId, is_fn_like_ann};
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
    fn is_generator_family_ident(&self, eid: ExprId) -> bool {
        matches!(self.ast.get_expr(eid), Expr::Ident(n)
            if self.ast.generator_factory_classes.contains_key(n)
                || self.ast.async_generator_fns.contains(n))
    }

    /// A top-FnDecl Ident whose sig gained the hidden `__this` first
    /// param from bind_this_param — its raw FnSig arity no longer
    /// matches the user-visible one, so value uses must wrap.
    fn is_this_promoted_ident(&self, eid: ExprId) -> bool {
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
    fn is_untyped_plain_fn_ident(&self, eid: ExprId) -> bool {
        matches!(self.ast.get_expr(eid), Expr::Ident(n)
            if self.fn_sigs.get(n).is_some_and(
                |(params, _, _)| params.iter().any(|p| p.type_ann.is_none() && !p.is_rest)))
    }

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
            && let Some((params, _, _)) = self.fn_sigs.get(cname)
        {
            for (i, arg) in args.iter().enumerate() {
                if params.get(i).is_some_and(|p| {
                    p.type_ann.as_deref().is_some_and(|a| {
                        a.trim() == "any" || (is_fn_like_ann(a) && a.contains("__rest("))
                    })
                }) {
                    self.try_mark(*arg);
                }
                // S2.37 followup — a REWRITE-MINTED method-body ident
                // (`__sm_`/`__cm_`, the static-member rewrite's
                // product; user code never spells these) admits into
                // generic / untyped param slots too, where the
                // instantiation lands on Any and the box site can't
                // take a raw FnSig (`__t262_notSameValue(result,
                // this.method)` — the t262 forbidden-ext family).
                // Fn-like-annotated params keep their raw-FnSig
                // direct dispatch.
                if let Expr::Ident(n) = self.ast.get_expr(*arg)
                    && (n.starts_with("__sm_") || n.starts_with("__cm_"))
                    && params
                        .get(i)
                        .is_some_and(|p| !p.type_ann.as_deref().is_some_and(is_fn_like_ann))
                {
                    self.try_mark(*arg);
                }
            }
        }
        // r290 (box_to_any FnSig sweep cluster) — an indirect call
        // through a binding that is NOT a top-FnDecl name
        // (`var every = Array.prototype.every; every(callback)`):
        // the callee has no static signature, so its argv packs
        // any-boxed slots, which a raw FnSig can't. Wrap every
        // top-FnDecl Ident argument. Known-fn callees ride the
        // declared-param axis above (typed params keep raw-FnSig
        // direct dispatch), and a typed closure param is already
        // Closure-repr, so the wrap agrees with it.
        if let Expr::Ident(cname) = self.ast.get_expr(*callee)
            && !self.fn_sigs.contains_key(cname)
        {
            for &arg in args {
                self.try_mark(arg);
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
        // Cluster #4 (test262) — a bare top-FnDecl Ident argument to
        // an Object / Reflect namespace-static call
        // (`Object.getOwnPropertyDescriptor(inner, "caller")`): the
        // checker's Any-param sig admits it and the lowering packs
        // an any argv, which a raw FnSig can't box. Wrap it so the
        // boxed closure cell reaches the reflection kernels (the
        // closure-receiver forms already answer spec meta — probe
        // d5/d7). A user binding named Object/Reflect shadowing the
        // namespace only costs the wrap.
        if let Expr::Member { obj, .. } = self.ast.get_expr(*callee)
            && let Expr::Ident(ns) = self.ast.get_expr(*obj)
            && matches!(ns.as_str(), "Object" | "Reflect")
        {
            for &arg in args {
                if !self.is_generator_family_ident(arg) {
                    self.try_mark(arg);
                }
            }
        }
        // S2.37 followup — a REWRITE-MINTED method-body ident
        // (`__sm_<C>__<m>` / `__cm_<C>__<m>`, the static-member /
        // static-this rewrite's product — user code never spells
        // these) as an argument of ANY member call: the arg boxes
        // into the any world at the dispatch boundary, which a raw
        // FnSig can't (`assert.notSameValue(result, this.method)` in
        // a static body — the t262 forbidden-ext family). Bare-ident
        // callees ride the declared-param axis above; the mangled
        // prefix keeps user `xs.map(topFn)` hot paths on their raw
        // FnSig direct dispatch.
        if matches!(self.ast.get_expr(*callee), Expr::Member { .. }) {
            for &arg in args {
                if let Expr::Ident(n) = self.ast.get_expr(arg)
                    && (n.starts_with("__sm_") || n.starts_with("__cm_"))
                {
                    self.try_mark(arg);
                }
            }
        }
        // Cluster #4 (test262) — a top-FnDecl Ident as the RECEIVER
        // of a member call the Function family's member tables
        // answer with their catch-all (`inner.hasOwnProperty(…)`):
        // the read types Any, the call rides the runtime any-method
        // lane, and the receiver must box — which a raw FnSig
        // can't. Wrap it so a boxed closure cell reaches the lane.
        // The fn-surface names with their own raw-FnSig lanes stay
        // unwrapped (call/apply/bind desugar + fn-toString fold);
        // a name with a non-Function typed answer (`inner.name()`)
        // only costs the wrap — the reject stays identical.
        if let Expr::Member { obj, name: mname } = self.ast.get_expr(*callee)
            && !matches!(
                mname.as_str(),
                "call" | "apply" | "bind" | "toString" | "toLocaleString"
            )
            && !self.is_generator_family_ident(*obj)
        {
            self.try_mark(*obj);
        }
        // Cluster #1 (test262) — a member-call argument fn-Ident whose
        // raw FnSig can't serve the value use: a
        // bind_this_param-promoted fn (hidden `__this` first param —
        // every argument would land off by one slot) or an
        // untyped-param fn (becomes a `__T<N>` generic with no
        // concrete instance). Both ride the forwarder — its public
        // face skips `__this` / defaults to `any`, and its direct
        // forwarding call is the mono site for the generic. Typed,
        // unpromoted named fns keep their raw FnSig fast paths.
        if matches!(self.ast.get_expr(*callee), Expr::Member { .. }) {
            for &arg in args {
                if (self.is_this_promoted_ident(arg) || self.is_untyped_plain_fn_ident(arg))
                    && !self.is_generator_family_ident(arg)
                {
                    self.try_mark(arg);
                }
            }
        }
        self.mark_any_recv_member_args(callee, args);
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
            && let Some((params, _, _)) = self.fn_sigs.get(cname)
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
            && let Some((params, _, _)) = self.fn_sigs.get(cname)
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

    /// RFC 20260729-fn-value-any V1 — a user top-FnDecl Ident
    /// argument of a member call whose receiver cannot be statically
    /// typed: an `any`-bound ident receiver (`p.then(done)`, p: any)
    /// or a chained expression receiver (`ref(3).next().then($DONE,
    /// $DONE)` — the t262 async harness tail on every async case,
    /// the whole box_to_any FnSig 720-cluster's shared trigger). The
    /// callee rides the runtime any-method lane, whose argv boxes
    /// into the any world — a raw FnSig has no arm there. A TYPED
    /// ident receiver keeps its args unwrapped so `xs.map(topFn)`
    /// hot paths stay on raw-FnSig direct dispatch; a typed chained
    /// receiver (`getArr().map(topFn)`) only costs the wrap —
    /// closure values are legal fn-param arguments.
    fn mark_any_recv_member_args(&mut self, callee: &ExprId, args: &[ExprId]) {
        let Expr::Member { obj, .. } = self.ast.get_expr(*callee) else {
            return;
        };
        let typed_ident_recv = matches!(
            self.ast.get_expr(*obj),
            Expr::Ident(n) if !self.any_bindings.contains(n)
                && !self.new_init_bindings.contains(n)
        );
        if typed_ident_recv {
            return;
        }
        for &arg in args {
            if !self.is_generator_family_ident(arg) {
                self.try_mark(arg);
            }
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
                    self.walk_expr(*a);
                }
            }
            _ => {}
        }
    }
}
