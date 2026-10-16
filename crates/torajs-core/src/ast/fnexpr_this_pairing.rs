//! 399-03 — the SCOPE-PAIRED census for a multiply-declared name.
//!
//! Knife 2's `decls.len() != 1` guard exists because a by-name Ident
//! walk cannot tell which declaration a use belongs to — but the
//! refusal is program-wide, so six scopes each declaring their own
//! `const g = function () { …this… }` all lost promotion, and the one
//! whose scope had no `__this` turned into a loud reject: the same
//! shape answered differently in a small program and a large one.
//!
//! This module pairs every reachable `Ident(name)` use to the
//! `LetDecl` that lexically binds it, mirroring the scope rules the
//! free-variable walk ([`super::free_vars`]) already encodes: a
//! statement list is a block scope (a `let`/`const` binds the WHOLE
//! block — the pre-declaration span is a TDZ error at runtime, never
//! a different binding), `Stmt::Multi` shares its surrounding scope
//! (it is a desugar expansion, not a block), and a function body is a
//! scope of its own. A use in it with no declaration in scope belongs
//! to the binding a closure captured into that lifted body (followed
//! out through nested captures), or, in a top-level function, to the
//! top-level declaration; anything else fails the pairing and the
//! name keeps today's loud reject.
//!
//! The caller (`try_promote_scope_paired` in
//! [`super::fnexpr_this_routed`]) runs knife 2's full per-binding
//! proof on every group holding a fn-expr that reads `this` and
//! promotes all of them or none; the uses of every other group are
//! recorded so the name-keyed consumers leave them alone.
//!
//! The `__cmany_` / `__smany_` twin clones need no special casing
//! here: a twin body is an ordinary `FnDecl` to this walk, so its
//! cloned declaration becomes its own group, is proven with everyone
//! else, and gets its own `FacePatch` — which is exactly the
//! "guard counts sources, promotion patches copies" contract
//! `collect_decls_split` documents, reached by a different route.

use super::{Expr, ExprId, Param, Stmt, SwitchCase};

mod captures;

use captures::{Capture, direct_decl_inits};

/// One declaration of the name, with every reachable use that
/// lexically binds to it (tree order, deduplicated).
pub(super) struct DeclGroup {
    /// `None` for a function parameter of the name.
    pub(super) init: Option<ExprId>,
    pub(super) uses: Vec<ExprId>,
}

/// Pair every reachable use of `name` to its binding declaration.
/// `None` means the census could not prove a pairing — a use with no
/// dominating declaration (a nested function reading an outer local
/// it did not capture), a list declaring the name twice, one lifted
/// body captured from two places, a shared Ident node
/// reached under two different bindings (the face pre-pass clones
/// Calls with leaf ExprIds shared), or a `UsingDecl` — and the caller
/// keeps the loud reject.
///
/// The walk visits the statement TREE, not the expression arena: a
/// node no statement reaches is never executed, so it cannot witness
/// an unsafe use. (The by-name single-decl path scans the arena flat,
/// which is strictly more conservative; this walk is the half that
/// needs positions.)
pub(super) fn pair_decls_scoped<'a>(
    stmts: &'a [Stmt],
    exprs: &'a [Expr],
    name: &'a str,
) -> Option<Vec<DeclGroup>> {
    let mut p = Pairing {
        exprs,
        name,
        groups: Vec::new(),
        decl_inits: std::collections::HashSet::new(),
        owner: std::collections::HashMap::new(),
        failed: false,
        cur_fn: None,
        unbound: Vec::new(),
        captured_by: std::collections::HashMap::new(),
    };
    p.walk_scope_list(stmts, None);
    // a lifted closure body only ever reads what its closure captured
    let lifted: std::collections::HashSet<&str> = exprs
        .iter()
        .filter_map(|e| match e {
            Expr::Closure { fn_name, .. } => Some(fn_name.as_str()),
            _ => None,
        })
        .collect();
    let top_fns: std::collections::HashSet<&str> = stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::FnDecl { name, .. } if !lifted.contains(name.as_str()) => Some(name.as_str()),
            _ => None,
        })
        .collect();
    let mut top_inits = Vec::new();
    direct_decl_inits(stmts, name, &mut top_inits);
    let top_group = match top_inits.as_slice() {
        [i] => p.groups.iter().position(|d| d.init == Some(*i)),
        _ => None,
    };
    p.bind_captured_uses(&top_fns, top_group);
    if p.failed { None } else { Some(p.groups) }
}

struct Pairing<'a> {
    exprs: &'a [Expr],
    name: &'a str,
    groups: Vec<DeclGroup>,
    /// Init ExprIds of every declaration a scope pre-scan registered —
    /// a `LetDecl` the statement walk meets that is NOT in here sits
    /// in a position the pre-scan cannot see (e.g. a bare declaration
    /// as an `if` branch), so the census is incomplete and fails.
    decl_inits: std::collections::HashSet<ExprId>,
    /// Which group each use ExprId resolved to — the same node reached
    /// under two different bindings fails the pairing.
    owner: std::collections::HashMap<ExprId, usize>,
    failed: bool,
    /// The function body being walked, while no declaration of the
    /// name is in scope inside it.
    cur_fn: Option<&'a str>,
    /// Uses met with no declaration in scope, by the body they sit in:
    /// a lifted closure body reads a captured outer binding this way.
    unbound: Vec<(&'a str, ExprId)>,
    /// Lifted body name -> where its captured `name` comes from.
    captured_by: std::collections::HashMap<&'a str, Capture<'a>>,
}

impl<'a> Pairing<'a> {
    /// Pre-scan a list's DIRECT declarations of the name (through
    /// `Multi`, which shares the surrounding scope; never through a
    /// `Block` or a function body). Zero hits inherit the outer
    /// binding; exactly one opens a new group that binds the whole
    /// list; two or more fail.
    fn scan_direct_decls(&mut self, list: &'a [Stmt], found: &mut Vec<usize>) {
        for s in list {
            match s {
                Stmt::LetDecl { name, init, .. } if name == self.name => {
                    self.groups.push(DeclGroup {
                        init: Some(*init),
                        uses: Vec::new(),
                    });
                    self.decl_inits.insert(*init);
                    found.push(self.groups.len() - 1);
                }
                Stmt::UsingDecl { name, .. } if name == self.name => {
                    self.failed = true;
                }
                Stmt::Multi(inner) => self.scan_direct_decls(inner, found),
                _ => {}
            }
        }
    }

    /// A block-scope statement list: pre-scan its own declaration,
    /// then walk under whichever binding governs.
    fn walk_scope_list(&mut self, list: &'a [Stmt], cur: Option<usize>) {
        let mut found = Vec::new();
        self.scan_direct_decls(list, &mut found);
        let cur = match found.len() {
            0 => cur,
            1 => Some(found[0]),
            _ => {
                self.failed = true;
                return;
            }
        };
        for s in list {
            self.walk_stmt(s, cur);
        }
    }

    fn walk_stmt(&mut self, s: &'a Stmt, cur: Option<usize>) {
        if self.failed {
            return;
        }
        match s {
            Stmt::Expr(eid) | Stmt::Return(Some(eid)) | Stmt::Yield(eid) | Stmt::Throw(eid) => {
                self.walk_expr(*eid, cur)
            }
            Stmt::YieldInto { value, .. } => self.walk_expr(*value, cur),
            Stmt::Return(None) | Stmt::Break(_) | Stmt::Continue(_) => {}
            Stmt::LetDecl { name, init, .. } => {
                // The group was opened by the enclosing pre-scan; a
                // same-name declaration the pre-scan could not see
                // (a bare decl as a branch body) fails the census.
                if name == self.name && !self.decl_inits.contains(init) {
                    self.failed = true;
                    return;
                }
                self.walk_expr(*init, cur);
            }
            Stmt::UsingDecl { init, .. } => self.walk_expr(*init, cur),
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.walk_expr(*cond, cur);
                self.walk_stmt(then_branch, cur);
                if let Some(eb) = else_branch {
                    self.walk_stmt(eb, cur);
                }
            }
            Stmt::While { cond, body } => {
                self.walk_expr(*cond, cur);
                self.walk_stmt(body, cur);
            }
            Stmt::DoWhile { body, cond } => {
                self.walk_stmt(body, cur);
                self.walk_expr(*cond, cur);
            }
            Stmt::Switch {
                scrutinee,
                cases,
                default,
                ..
            } => self.walk_switch(*scrutinee, cases, default.as_deref(), cur),
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => {
                // A for-head declaration scopes over the whole `for`
                // (§14.7.4) — pre-scan the init statement alone.
                let mut found = Vec::new();
                if let Some(i) = init {
                    self.scan_direct_decls(std::slice::from_ref(i), &mut found);
                }
                let cur = match found.len() {
                    0 => cur,
                    1 => Some(found[0]),
                    _ => {
                        self.failed = true;
                        return;
                    }
                };
                if let Some(i) = init {
                    self.walk_stmt(i, cur);
                }
                if let Some(c) = cond {
                    self.walk_expr(*c, cur);
                }
                if let Some(st) = step {
                    self.walk_expr(*st, cur);
                }
                self.walk_stmt(body, cur);
            }
            Stmt::Block(inner) => self.walk_scope_list(inner, cur),
            Stmt::Multi(inner) => {
                // Same surrounding scope — its declarations were
                // already registered by the enclosing pre-scan.
                for st in inner {
                    self.walk_stmt(st, cur);
                }
            }
            Stmt::ForOfSplitIter {
                parent, sep, body, ..
            } => {
                // The loop variable shadowing `name` is already
                // rejected by `name_shadowed_elsewhere` before the
                // pairing runs (same for ForOf and catch params).
                self.walk_expr(*parent, cur);
                self.walk_expr(*sep, cur);
                self.walk_stmt(body, cur);
            }
            Stmt::ForOf {
                elem_expr, body, ..
            } => {
                self.walk_expr(*elem_expr, cur);
                self.walk_stmt(body, cur);
            }
            Stmt::Labeled { body, .. } => self.walk_stmt(body, cur),
            Stmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                self.walk_scope_list(body, cur);
                self.walk_scope_list(catch_body, cur);
                if let Some(fb) = finally_body {
                    self.walk_scope_list(fb, cur);
                }
            }
            // A function body is its own scope: the outer binding does
            // NOT thread in (a nested function reaches an outer local
            // through closure capture, which this walk cannot pair).
            Stmt::FnDecl {
                name, params, body, ..
            } => self.walk_fn_decl(name, params, body),
            Stmt::TypeDecl { .. } | Stmt::ImportDecl { .. } => {}
            // Classes are desugared before the fn-expr knives run; a
            // surviving ClassDecl would hide uses from this walk, so
            // its presence voids the census outright.
            Stmt::ClassDecl { .. } => self.failed = true,
            Stmt::ExportDecl { inner, .. } => {
                if let Some(inner) = inner {
                    self.walk_stmt(inner, cur);
                }
            }
        }
    }

    fn walk_switch(
        &mut self,
        scrutinee: ExprId,
        cases: &'a [SwitchCase],
        default: Option<&'a [Stmt]>,
        cur: Option<usize>,
    ) {
        self.walk_expr(scrutinee, cur);
        // The whole CaseBlock is one scope (§14.12.4): a declaration in
        // one clause binds the selectors and every other clause too.
        let mut found = Vec::new();
        for c in cases {
            self.scan_direct_decls(&c.body, &mut found);
        }
        if let Some(db) = default {
            self.scan_direct_decls(db, &mut found);
        }
        let cur = match found.len() {
            0 => cur,
            1 => Some(found[0]),
            _ => {
                self.failed = true;
                return;
            }
        };
        for c in cases {
            self.walk_expr(c.value, cur);
            for s in &c.body {
                self.walk_stmt(s, cur);
            }
        }
        for s in default.unwrap_or_default() {
            self.walk_stmt(s, cur);
        }
    }

    fn walk_fn_decl(&mut self, name: &'a str, params: &'a [Param], body: &'a [Stmt]) {
        let outer = self.cur_fn.replace(name);
        let param = params.iter().any(|p| p.name == self.name).then(|| {
            self.groups.push(DeclGroup {
                init: None,
                uses: Vec::new(),
            });
            self.groups.len() - 1
        });
        // a `var` of a parameter's name is the same binding,
        // which a list scope would split in two
        let mut redecl = Vec::new();
        direct_decl_inits(body, self.name, &mut redecl);
        if param.is_some() && !redecl.is_empty() {
            self.failed = true;
        }
        self.walk_scope_list(body, param);
        self.cur_fn = outer;
    }

    fn record_use(&mut self, eid: ExprId, cur: Option<usize>) {
        let Some(g) = cur else {
            // No declaration in scope: either a lifted body reading a
            // captured binding (resolved once every capture is known)
            // or an unpaired use.
            match self.cur_fn {
                Some(f) => self.unbound.push((f, eid)),
                None => self.failed = true,
            }
            return;
        };
        match self.owner.get(&eid) {
            Some(prev) if *prev != g => self.failed = true,
            Some(_) => {}
            None => {
                self.owner.insert(eid, g);
                self.groups[g].uses.push(eid);
            }
        }
    }

    fn walk_expr(&mut self, eid: ExprId, cur: Option<usize>) {
        if self.failed {
            return;
        }
        let exprs: &'a [Expr] = self.exprs;
        match &exprs[eid.0 as usize] {
            Expr::Ident(n) => {
                if n == self.name {
                    self.record_use(eid, cur);
                }
            }
            Expr::Elision
            | Expr::String(_)
            | Expr::Number(_)
            | Expr::BigInt { .. }
            | Expr::Bool(_)
            | Expr::Null
            | Expr::Uninit
            | Expr::Regex { .. }
            | Expr::This
            | Expr::NewTarget => {}
            Expr::BinOp { left, right, .. }
            | Expr::Sequence { left, right }
            | Expr::Nullish {
                lhs: left,
                rhs: right,
            } => {
                self.walk_expr(*left, cur);
                self.walk_expr(*right, cur);
            }
            Expr::Unary { expr, .. }
            | Expr::TypeOf { expr }
            | Expr::Delete { expr }
            | Expr::Spread { expr }
            | Expr::As { expr, .. } => self.walk_expr(*expr, cur),
            Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => self.walk_expr(*obj, cur),
            Expr::Call { callee, args } | Expr::OptCall { callee, args } => {
                self.walk_expr(*callee, cur);
                for a in args {
                    self.walk_expr(*a, cur);
                }
            }
            Expr::Assign { target, value } => {
                self.walk_expr(*target, cur);
                self.walk_expr(*value, cur);
            }
            Expr::Index { obj, index } | Expr::OptIndex { obj, index } => {
                self.walk_expr(*obj, cur);
                self.walk_expr(*index, cur);
            }
            Expr::Array(elems) => {
                for e in elems {
                    self.walk_expr(*e, cur);
                }
            }
            Expr::ObjectLit { fields } => {
                for (_, e) in fields {
                    self.walk_expr(*e, cur);
                }
            }
            // An un-lifted arrow body is a function scope of its own
            // (the knives run after the lift, so this arm is a
            // conservative leftover-guard, not a hot path).
            Expr::ArrowFn { body, .. } => {
                let outer = self.cur_fn.take();
                self.walk_scope_list(body, None);
                self.cur_fn = outer;
            }
            Expr::Closure { fn_name, captures } => {
                if captures.iter().any(|c| c == self.name) {
                    self.capture(fn_name, cur);
                }
            }
            Expr::New { args, .. } | Expr::Super { args } => {
                for a in args {
                    self.walk_expr(*a, cur);
                }
            }
            Expr::NewDynamic { callee, args } => {
                self.walk_expr(*callee, cur);
                for a in args {
                    self.walk_expr(*a, cur);
                }
            }
            Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            } => {
                self.walk_expr(*cond, cur);
                self.walk_expr(*then_branch, cur);
                self.walk_expr(*else_branch, cur);
            }
            Expr::InstanceOf { expr, rhs } => {
                self.walk_expr(*expr, cur);
                self.walk_expr(*rhs, cur);
            }
            Expr::PostIncr { target, .. } => self.walk_expr(*target, cur),
        }
    }
}
