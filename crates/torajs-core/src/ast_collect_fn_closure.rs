//! FnSig-to-Closure store-site collection (chunk 142 extract;
//! chunk 525 collector-struct rework + the L3b #8 axes).
//!
//! The pre-typecheck walk finds positions where a bare top-level
//! FnDecl Ident crosses into a Closure-expected or `any`-boxed slot
//! and marks it for the `__forward_<name>` zero-capture closure
//! rewrite (`ast/forwarders_object.rs` synthesizes the forwarders
//! and applies the rewrites):
//!
//! - `let x: <struct-with-Closure-field> = { ..., f: name, ... }` —
//!   the destination field was `__cls`-tagged.
//! - `let f: any = name` and ObjectLit-field / Array-element
//!   positions inside an `any`-annotated init (chunks 518/519).
//! - `f = name` where `f` was declared with an `any` annotation
//!   (L3b #8 assign-into-any; binding names collect scope-
//!   approximate — a shadowing non-any `f` in another scope would
//!   also match, which only costs an unnecessary wrap).
//! - `callee(name)` where the matching declared param is
//!   `any`-annotated (L3b #8; fn-typed params stay raw FnSig —
//!   direct dispatch preserved).
//! - `return name` inside a fn whose declared return type is `any`
//!   (L3b #8; closure-typed returns ride the older
//!   `ast/forwarders.rs` pass).
//! - `callee(name)` where the matching declared param carries a
//!   variadic fn-type ann (`__rest(` — RFC 20260708-variadic
//!   chunk 3): the slot is Closure repr and dispatches through the
//!   boxed dual entry, which a raw FnSig doesn't carry.
//! - `s.replace(pat, name)` / `s.replaceAll(pat, name)` (chunk 617)
//!   — the functional-replaceValue runtime invokes the callback
//!   through the closure boxed entry; a bare FnSig has neither env
//!   nor boxed entry (604-era loud panic). A user-class `.replace`
//!   method hit by the same shape only costs the unnecessary wrap
//!   (closure values are legal fn-param arguments).
//! - `cb = name` where `cb` is a Closure-repr top-level binding
//!   (RFC 20260709-closure-global chunk 4): the mutable-closure
//!   global assign lane stores closure cells.
//! - top-level `const f = name` / `const f: (P)=>R = name` read by
//!   a named-fn body (same RFC; the let-init axis lives in
//!   `ast/forwarders_object.rs` — it needs the ast_refs gate, not
//!   the recursive walk).
//! - fn-typed ARRAY positions (chunk 733 — element slots are
//!   Closure-repr per `ssa_lower_parse_type`'s `[]` re-repr):
//!   array-literal elements in a fn-arr-annotated let-init or
//!   call-arg (`const ops: ((n)=>n)[] = [name]` / `run([name])`),
//!   `fns.push(name)` / `fns.unshift(name)`, and `fns[i] = name`
//!   where `fns` was declared with a fn-arr annotation.
//!
//! Without the wrap these positions hold a raw FnSig value: the
//! any-boxing site has no FnSig arm ("box_to_any element type FnSig
//! not supported") and the runtime `__torajs_any_call` rejects
//! non-closure-cell callees — the wrap routes the value through the
//! closure construction site, which carries the boxed dual entry.

use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, Expr, ExprId, Param, Stmt, is_fn_like_ann};

/// Walk state + outputs for the store-site collection. `targets`
/// is the set of fn names needing a forwarder; `rewrites` the exact
/// ExprIds to replace with `Closure { __forward_<name> }`.
pub(crate) struct FnToClosureCollector<'a> {
    pub(crate) ast: &'a Ast,
    pub(crate) fn_sigs: &'a HashMap<String, (Vec<Param>, Option<String>)>,
    pub(crate) struct_field_anns: &'a HashMap<String, HashMap<String, String>>,
    /// Binding names declared with an `any` annotation anywhere in
    /// the program (scope-approximate; see module doc).
    pub(crate) any_bindings: &'a HashSet<String>,
    /// Top-level binding names whose slot is Closure-repr (a lifted
    /// arrow init or a fn-type annotation) — RFC 20260709 chunk 4:
    /// `cb = top_fn` must wrap so the drop-old/store-new global lane
    /// stores a closure cell. Scope-approximate like `any_bindings`
    /// (a shadowing local match only costs the wrap).
    pub(crate) closure_bindings: &'a HashSet<String>,
    /// Binding names declared with a fn-typed ARRAY annotation
    /// (chunk 733, `is_fn_arr_ann`): the element slot is Closure-repr,
    /// so `fns.push(top_fn)` / `fns[i] = top_fn` wrap. Scope-
    /// approximate like the other two sets.
    pub(crate) fn_arr_bindings: &'a HashSet<String>,
    pub(crate) targets: HashSet<String>,
    pub(crate) rewrites: Vec<(ExprId, String)>,
}

/// Collect every `let`/`const`/`var` binding name carrying an `any`
/// annotation, recursing through fn bodies and statement containers.
pub(crate) fn collect_any_bindings(stmts: &[Stmt], out: &mut HashSet<String>) {
    collect_bindings_matching(stmts, &|a| a.trim() == "any", out);
}

/// Chunk 733 — binding names declared with a fn-typed array
/// annotation (`((n)=>n)[]` / `Array<(n)=>n>`); their element slots
/// are Closure-repr, so named-fn store-sites into them wrap.
pub(crate) fn collect_fn_arr_bindings(stmts: &[Stmt], out: &mut HashSet<String>) {
    collect_bindings_matching(stmts, &crate::ast::is_fn_arr_ann, out);
}

/// Chunk 736 — binding names declared with a plain fn-type
/// annotation at ANY scope depth (the closure_bindings top-level
/// walk misses fn-body `let cb: (n)=>n` bindings, so a body-local
/// `cb = top_fn` assign escaped the wrap). Variadic anns keep their
/// boxed-dual route.
pub(crate) fn collect_fn_ann_bindings(stmts: &[Stmt], out: &mut HashSet<String>) {
    collect_bindings_matching(stmts, &|a| is_fn_like_ann(a) && !a.contains("__rest("), out);
}

/// Shared annotation-predicate binding walk (chunk 733 — the any
/// and fn-arr collections differ only in the ann test).
fn collect_bindings_matching(
    stmts: &[Stmt],
    pred: &dyn Fn(&str) -> bool,
    out: &mut HashSet<String>,
) {
    for s in stmts {
        collect_bindings_matching_stmt(s, pred, out);
    }
}

fn collect_bindings_matching_stmt(
    s: &Stmt,
    pred: &dyn Fn(&str) -> bool,
    out: &mut HashSet<String>,
) {
    match s {
        Stmt::LetDecl { name, type_ann, .. } => {
            if type_ann.as_deref().is_some_and(pred) {
                out.insert(name.clone());
            }
        }
        Stmt::FnDecl { body, .. } => collect_bindings_matching(body, pred, out),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            collect_bindings_matching_stmt(then_branch, pred, out);
            if let Some(eb) = else_branch {
                collect_bindings_matching_stmt(eb, pred, out);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            collect_bindings_matching_stmt(body, pred, out)
        }
        Stmt::For { init, body, .. } => {
            if let Some(init) = init {
                collect_bindings_matching_stmt(init, pred, out);
            }
            collect_bindings_matching_stmt(body, pred, out);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => collect_bindings_matching(stmts, pred, out),
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                collect_bindings_matching(&c.body, pred, out);
            }
            if let Some(d) = default {
                collect_bindings_matching(d, pred, out);
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            collect_bindings_matching(body, pred, out);
            collect_bindings_matching(catch_body, pred, out);
            if let Some(fb) = finally_body {
                collect_bindings_matching(fb, pred, out);
            }
        }
        _ => {}
    }
}

impl<'a> FnToClosureCollector<'a> {
    /// Mark `eid` for the forwarder rewrite when it is a bare
    /// top-FnDecl Ident. Answers whether it matched.
    fn try_mark(&mut self, eid: ExprId) -> bool {
        if let Expr::Ident(name) = self.ast.get_expr(eid)
            && self.fn_sigs.contains_key(name)
        {
            self.targets.insert(name.clone());
            self.rewrites.push((eid, name.clone()));
            return true;
        }
        false
    }

    /// Walk one Stmt (and any Stmts / Exprs it contains) looking for
    /// store-sites. `ret_is_any` carries the enclosing fn's declared
    /// `any` return annotation down to `Return` sites.
    pub(crate) fn walk_stmt(&mut self, s: &Stmt, ret_is_any: bool) {
        match s {
            Stmt::LetDecl {
                mutable,
                type_ann,
                init,
                ..
            } => {
                self.collect_objectlit_field_sites(*init, type_ann.as_deref());
                if type_ann.as_deref().is_some_and(|a| a.trim() == "any") {
                    self.collect_any_init_sites(*init);
                }
                // Chunk 733 — `const fns: ((n)=>n)[] = [top_fn, ...]`:
                // the element slot is Closure-repr, wrap each bare
                // named-fn element.
                if type_ann.as_deref().is_some_and(crate::ast::is_fn_arr_ann) {
                    self.mark_array_lit_elems(*init);
                }
                // Chunk 736 — a MUTABLE fn-typed binding initialized
                // with a bare named fn (`let cb: (n)=>n = take`): the
                // slot re-reprs Closure (chunk 732 local / K.3b
                // global), so the raw-FnSig init wraps. The rewrite
                // also turns the init into Expr::Closure, steering
                // the lowerer off the immutable-only fn_addr_let
                // direct-dispatch lane naturally. Variadic anns keep
                // their own boxed-dual route (chunk-4 axis mirror).
                if *mutable
                    && type_ann
                        .as_deref()
                        .is_some_and(|a| is_fn_like_ann(a) && !a.contains("__rest("))
                {
                    self.try_mark(*init);
                }
                self.walk_expr(*init);
            }
            Stmt::FnDecl {
                return_type, body, ..
            } => {
                let ret_any = return_type.as_deref().is_some_and(|a| a.trim() == "any");
                for inner in body {
                    self.walk_stmt(inner, ret_any);
                }
            }
            Stmt::Expr(eid) => self.walk_expr(*eid),
            Stmt::Return(Some(eid)) => {
                // L3b #8 — `return name` boxes into an `any` return
                // slot; wrap so the box holds a closure cell.
                if !(ret_is_any && self.try_mark(*eid)) {
                    self.walk_expr(*eid);
                }
            }
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.walk_expr(*cond);
                self.walk_stmt(then_branch, ret_is_any);
                if let Some(eb) = else_branch {
                    self.walk_stmt(eb, ret_is_any);
                }
            }
            Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
                self.walk_expr(*cond);
                self.walk_stmt(body, ret_is_any);
            }
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => {
                if let Some(init) = init {
                    self.walk_stmt(init, ret_is_any);
                }
                if let Some(c) = cond {
                    self.walk_expr(*c);
                }
                if let Some(stp) = step {
                    self.walk_expr(*stp);
                }
                self.walk_stmt(body, ret_is_any);
            }
            Stmt::Block(stmts) | Stmt::Multi(stmts) => {
                for inner in stmts {
                    self.walk_stmt(inner, ret_is_any);
                }
            }
            Stmt::Switch {
                scrutinee,
                cases,
                default,
            } => {
                self.walk_expr(*scrutinee);
                for c in cases {
                    for inner in &c.body {
                        self.walk_stmt(inner, ret_is_any);
                    }
                }
                if let Some(d) = default {
                    for inner in d {
                        self.walk_stmt(inner, ret_is_any);
                    }
                }
            }
            Stmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                for inner in body {
                    self.walk_stmt(inner, ret_is_any);
                }
                for inner in catch_body {
                    self.walk_stmt(inner, ret_is_any);
                }
                if let Some(fb) = finally_body {
                    for inner in fb {
                        self.walk_stmt(inner, ret_is_any);
                    }
                }
            }
            Stmt::Throw(eid) | Stmt::Yield(eid) => self.walk_expr(*eid),
            _ => {}
        }
    }

    /// Walk an Expr looking for nested store-sites (Call args,
    /// assigns into `any` bindings, nested ObjectLits, etc.).
    fn walk_expr(&mut self, eid: ExprId) {
        match self.ast.get_expr(eid) {
            Expr::Call { callee, args } => {
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
                    for arg in args.clone() {
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
                        if params.get(i).is_some_and(|p| {
                            p.type_ann.as_deref().is_some_and(crate::ast::is_fn_arr_ann)
                        }) {
                            self.mark_array_lit_elems(*arg);
                        }
                    }
                }
                self.walk_expr(*callee);
                for arg in args {
                    self.walk_expr(*arg);
                }
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
                self.walk_expr(*target);
                self.walk_expr(*value);
            }
            Expr::BinOp { left, right, .. } => {
                self.walk_expr(*left);
                self.walk_expr(*right);
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
                // Untyped ObjectLit — only recurse into fields (no
                // closure-typed signal available without surrounding
                // LetDecl context).
                for (_, feid) in fields {
                    self.walk_expr(*feid);
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

    /// `const o: T = { k: v, ... }` where `T` resolves to a known
    /// TypeDecl whose field `k` is fn-typed and `v` is a bare
    /// top-FnDecl Ident.
    fn collect_objectlit_field_sites(&mut self, init: ExprId, type_ann: Option<&str>) {
        let Some(ann) = type_ann else { return };
        let Some(field_anns) = self.struct_field_anns.get(ann.trim()) else {
            return;
        };
        if let Expr::ObjectLit { fields } = self.ast.get_expr(init) {
            for (fname, feid) in fields.clone() {
                if let Some(fann) = field_anns.get(&fname) {
                    // RFC 20260710 C5 — an optional fn field
                    // (`__nullable(__cls(...))` after the retag
                    // pass) is the same Closure-repr slot; a bare
                    // named-fn init needs the forwarder wrap too.
                    let inner = fann
                        .strip_prefix("__nullable(")
                        .and_then(|r| r.strip_suffix(')'))
                        .unwrap_or(fann);
                    if is_fn_like_ann(inner) {
                        self.try_mark(feid);
                    }
                }
            }
        }
    }

    /// Chunk 733 — mark every bare top-FnDecl Ident element of an
    /// array literal destined for a fn-typed array slot (let-init /
    /// call-arg positions).
    fn mark_array_lit_elems(&mut self, eid: ExprId) {
        if let Expr::Array(els) = self.ast.get_expr(eid) {
            for e in els.clone() {
                self.try_mark(e);
            }
        }
    }

    /// `const f: any = top_fn` (chunk 518) and ObjectLit-field /
    /// Array-element positions inside the `any`-destined init
    /// (chunk 519) — see module doc for why the wrap is needed.
    fn collect_any_init_sites(&mut self, eid: ExprId) {
        if self.try_mark(eid) {
            return;
        }
        match self.ast.get_expr(eid) {
            Expr::ObjectLit { fields } => {
                for (_, feid) in fields.clone() {
                    self.collect_any_init_sites(feid);
                }
            }
            Expr::Array(els) => {
                for e in els.clone() {
                    self.collect_any_init_sites(e);
                }
            }
            _ => {}
        }
    }
}
