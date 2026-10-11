//! The derived constructor's this-binding before `super()` returns.
//!
//! §10.2.1.2 OrdinaryCallBindThis leaves a derived constructor's
//! this-binding uninitialized; §13.3.7.1 binds it when `super(…)`
//! returns, and only then. Until that point every GetThisBinding
//! (§9.1.1.3.4) — a `this` read, a super property access, §10.2.2
//! step 15's implicit `return this` — throws a ReferenceError, and a
//! second binding (§9.1.1.3.1 BindThisValue) throws one too.
//!
//! tr mints the instance before the constructor runs, so `this` always
//! has an object behind it; what is missing is the binding state. The
//! common constructor needs none: its first top-level statement that
//! is a `super(…)` call runs before every `this` it holds, nothing ahead
//! of that statement (parameter defaults included) touches `this`, and
//! it calls `super` nowhere else. Such a constructor is left exactly as
//! it was. Every other one gets a boolean `__this_bound` local: each
//! `this` that may run ahead of the binding reads it first, each
//! `super(…)` sets it (and raises when it was already set), and a body
//! that can fall off its end or `return;` without having called `super`
//! raises there.
//!
//! Two phases, because the passes between them rewrite what this one
//! has to find. [`plan`] runs on the class index before it freezes,
//! while the constructor's statements can still be edited and every
//! `super` is still the parser's node. [`apply`] runs after the
//! super-call and super-property rewrites (they mint the receiver
//! `this` of `super.m()` / `super.x`, which reads the binding too) and
//! before Pass 2 turns `this` into `__this`.

use super::desugar_async_sm_this::owns_this;
use super::desugar_classes_super::ClassIndexEntry;
use super::desugar_with::walk;
use super::super_collect::collect_super_in_stmt;
use super::super_collect_prop::{
    SUPER_BASE_MARKER, SuperPropSite, SuperPropSites, collect_superprop_in_expr,
    collect_superprop_in_stmt,
};
use super::{Ast, BinOp, ClassCtor, Expr, ExprId, Stmt, UnaryOp};
use std::collections::HashSet;
use std::ops::Range;

pub(super) const FLAG: &str = "__this_bound";

/// A constructor that carries the binding flag: what may run before
/// `super(…)` has bound `this`, the super property sites among it, and
/// every `super(…)` site with the arguments the parser gave it.
pub(super) struct FlaggedCtor {
    early_stmts: Vec<Stmt>,
    early_exprs: Vec<ExprId>,
    early_props: Vec<ExprId>,
    sites: Vec<(ExprId, Vec<ExprId>)>,
}

/// `__torajs_this_binding_throw(bound)` — the raiser for a read of an
/// uninitialized binding (`bound == false`) or a second binding.
pub(super) fn raise_call(ast: &mut Ast, bound: bool) -> ExprId {
    let callee = ast.add_expr(Expr::Ident("__torajs_this_binding_throw".to_string()));
    let arg = ast.add_expr(Expr::Bool(bound));
    ast.add_expr(Expr::Call {
        callee,
        args: vec![arg],
    })
}

/// `__this_bound || raise(false)`
fn read_check(ast: &mut Ast) -> ExprId {
    let flag = ast.add_expr(Expr::Ident(FLAG.into()));
    let raise = raise_call(ast, false);
    ast.add_expr(Expr::BinOp {
        op: BinOp::LOr,
        left: flag,
        right: raise,
    })
}

/// Decide which derived constructors need the flag and give them its
/// declaration, the fall-off check and the `return;` checks.
pub(super) fn plan(ast: &mut Ast, class_index: &mut [ClassIndexEntry]) -> Vec<FlaggedCtor> {
    let mut out = Vec::new();
    for (_, cname, _tp, parent, _, _, ctor, _, _, _) in class_index.iter_mut() {
        // The implicit constructor (§15.7.14) calls `super` first; what
        // the desugar wrote into one is field initializers.
        if parent.is_none() || !ast.explicit_ctor_classes.contains(cname.as_str()) {
            continue;
        }
        if let Some(c) = ctor.as_mut()
            && let Some(fc) = plan_ctor(ast, cname, c)
        {
            out.push(fc);
        }
    }
    out
}

fn plan_ctor(ast: &mut Ast, cname: &str, c: &mut ClassCtor) -> Option<FlaggedCtor> {
    let mut site_list: Vec<(ExprId, Vec<ExprId>)> = Vec::new();
    for s in &c.body {
        collect_super_in_stmt(ast, s, &mut site_list);
    }
    let first = c
        .body
        .iter()
        .position(|s| matches!(s, Stmt::Expr(e) if matches!(ast.get_expr(*e), Expr::Super { .. })));
    // Without a top-level `super(…)` statement the parser put the field
    // initializers at the head; they are the desugar's, not reads the
    // program wrote ahead of the binding.
    let early = match first {
        Some(k) => 0..k,
        None => ast.ctor_field_init_end.get(cname).copied().unwrap_or(0)..c.body.len(),
    };
    let mut early_exprs: Vec<ExprId> = c.params.iter().filter_map(|p| p.default).collect();
    if let Some(k) = first
        && let Stmt::Expr(e) = &c.body[k]
        && let Expr::Super { args } = ast.get_expr(*e)
    {
        early_exprs.extend(args.iter().copied());
    }
    let touches = early_exprs.iter().any(|e| expr_touches_this(ast, *e))
        || c.body[early.clone()]
            .iter()
            .any(|s| stmt_touches_this(ast, s));
    let early_return = c.body[early.clone()].iter().any(has_bare_return);
    let one_leading_super = site_list.len() == 1 && first.is_some();
    if !touches && !early_return && (one_leading_super || site_list.is_empty()) {
        return None;
    }
    // A body with no `super` at all already ends in the raiser
    // (`append_no_super_throw`); one whose `super` is not a top-level
    // statement may reach its end without having run it.
    let falls_off = first.is_none() && !site_list.is_empty();
    let early = install_flag(ast, cname, c, early, falls_off);
    // A super property's own rewrite may read the base alone, with no
    // `this` for the read check to stand on; §13.3.7.1 still asks
    // GetThisBinding first, so the whole site takes the check.
    let mut props = SuperPropSites::default();
    for s in &c.body[early.clone()] {
        collect_superprop_in_stmt(ast, s, &mut props);
    }
    for e in &early_exprs {
        collect_superprop_in_expr(ast, *e, &mut props);
    }
    Some(FlaggedCtor {
        early_stmts: c.body[early].to_vec(),
        early_exprs,
        early_props: props.sites.iter().map(prop_site_root).collect(),
        sites: site_list,
    })
}

/// Declare the flag at the head of the body, check it at every early
/// `return;` (and at the end when `falls_off`), and answer where the
/// early statements sit now.
fn install_flag(
    ast: &mut Ast,
    cname: &str,
    c: &mut ClassCtor,
    early: Range<usize>,
    falls_off: bool,
) -> Range<usize> {
    let init = ast.add_expr(Expr::Bool(false));
    c.body.insert(
        0,
        Stmt::LetDecl {
            mutable: true,
            name: FLAG.into(),
            type_ann: Some("boolean".into()),
            init,
            is_var: false,
        },
    );
    if let Some(end) = ast.ctor_field_init_end.get_mut(cname) {
        *end += 1;
    }
    let early = early.start + 1..early.end + 1;
    for s in &mut c.body[early.clone()] {
        guard_bare_returns(ast, s);
    }
    if falls_off {
        let check = read_check(ast);
        c.body.push(Stmt::Expr(check));
    }
    early
}

/// The node a super property site's rewrite replaces in place.
fn prop_site_root(site: &SuperPropSite) -> ExprId {
    match site {
        SuperPropSite::Read { member, .. } => *member,
        SuperPropSite::IndexRead { index_expr } => *index_expr,
        SuperPropSite::AssignName { assign, .. } | SuperPropSite::AssignIndex { assign, .. } => {
            *assign
        }
        SuperPropSite::CallOpt { call, .. } | SuperPropSite::CallIndex { call, .. } => *call,
        SuperPropSite::Update { node, .. } => *node,
    }
}

fn has_bare_return(s: &Stmt) -> bool {
    matches!(s, Stmt::Return(None)) || walk::stmt_children_ref(s).into_iter().any(has_bare_return)
}

/// Put the read check in front of every `return;` of the constructor's
/// own frame: §10.2.2 step 15 answers the this-binding for it. Not one
/// a `finally` follows — step 15 reads the binding once that has run,
/// and it may be the code that calls `super`.
fn guard_bare_returns(ast: &mut Ast, s: &mut Stmt) {
    if matches!(s, Stmt::Return(None)) {
        let check = read_check(ast);
        *s = Stmt::Multi(vec![Stmt::Expr(check), Stmt::Return(None)]);
        return;
    }
    // a nested declaration's `return;` leaves its own frame
    if matches!(s, Stmt::FnDecl { .. } | Stmt::ClassDecl { .. }) {
        return;
    }
    if let Stmt::Try {
        finally_body: Some(fin),
        ..
    } = s
    {
        for f in fin {
            guard_bare_returns(ast, f);
        }
        return;
    }
    for child in walk::stmt_children(s) {
        guard_bare_returns(ast, child);
    }
}

/// Route every early `this` of each flagged constructor through the
/// read check and make every `super(…)` site bind the flag.
pub(super) fn apply(ast: &mut Ast, flagged: Vec<FlaggedCtor>) {
    for fc in flagged {
        let mut reads: Vec<ExprId> = Vec::new();
        for e in &fc.early_exprs {
            collect_this(ast, *e, &fc.sites, &mut reads);
        }
        for s in &fc.early_stmts {
            collect_this_stmt(ast, s, &fc.sites, &mut reads);
        }
        let mut seen: HashSet<ExprId> = HashSet::new();
        for id in reads.into_iter().chain(fc.early_props) {
            if seen.insert(id) {
                check_before(ast, id);
            }
        }
        for (site, _) in &fc.sites {
            bind_at(ast, *site);
        }
    }
}

/// `e` → `(__this_bound || raise(false), e)`, in place.
fn check_before(ast: &mut Ast, id: ExprId) {
    let moved = ast.add_expr(ast.exprs[id.0 as usize].clone());
    let check = read_check(ast);
    ast.exprs[id.0 as usize] = Expr::Sequence {
        left: check,
        right: moved,
    };
}

/// `super(…)` → `((super(…), __this_bound = !__this_bound || raise(true)), this)`
/// — the call, then §13.3.7.1 step 7 BindThisValue, then step 9's
/// answer.
fn bind_at(ast: &mut Ast, site: ExprId) {
    let call = ast.add_expr(ast.exprs[site.0 as usize].clone());
    let flag = ast.add_expr(Expr::Ident(FLAG.into()));
    let unbound = ast.add_expr(Expr::Unary {
        op: UnaryOp::Not,
        expr: flag,
    });
    let raise = raise_call(ast, true);
    let value = ast.add_expr(Expr::BinOp {
        op: BinOp::LOr,
        left: unbound,
        right: raise,
    });
    let target = ast.add_expr(Expr::Ident(FLAG.into()));
    let bind = ast.add_expr(Expr::Assign { target, value });
    let left = ast.add_expr(Expr::Sequence {
        left: call,
        right: bind,
    });
    let right = ast.add_expr(Expr::This);
    ast.exprs[site.0 as usize] = Expr::Sequence { left, right };
}

/// Does `eid` read the constructor's own this-binding — a `this`, or a
/// super property / super method marker — through arrows but not into
/// a function that binds its own?
fn expr_touches_this(ast: &Ast, eid: ExprId) -> bool {
    match ast.get_expr(eid) {
        Expr::This => true,
        Expr::Ident(n) => n == SUPER_BASE_MARKER || n.starts_with("__supercall__"),
        Expr::ArrowFn { body, .. } => {
            !binds_own_this(ast, eid) && body.iter().any(|s| stmt_touches_this(ast, s))
        }
        _ => walk::expr_children(ast, eid)
            .into_iter()
            .any(|c| expr_touches_this(ast, c)),
    }
}

fn stmt_touches_this(ast: &Ast, s: &Stmt) -> bool {
    walk::stmt_exprs(s)
        .into_iter()
        .any(|e| expr_touches_this(ast, e))
        || walk::stmt_children_ref(s)
            .into_iter()
            .any(|c| stmt_touches_this(ast, c))
}

fn binds_own_this(ast: &Ast, eid: ExprId) -> bool {
    owns_this(ast, eid) || ast.gen_fn_exprs.contains_key(&eid)
}

/// The `this` nodes under `eid` that read the constructor's binding. A
/// `super(…)` site is entered only through the arguments it was written
/// with: the receiver its rewrite passes the parent constructor is the
/// binding being initialized, not a read of it.
fn collect_this(ast: &Ast, eid: ExprId, sites: &[(ExprId, Vec<ExprId>)], out: &mut Vec<ExprId>) {
    if let Some((_, args)) = sites.iter().find(|(id, _)| *id == eid) {
        for a in args {
            collect_this(ast, *a, sites, out);
        }
        return;
    }
    match ast.get_expr(eid) {
        Expr::This => out.push(eid),
        Expr::ArrowFn { body, .. } => {
            if !binds_own_this(ast, eid) {
                for s in body {
                    collect_this_stmt(ast, s, sites, out);
                }
            }
        }
        _ => {
            for c in walk::expr_children(ast, eid) {
                collect_this(ast, c, sites, out);
            }
        }
    }
}

fn collect_this_stmt(ast: &Ast, s: &Stmt, sites: &[(ExprId, Vec<ExprId>)], out: &mut Vec<ExprId>) {
    for e in walk::stmt_exprs(s) {
        collect_this(ast, e, sites, out);
    }
    for c in walk::stmt_children_ref(s) {
        collect_this_stmt(ast, c, sites, out);
    }
}
