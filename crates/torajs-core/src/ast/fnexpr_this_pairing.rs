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
//! scope of its own that the pairing deliberately does NOT thread the
//! outer binding into — a nested function reads an outer local
//! through the closure-capture machinery, which a syntactic walk
//! cannot express, so such a use fails the pairing and the name keeps
//! today's loud reject.
//!
//! The caller (`try_promote_scope_paired` in
//! [`super::fnexpr_this_routed`]) runs knife 2's full per-binding
//! proof on EVERY group and promotes all or none: the downstream
//! consumers — `fnexpr_recv_locals` and the direct-call `undefined`
//! seeding — are name-keyed, so promoting one binding of a name while
//! another stays unpromoted would shift argv on call sites whose
//! runtime value is the unpromoted closure. All-or-nothing keeps the
//! name-keyed consumers sound without threading declaration identity
//! through them.
//!
//! The `__cmany_` / `__smany_` twin clones need no special casing
//! here: a twin body is an ordinary `FnDecl` to this walk, so its
//! cloned declaration becomes its own group, is proven with everyone
//! else, and gets its own `FacePatch` — which is exactly the
//! "guard counts sources, promotion patches copies" contract
//! `collect_decls_split` documents, reached by a different route.

use super::{Expr, ExprId, Stmt};

/// One declaration of the name, with every reachable use that
/// lexically binds to it (tree order, deduplicated).
pub(super) struct DeclGroup {
    pub(super) init: ExprId,
    pub(super) uses: Vec<ExprId>,
}

/// Pair every reachable use of `name` to its binding declaration.
/// `None` means the census could not prove a pairing — a use with no
/// dominating declaration (a nested function reading an outer local),
/// a list declaring the name twice, a closure CAPTURING the name (the
/// escape a syntactic walk cannot follow), a shared Ident node
/// reached under two different bindings (the face pre-pass clones
/// Calls with leaf ExprIds shared), or a `UsingDecl` — and the caller
/// keeps the loud reject.
///
/// The walk visits the statement TREE, not the expression arena: a
/// node no statement reaches is never executed, so it cannot witness
/// an unsafe use. (The by-name single-decl path scans the arena flat,
/// which is strictly more conservative; this walk is the half that
/// needs positions.)
pub(super) fn pair_decls_scoped(
    stmts: &[Stmt],
    exprs: &[Expr],
    name: &str,
) -> Option<Vec<DeclGroup>> {
    let mut p = Pairing {
        exprs,
        name,
        groups: Vec::new(),
        decl_inits: std::collections::HashSet::new(),
        owner: std::collections::HashMap::new(),
        failed: false,
    };
    p.walk_scope_list(stmts, None);
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
}

impl Pairing<'_> {
    /// Pre-scan a list's DIRECT declarations of the name (through
    /// `Multi`, which shares the surrounding scope; never through a
    /// `Block` or a function body). Zero hits inherit the outer
    /// binding; exactly one opens a new group that binds the whole
    /// list; two or more fail.
    fn scan_direct_decls(&mut self, list: &[Stmt], found: &mut Vec<usize>) {
        for s in list {
            match s {
                Stmt::LetDecl { name, init, .. } if name == self.name => {
                    self.groups.push(DeclGroup {
                        init: *init,
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
    fn walk_scope_list(&mut self, list: &[Stmt], cur: Option<usize>) {
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

    fn walk_stmt(&mut self, s: &Stmt, cur: Option<usize>) {
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
            } => {
                self.walk_expr(*scrutinee, cur);
                for c in cases {
                    self.walk_expr(c.value, cur);
                    self.walk_scope_list(&c.body, cur);
                }
                if let Some(db) = default {
                    self.walk_scope_list(db, cur);
                }
            }
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
            Stmt::FnDecl { body, .. } => self.walk_scope_list(body, None),
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

    fn record_use(&mut self, eid: ExprId, cur: Option<usize>) {
        let Some(g) = cur else {
            // No dominating declaration — an unpaired use.
            self.failed = true;
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
        match &self.exprs[eid.0 as usize] {
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
            Expr::ArrowFn { body, .. } => self.walk_scope_list(body, None),
            Expr::Closure { captures, .. } => {
                // A closure CAPTURING the name consumes the binding
                // through machinery this walk cannot pair — fail.
                if captures.iter().any(|c| c == self.name) {
                    self.failed = true;
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
