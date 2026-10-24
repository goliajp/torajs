//! Name resolution for the constraint walk, including the names a
//! lifted closure reads from its creator.
//!
//! A lifted `__closure_N` body still spells its captures as plain
//! identifiers, but the scope that declared them is gone. Resolving
//! such a name by spelling alone picks the wrong slot whenever the name
//! is shadowed (a function parameter `offset` beside a top-level
//! `offset`) and no slot at all when the capture is not top-level; an
//! unresolved read then contributed no width, so `(x) => x + delta`
//! kept an integer return slot while `delta` carried 0.5 and the sum was
//! truncated. The binding is still recoverable at the `Closure`
//! construction site, which sits in the creator's body and names its
//! captures — so each capture resolves there, in the creator's scope.

use super::{Analysis, Scope, SlotKeyId, SlotKeys, W, let_names};
use crate::ast::desugar_async_sm_walk::{visit_expr_children, visit_stmt_exprs};
use crate::ast::{Ast, Expr, ExprId, Stmt};
use std::collections::{HashMap, HashSet};

/// Lifted fn → captured name → the creator-side binding it reads.
/// `None` marks a capture whose binding could not be pinned to one
/// slot (unresolvable in the creator, or two creators disagreeing).
pub(super) type CaptureMap = HashMap<String, HashMap<String, Option<SlotKeyId>>>;

/// The slot `n` names inside `fn_name` (`""` = the top level): the
/// fn's own param, else its own local, else the binding its creator
/// handed it, else the top-level binding. A capture that resolved to
/// no single slot answers `None` rather than falling through to a
/// same-spelled top-level binding it may not be.
fn resolve_name(
    keys: &SlotKeys,
    fn_name: &str,
    params: &HashSet<String>,
    locals: &HashSet<String>,
    toplevel_lets: &HashSet<String>,
    captures: &CaptureMap,
    n: &str,
) -> Option<SlotKeyId> {
    if !fn_name.is_empty() {
        if params.contains(n) {
            return Some(keys.param(fn_name, n));
        }
        if locals.contains(n) {
            return Some(keys.local(fn_name, n));
        }
        if let Some(c) = captures.get(fn_name).and_then(|m| m.get(n)) {
            return *c;
        }
    }
    toplevel_lets.contains(n).then(|| keys.global(n))
}

/// One body expressions can sit in, with the names it binds: a
/// top-level fn (lifted closures included) or the top level (`""`).
pub(super) struct Body<'s> {
    pub(super) name: String,
    pub(super) params: HashSet<String>,
    pub(super) locals: HashSet<String>,
    stmts: Vec<&'s Stmt>,
}

impl Body<'_> {
    pub(super) fn resolve(
        &self,
        keys: &SlotKeys,
        toplevel_lets: &HashSet<String>,
        captures: &CaptureMap,
        n: &str,
    ) -> Option<SlotKeyId> {
        resolve_name(
            keys,
            &self.name,
            &self.params,
            &self.locals,
            toplevel_lets,
            captures,
            n,
        )
    }

    pub(super) fn stmts(&self) -> impl Iterator<Item = &Stmt> {
        self.stmts.iter().copied()
    }

    /// `n` is a `let` of this fn's own frame: a fresh binding per call.
    pub(super) fn binds_frame_local(&self, n: &str) -> bool {
        !self.params.contains(n) && self.locals.contains(n)
    }

    /// Every expression the body evaluates in its own frame.
    pub(super) fn each_expr(&self, ast: &Ast, f: &mut dyn FnMut(ExprId)) {
        fn visit(ast: &Ast, eid: ExprId, f: &mut dyn FnMut(ExprId)) {
            f(eid);
            visit_expr_children(ast, eid, &mut |c| visit(ast, c, f));
        }
        for s in &self.stmts {
            visit_stmt_exprs(s, &mut |e| visit(ast, e, f));
        }
    }

    /// Every binding the body declares, with its declaration.
    pub(super) fn each_binding(&self, f: let_names::BindingVisit) {
        for s in &self.stmts {
            let_names::walk_bindings(s, f);
        }
    }

    /// The key a binding of `n` declared in this body lives under.
    pub(super) fn binding_key(&self, keys: &SlotKeys, n: &str) -> SlotKeyId {
        keys.binding(&self.name, n)
    }
}

pub(super) fn bodies(ast: &Ast) -> Vec<Body<'_>> {
    let mut top = Body {
        name: String::new(),
        params: HashSet::new(),
        locals: HashSet::new(),
        stmts: Vec::new(),
    };
    let mut out = Vec::new();
    for stmt in &ast.stmts {
        let Stmt::FnDecl {
            name, params, body, ..
        } = stmt
        else {
            top.stmts.push(stmt);
            continue;
        };
        let mut locals = HashSet::new();
        for b in body {
            let_names::collect_let_names(b, &mut locals);
        }
        out.push(Body {
            name: name.clone(),
            params: params.iter().map(|p| p.name.clone()).collect(),
            locals,
            stmts: body.iter().collect(),
        });
    }
    out.push(top);
    out
}

/// Resolve every capture at its construction site. A closure built
/// inside another lifted closure resolves through that one's map, so
/// the pass repeats until no entry changes (once per nesting level).
pub(super) fn collect_captures(
    ast: &Ast,
    bodies: &[Body<'_>],
    toplevel_lets: &HashSet<String>,
    keys: &SlotKeys,
) -> CaptureMap {
    let mut sites: Vec<(&Body<'_>, &String, &Vec<String>)> = Vec::new();
    for b in bodies {
        b.each_expr(ast, &mut |eid| {
            if let Expr::Closure { fn_name, captures } = ast.get_expr(eid) {
                sites.push((b, fn_name, captures));
            }
        });
    }
    let mut caps = CaptureMap::new();
    let mut passes = 0;
    loop {
        // one pass per closure nesting level
        passes += 1;
        debug_assert!(passes <= sites.len() + 2, "capture fixpoint diverged");
        let mut next = CaptureMap::new();
        for (b, cf, names) in &sites {
            let m = next.entry((*cf).clone()).or_default();
            for n in names.iter() {
                let r = b.resolve(keys, toplevel_lets, &caps, n);
                let e = m.entry(n.clone()).or_insert(r);
                if *e != r {
                    *e = None;
                }
            }
        }
        // a generator's iterator method reads its captures through the
        // env of the wrapper closure, so they are the wrapper's
        for (m, info) in &ast.gen_env_methods {
            if let Some(w) = next.get(&info.wrapper_fn_name(ast)) {
                let reads = (w.iter()).filter(|(n, _)| info.names.contains(n));
                let reads = reads.map(|(n, k)| (n.clone(), *k)).collect();
                next.insert(m.clone(), reads);
            }
        }
        if next == caps {
            return caps;
        }
        caps = next;
    }
}

impl<'a> Analysis<'a> {
    pub(super) fn resolve(&self, n: &str, scope: &Scope) -> Option<SlotKeyId> {
        resolve_name(
            &self.keys,
            scope.fn_name,
            &scope.params,
            &scope.locals,
            &self.toplevel_lets,
            &self.captures,
            n,
        )
    }

    /// Width of a read of `n` that [`Self::resolve`] could not pin to a
    /// slot. Outside synthetic bodies that is a non-binding name
    /// (`Math`, a fn). Inside one it may still be an outer binding, so
    /// it joins every same-named slot — and a capture with no slot of
    /// that name at all is taken as fractional, since an integer answer
    /// here would truncate whatever the binding really holds.
    pub(super) fn unresolved_read(&self, n: &str, scope: &Scope) -> W {
        // `undefined` in a `number` slot's own spelling (a generator's
        // completion value, an async tail): only an f64 slot has the
        // sentinel bits for it, an integer one reads back 0
        if n.strip_prefix(crate::ast::UNDEF_SLOT_MARKER) == Some("number") {
            return W::F64;
        }
        if !scope.fn_name.starts_with("__") {
            return W::NotNum;
        }
        if let Some(keys) = self.named(n) {
            return W::Num(keys.iter().map(|k| (*k, false)).collect());
        }
        let captured = self
            .captures
            .get(scope.fn_name)
            .is_some_and(|m| m.contains_key(n));
        // a captured function or class is not a number
        let named_decl = self.fn_params.contains_key(n) || self.classes.iter().any(|c| c == n);
        if captured && !named_decl {
            W::F64
        } else {
            W::NotNum
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{SlotKey, WidthTable, analyze};
    use crate::{lexer, parser};
    use std::collections::HashMap;

    fn table(src: &str) -> WidthTable {
        let tokens = lexer::tokenize(src).expect("lex");
        let mut ast = parser::parse(src, &tokens).expect("parse");
        crate::ast::lift_arrow_fns(&mut ast);
        analyze(
            &ast,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        )
    }

    fn ret(f: &str) -> SlotKey {
        SlotKey::Ret(f.into())
    }

    #[test]
    fn captured_param_reaches_closure_ret() {
        let t = table(
            "function f(xs: number[], delta: number): number {\n  const add = (x: number): number => x + delta;\n  return add(xs[0]);\n}\nconsole.log(f([1], 0.5));",
        );
        assert!(t.slot_is_f64(&ret("__closure_0")));
    }

    #[test]
    fn capture_resolves_past_same_named_toplevel() {
        // the top-level `offset` stays integral; the closure reads the
        // parameter, which a call site fed 0.5
        let t = table(
            "const offset = 3;\nfunction f(offset: number): number {\n  const add = (x: number): number => x + offset;\n  return add(1);\n}\nconsole.log(f(0.5) + offset);",
        );
        assert!(t.slot_is_f64(&ret("__closure_0")));
        assert!(!t.slot_is_f64(&SlotKey::Global("offset".into())));
    }

    #[test]
    fn same_name_captures_keep_their_own_bindings() {
        let t = table(
            "function a(k: number): number {\n  const g = (): number => k + 1;\n  return g();\n}\nfunction b(k: number): number {\n  const h = (): number => k + 1;\n  return h();\n}\nconsole.log(a(2), b(0.5));",
        );
        assert!(!t.slot_is_f64(&ret("__closure_0")));
        assert!(t.slot_is_f64(&ret("__closure_1")));
    }

    #[test]
    fn closure_write_reaches_the_captured_let() {
        let t = table(
            "function f(n: number): number {\n  let acc = 0;\n  const bump = (): void => { acc = acc + 0.5; };\n  bump();\n  return acc + n;\n}\nconsole.log(f(1));",
        );
        assert!(t.slot_is_f64(&SlotKey::Local("f".into(), "acc".into())));
        assert!(t.slot_is_f64(&ret("f")));
    }

    #[test]
    fn sibling_block_closures_share_their_slots() {
        // both consts live under one `Local(f, cb)` key, so a read of
        // either may hand over either closure
        let t = table(
            "function f(xs: number[]): void {\n  {\n    const cb = (v: number): number => v + 0.5;\n    console.log(xs.map(cb));\n  }\n  {\n    const cb = (v: number): number => v + 1;\n    console.log(xs.map(cb));\n  }\n}\nf([1]);",
        );
        assert!(t.slot_is_f64(&ret("__closure_0")));
        assert!(t.slot_is_f64(&ret("__closure_1")));
    }

    #[test]
    fn nested_closure_resolves_through_its_creator() {
        let t = table(
            "function f(d: number): number {\n  const outer = (): number => {\n    const inner = (x: number): number => x + d;\n    return inner(1);\n  };\n  return outer();\n}\nconsole.log(f(0.5));",
        );
        let lifted: Vec<SlotKey> = (0..2).map(|i| ret(&format!("__closure_{i}"))).collect();
        assert!(lifted.iter().all(|k| t.slot_is_f64(k)));
    }
}
