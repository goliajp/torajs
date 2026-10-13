//! `FnToClosureCollector::walk_expr` — the expression half of the
//! store-site walk, carved out of `ast_collect_fn_closure.rs` when
//! the rotation-128 shadow stack pushed that file past the 500-line
//! hard limit (the stmt half + collector struct stay there; module
//! doc lists every axis). The `Expr::Call` arm lives in
//! `ast_collect_fn_closure_call.rs` since rotation 291.

use crate::ast::{BinOp, Expr, ExprId};
use crate::ast_collect_fn_closure::FnToClosureCollector;

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
    pub(crate) fn is_no_original_fn_ident(&self, eid: ExprId) -> bool {
        matches!(self.ast.get_expr(eid), Expr::Ident(n)
            // 563-08 — an EXPLICIT generic (`function idf<T>(x: T)`)
            // has no original for the same reason an implicit one
            // does, and its params ARE annotated, so the older
            // spelling of this question ("does it have an
            // un-annotated param?") answered no for it.
            if self.fn_type_params.contains_key(n)
                || self.fn_sigs.get(n).is_some_and(
                    |(params, _, _)| params.iter().any(|p| p.type_ann.is_none() && !p.is_rest)))
    }

    /// `Expr::Assign` arm.
    fn walk_assign(&mut self, target: &ExprId, value: &ExprId) {
        // L3b #8 — `f = name` where `f` was declared `any`;
        // RFC 20260709 chunk 4 — `cb = name` where `cb` is a
        // Closure-repr top-level binding.
        // Rotation 599 — the identity axis, assign half. When the
        // let-decl arm wrapped a binding's INIT because the name is
        // compared, its slot re-reprs Closure, so a later `h = t`
        // feeding the raw FnSig is a slot/value mismatch (a loud
        // `assignment to \`h\` mismatch` reject, which is how this
        // was caught). Both halves read the same census, so the
        // binding is Closure-repr on every write or on none.
        if let Expr::Ident(tname) = self.ast.get_expr(*target)
            && (self.any_bindings.contains(tname)
                || self.closure_bindings.contains(tname)
                || self.ident_census.eq_operands.contains(tname))
        {
            self.try_mark(*value);
        }
        // Chunk 733 — `fns[i] = top_fn` (fn-typed array ann,
        // Closure-repr element slot); widened to EVERY index-assign
        // RHS in r295: a computed-key store on any receiver —
        // `(Array.prototype as any)[Symbol.iterator] = gen` — rides
        // the keyed set lane, whose value slot boxes into the any
        // world, which a raw FnSig can't. (r293's probe of this
        // widening hit a poison-split codegen fault — the container
        // poison reached field layouts but not their scalar
        // dependents, bit-punning an F64 load into a GPR consumer;
        // fixed in num_width::analyze. No generator-family skip:
        // since r292 the forward cell carries the G2 generator
        // reflection faces.)
        if let Expr::Index { .. } = self.ast.get_expr(*target) {
            self.try_mark(*value);
        }
        // Every member store's slot holds a function OBJECT, never a
        // bare code address: a fn-typed struct field is Closure-repr
        // after the retag pass (chunk 783 — a raw FnSig there was
        // CallIndirected as an env block, SIGBUS), and every other
        // slot boxes into the any world, which a raw FnSig can't
        // (r293: `box.prop = fn` on an any binding; the same store
        // through `globalThis.X`, `(m as any).f` or `mk().f` was a
        // loud lowering reject). The index-assign twin above has
        // marked unconditionally since r295.
        if let Expr::Member { .. } = self.ast.get_expr(*target) {
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
            self.walk_eq_operand(*left);
            self.walk_eq_operand(*right);
        } else {
            self.walk_expr(*left);
            self.walk_expr(*right);
        }
    }

    /// An operand of IsStrictlyEqual / IsLooselyEqual: `===` and its
    /// kin, a switch scrutinee and its case values (§14.12.4). `as`
    /// layers are value pass-throughs — strip so `(getX as any) ===
    /// getX` marks the inner Ident (the rewrite lands there; the As
    /// then forwards the canonical cell unchanged).
    pub(crate) fn walk_eq_operand(&mut self, e: ExprId) {
        let mut inner = e;
        while let Expr::As { expr, .. } = self.ast.get_expr(inner) {
            inner = *expr;
        }
        if !self.try_mark(inner) {
            self.walk_expr(e);
        }
    }

    /// The `Expr::Delete` arm of [`Self::walk_expr`] — `delete f.k`
    /// has to reach the fn OBJECT, and on the FnSig lane a bare fn
    /// name is a raw vaddr with no cell at all. Marking the base
    /// routes it through the canonical `__fncell_` singleton, the
    /// SAME object every other value site answers, so the reads that
    /// follow see the removal. Minting a second cell here instead
    /// would answer `true` and change nothing observable.
    ///
    /// The base only. A key expression inside the operand (`delete
    /// o[f]`) is not walked today either, and pulling that in is a
    /// separate axis with its own faces.
    pub(crate) fn mark_delete_receiver(&mut self, operand: ExprId) {
        let base = match self.ast.get_expr(operand) {
            Expr::Member { obj, .. } | Expr::Index { obj, .. } => *obj,
            _ => return,
        };
        self.try_mark(self.strip_as(base));
    }

    /// `obj` when it names a fn whose properties this module deletes
    /// or redefines — the base then wraps at EVERY read, `name` /
    /// `length` included: those two fold statically off a raw base
    /// and would keep answering the declaration after the removal.
    /// Through the cell they stay right, since the length read carries
    /// the declared arity as its fallback (`f(a, b = 39).length` is
    /// still 1) and the name read walks the cell-aware chain the `any`
    /// spelling already walked correctly.
    ///
    /// A define is the same shape as a delete and was the same
    /// silent-wrong: `Object.defineProperty(f, "length", {value: 9})`
    /// then `f.length` answered the declared arity while `(f as any)
    /// .length` — one cell, one property — answered 9. §20.2.3 makes
    /// both properties configurable, so the define is legal and the
    /// fold is what has to stand down.
    ///
    /// Looks through `as` layers, which the sibling axes do not:
    /// `(g as any).name` is still a read of `g` and the cast is
    /// identity at run time. The generator / async forms stay raw for
    /// the reason the prototype axis gives — their faces ride
    /// dedicated static arms.
    pub(crate) fn reflected_fn_base(&self, obj: ExprId) -> Option<ExprId> {
        let base = self.strip_as(obj);
        match self.ast.get_expr(base) {
            Expr::Ident(n)
                if (self.ident_census.delete_bases.contains(n)
                    || self.ident_census.define_bases.contains(n))
                    && !self.ast.generator_factory_classes.contains_key(n)
                    && !self.ast.async_fns.contains(n)
                    && !self.ast.async_generator_fns.contains(n) =>
            {
                Some(base)
            }
            _ => None,
        }
    }

    /// `e` with every `as` layer peeled — a cast is identity at run
    /// time, so a value question about `(x as T)` is a question
    /// about `x`.
    pub(crate) fn strip_as(&self, mut e: ExprId) -> ExprId {
        while let Expr::As { expr, .. } = self.ast.get_expr(e) {
            e = *expr;
        }
        e
    }

    /// The `Expr::Member` arm of [`Self::walk_expr`] — the member-BASE
    /// axis. Extracted the way `walk_let_decl` was (rotation 434): it
    /// is the walker's biggest arm, and the delete-receiver axis
    /// pushed the walker past the 200-line cap.
    fn walk_member_base(&mut self, obj: ExprId, name: &str) {
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
        let plain_fn_base = matches!(self.ast.get_expr(obj), Expr::Ident(n)
                if !self.ast.generator_factory_classes.contains_key(n)
                    && !self.ast.async_fns.contains(n)
                    && !self.ast.async_generator_fns.contains(n));
        // Rotation 459 — an implicit-generic decl (any
        // un-annotated param) has NO lowered original: the
        // `desugar_implicit_generics` pass gives it `__T<N>`
        // TypeVars and only per-call monos are emitted, so a
        // bare-Ident base of ANY property died with
        // `ssa-lower: unknown ident`. Its forwarder cell is
        // the fn object (`typeof` / `name` / `length` all
        // answer through it), so the base wraps whatever the
        // property is — that is the whole reason the
        // prototype-only restriction above can stay narrow
        // for the fns that DO have a lowered original.
        // `name` / `length` are the exception even here: their
        // static arms read the DECL (so `f(a, b = 39).length`
        // is 1 per §20.2.4.1), while the forwarder shim has
        // lost the default and answers its own arity — gate
        // fn-length-dflt-001 caught exactly that.
        let no_original_base = plain_fn_base
            && name != "name"
            && name != "length"
            && self.is_no_original_fn_ident(obj);
        // 633-02 — a fn whose properties are deleted or redefined
        // anywhere in the module wraps at EVERY read (see
        // `reflected_fn_base`), `name` / `length` included.
        if let Some(b) = self.reflected_fn_base(obj)
            && self.try_mark(b)
        {
        } else if (name != "prototype" && !no_original_base) || !self.try_mark(obj) {
            self.walk_expr(obj);
        }
    }

    pub(crate) fn walk_expr(&mut self, eid: ExprId) {
        match self.ast.get_expr(eid) {
            Expr::Call { callee, args } => {
                let (callee, args) = (*callee, args.clone());
                self.walk_call(&callee, &args);
            }
            Expr::Member { obj, name } => {
                let (obj, name) = (*obj, name.clone());
                self.walk_member_base(obj, &name);
            }
            Expr::Delete { expr } => self.mark_delete_receiver(*expr),
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
                let (obj, index) = (*obj, *index);
                // A computed key on a bare declaration reads a property
                // of the function OBJECT (`f[Symbol.hasInstance](x)`),
                // and the FnSig lane has no object to read it off: the
                // base rides the canonical cell. A string key is a
                // property name the static member arms answer.
                let computed_on_plain_fn = !matches!(self.ast.get_expr(index), Expr::String(_))
                    && matches!(self.ast.get_expr(obj), Expr::Ident(n)
                        if !self.ast.generator_factory_classes.contains_key(n)
                            && !self.ast.async_fns.contains(n)
                            && !self.ast.async_generator_fns.contains(n));
                if !(computed_on_plain_fn && self.try_mark(obj)) {
                    self.walk_expr(obj);
                }
                self.walk_expr(index);
            }
            Expr::Assign { target, value } => {
                let (target, value) = (*target, *value);
                self.walk_assign(&target, &value);
            }
            Expr::BinOp { op, left, right } => {
                let (op, left, right) = (*op, *left, *right);
                if matches!(op, BinOp::LAnd | BinOp::LOr) {
                    self.target_mixed_choice(eid);
                }
                self.walk_binop(&op, &left, &right);
            }
            // Rotation 459 — `typeof F` on an implicit-generic decl:
            // same missing original as the member-base axis, but this
            // one failed SILENTLY (the typeof lane's fn-name arm keys
            // on the raw-FnSig table, misses, and falls through to
            // `undefined`). Wrapping answers "function" off the cell.
            Expr::TypeOf { expr } => {
                let e = *expr;
                let wraps = self.is_no_original_fn_ident(e) && !self.is_generator_family_ident(e);
                if !wraps || !self.try_mark(e) {
                    self.walk_expr(e);
                }
            }
            Expr::Unary { expr, .. } | Expr::Spread { expr } => self.walk_expr(*expr),
            Expr::InstanceOf { expr, rhs } => {
                self.walk_expr(*expr);
                self.walk_expr(*rhs);
            }
            // `fn as any` — the cast IS an any-boxing position (the
            // widened box has no FnSig arm), so a bare top-FnDecl
            // Ident wraps to its canonical forwarder cell right here;
            // `(Pair as any).bind(null, 1)` is the t262 shape that
            // panicked at the box site. Nested As layers peel like
            // every other axis; non-any casts keep the plain
            // recursion (typecheck-only, no IR side effect).
            Expr::As { expr, ty_ann } => {
                let (expr, is_any) = (*expr, ty_ann == "any");
                let inner = {
                    let mut e = expr;
                    while let Expr::As { expr, .. } = self.ast.get_expr(e) {
                        e = *expr;
                    }
                    e
                };
                if !is_any || !self.try_mark(inner) {
                    self.walk_expr(expr);
                }
            }
            Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            } => {
                self.target_mixed_choice(eid);
                self.walk_expr(*cond);
                self.walk_expr(*then_branch);
                self.walk_expr(*else_branch);
            }
            Expr::Sequence { left, right }
            | Expr::Nullish {
                lhs: left,
                rhs: right,
            } => {
                if matches!(self.ast.get_expr(eid), Expr::Nullish { .. }) {
                    self.target_mixed_choice(eid);
                }
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
