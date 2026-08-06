//! `FnToClosureCollector::walk_call` — the Call-arm half of the
//! store-site walk, carved out of `ast_collect_fn_closure_expr.rs`
//! when the rotation-291 generic-param axis pushed that file to the
//! 500-line hard limit (the other expr arms + walk_expr stay there;
//! `ast_collect_fn_closure.rs`'s module doc lists every axis).

use crate::ast::{Expr, ExprId, is_fn_like_ann};
use crate::ast_collect_fn_closure::FnToClosureCollector;

impl<'a> FnToClosureCollector<'a> {
    /// The known-fn-callee axes — L3b #8: a bare top-FnDecl Ident
    /// passed where the callee's declared param is `any`-annotated
    /// boxes into the any world; wrap it. Fn-typed params stay raw
    /// FnSig (direct dispatch preserved), and rest / excess
    /// positions fall through untouched. RFC 20260708-variadic
    /// chunk 3 — a variadic-annotated param (`(...args: E[]) => R`,
    /// encoded `__rest(` in the fn-like ann) holds a Closure-repr
    /// slot and calls through the boxed dual entry, so it wraps the
    /// same way.
    fn mark_known_callee_args(&mut self, cname: &str, args: &[ExprId]) {
        let Some((params, _, _)) = self.fn_sigs.get(cname) else {
            return;
        };
        let type_params = self.fn_type_params.get(cname);
        for (i, arg) in args.iter().enumerate() {
            if params.get(i).is_some_and(|p| {
                p.type_ann.as_deref().is_some_and(|a| {
                    a.trim() == "any"
                        || (is_fn_like_ann(a) && a.contains("__rest("))
                        // Generic-param axis: a param annotated
                        // with one of the callee's own TypeVars
                        // (`sameValue<T>(actual: T, expected: T)`)
                        // instantiates at Any for a fn-name
                        // argument — the boxed argv slot can't
                        // take a raw FnSig, and the canonical
                        // `__forward_*` cell keeps `===` faces
                        // agreeing across wrap sites.
                        || type_params.is_some_and(|tps| tps.iter().any(|tp| tp == a.trim()))
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

    /// RFC 20260806 rotation 318 — a builtin method this module might
    /// have patched is not lowered to its kernel; the call stands
    /// down to the any-lane, whose argv packs any-boxed slots. A raw
    /// FnSig cannot ride there, and both ways it fails are
    /// whole-program rejects rather than wrong answers: an annotated
    /// callback dies at the boxing site ("box_to_any element type
    /// FnSig"), and an un-annotated one is an implicit generic that
    /// no call-site record ever instantiates ("unknown function").
    /// Both are how `[12, 11].every(callbackfn1)` broke when the gate
    /// first opened for the higher-order methods — a shape no probe
    /// wrote, because probes pass arrow literals.
    ///
    /// The receiver has no type yet at this point in the pipeline, so
    /// the question is asked by method name alone; wrapping a call
    /// that would not have stood down costs one closure and nothing
    /// else. An empty shadow set answers no to everything, which is
    /// every program that leaves builtin prototypes alone — so
    /// `xs.map(top_fn)` keeps its raw-FnSig direct dispatch.
    fn mark_shadowed_builtin_args(&mut self, callee: &ExprId, args: &[ExprId]) {
        if let Expr::Member { name, .. } = self.ast.get_expr(*callee)
            && self.proto_shadow.may_stand_down(name)
        {
            for &arg in args {
                self.try_mark(arg);
            }
        }
    }

    /// Walk an Expr looking for nested store-sites (Call args,
    /// assigns into `any` bindings, nested ObjectLits, etc.).
    /// `Expr::Call` arm — the callee/arg boxing decisions plus the
    /// recursive walk.
    pub(crate) fn walk_call(&mut self, callee: &ExprId, args: &[ExprId]) {
        if let Expr::Ident(cname) = self.ast.get_expr(*callee) {
            let cname = cname.clone();
            self.mark_known_callee_args(&cname, args);
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
        self.mark_shadowed_builtin_args(callee, args);
        self.mark_namespace_static_args(callee, args);
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
        {
            // r292 — generator factories wrap here too (the G2
            // forward-cell reflection faces carry them):
            // `f.hasOwnProperty("caller")` — the forbidden-ext /
            // restricted-properties family. `.prototype` reads are
            // member READS, not calls — the static fold this axis
            // must not erase never routes through it. r295 — `as`
            // layers on the receiver peel, same as the store-site
            // twin (`(__PROTO as any).isPrototypeOf(m)` marks the
            // inner Ident; the As forwards the cell unchanged).
            let base = {
                let mut e = *obj;
                while let Expr::As { expr, .. } = self.ast.get_expr(e) {
                    e = *expr;
                }
                e
            };
            self.try_mark(base);
        }
        // r291 — the apply/bind forms the fn-proto desugar does NOT
        // swallow (dynamic argArray `f.apply(t, arr)`, surplus bind
        // partials): the member call survives to lowering, so the
        // fn-name receiver must ride the wrapped closure lane (the
        // any-method apply/bind kernels take it from there). The
        // desugar's own predicate is the single source of truth —
        // wrapping a swallowed form would hide the Ident the
        // desugar's rewrite matches on.
        if let Expr::Member { obj, name: mname } = self.ast.get_expr(*callee)
            && matches!(mname.as_str(), "apply" | "bind")
            && let Expr::Ident(fname) = self.ast.get_expr(*obj)
            && let Some((params, _, _)) = self.fn_sigs.get(fname)
            && !crate::ast_desugar_function_prototype_methods::swallows_fn_proto_call(
                self.ast, mname, args, params,
            )
        {
            // r292 — the generator exclusion drops here too: an
            // unswallowed `g.apply(t, arr)` boxes its factory
            // receiver the same way (forward cell → any-apply
            // kernel → the factory dispatch mints the generator).
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

    /// Cluster #4 (test262) — a bare top-FnDecl Ident argument to
    /// an Object / Reflect namespace-static call
    /// (`Object.getOwnPropertyDescriptor(inner, "caller")`): the
    /// checker's Any-param sig admits it and the lowering packs
    /// an any argv, which a raw FnSig can't box. Wrap it so the
    /// boxed closure cell reaches the reflection kernels (the
    /// closure-receiver forms already answer spec meta — probe
    /// d5/d7). A user binding named Object/Reflect shadowing the
    /// namespace only costs the wrap.
    fn mark_namespace_static_args(&mut self, callee: &ExprId, args: &[ExprId]) {
        let Expr::Member { obj, name: nsname } = self.ast.get_expr(*callee) else {
            return;
        };
        let Expr::Ident(ns) = self.ast.get_expr(*obj) else {
            return;
        };
        if !matches!(ns.as_str(), "Object" | "Reflect") {
            return;
        }
        // getPrototypeOf keeps the generator exclusion: its
        // bare-Ident arg rides the compile-time genfn-trio fold
        // (kind-exact, ssa_lower_call_object_get_prototype_of),
        // which a wrap would erase. Every other member wraps —
        // the forward cell carries the G2 reflection faces
        // (gen-proto install + FLAG_FN_GENERATOR), so gOPD /
        // gOPN / keys answer through the closure kernels
        // (r292: the restricted-properties / forbidden-ext
        // box_to_any FnSig family).
        let keep_gen_exclusion = nsname == "getPrototypeOf";
        for &arg in args {
            if !(keep_gen_exclusion && self.is_generator_family_ident(arg)) {
                self.try_mark(arg);
            }
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
                // r293 — an `any`/untyped param of the enclosing fn
                // (a lifted arrow's `iter => iter.map(fn)`) is an
                // any receiver too.
                && !self.any_param_frames.iter().any(|f| f.contains(n))
        );
        if typed_ident_recv {
            return;
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
