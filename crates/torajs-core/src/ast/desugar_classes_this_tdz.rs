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
//! raises there. "May run ahead" is judged by statement order inside
//! each list ([`order`]): what follows a `super(…);` statement in its
//! own block reads a bound `this` and takes no check.
//!
//! Two phases, because the passes between them rewrite what this one
//! has to find. [`plan`] runs on the class index before it freezes,
//! while the constructor's statements can still be edited and every
//! `super` is still the parser's node. [`apply`] runs after the
//! super-call and super-property rewrites (they mint the receiver
//! `this` of `super.m()` / `super.x`, which reads the binding too) and
//! before Pass 2 turns `this` into `__this`.

pub(super) mod order;

use super::clone_body::BodyCloner;
use super::desugar_async_sm_this::owns_this;
use super::desugar_classes_super::ClassIndexEntry;
use super::desugar_classes_this_tdz_exit::{has_own_return, route_returns};
use super::desugar_with::walk;
use super::super_collect::collect_super_in_stmt;
use super::super_collect_prop::{
    SUPER_BASE_MARKER, SuperPropSite, SuperPropSites, collect_superprop_in_expr,
};
use super::{Ast, BinOp, ClassCtor, Expr, ExprId, Stmt, UnaryOp};
use order::{binds, parser_site, unbound_roots};
use std::collections::HashSet;
use std::ops::Range;

pub(super) const FLAG: &str = "__this_bound";
const RAISER: &str = "__torajs_this_binding_throw";

/// A constructor that carries the binding flag: what may run before
/// `super(…)` has bound `this`, the super property sites among it, and
/// every `super(…)` site with the arguments the parser gave it.
pub(super) struct FlaggedCtor {
    early_roots: Vec<ExprId>,
    early_props: Vec<ExprId>,
    sites: Vec<(ExprId, Vec<ExprId>)>,
}

/// `__torajs_this_binding_throw(bound)` — the raiser for a read of an
/// uninitialized binding (`bound == false`) or a second binding.
pub(super) fn raise_call(ast: &mut Ast, bound: bool) -> ExprId {
    let callee = ast.add_expr(Expr::Ident(RAISER.to_string()));
    let arg = ast.add_expr(Expr::Bool(bound));
    ast.add_expr(Expr::Call {
        callee,
        args: vec![arg],
    })
}

/// `__this_bound || raise(false)`
pub(super) fn read_check(ast: &mut Ast) -> ExprId {
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
        // the implicit constructor (§15.7.14) calls `super` first; what
        // the desugar wrote into one is field initializers
        if !ast.explicit_ctor_classes.contains(cname.as_str()) {
            continue;
        }
        // a builtin parent's `super(…)` sites are already rewritten
        // away (`strip_builtin_heritage`); only a body that never
        // calls it, and so ends in the raiser, is still decidable here
        let builtin = parent.is_none() && ast.builtin_class_parents.contains_key(cname.as_str());
        if parent.is_none() && !(builtin && ctor.as_ref().is_some_and(|c| ends_in_raise(ast, c))) {
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

/// Does the body end in the no-super raiser `strip_builtin_heritage`
/// appends to a constructor with no `super(…)` site?
fn ends_in_raise(ast: &Ast, c: &ClassCtor) -> bool {
    let Some(Stmt::Expr(e)) = c.body.last() else {
        return false;
    };
    let Expr::Call { callee, args } = ast.get_expr(*e) else {
        return false;
    };
    matches!(ast.get_expr(*callee), Expr::Ident(n) if n == RAISER)
        && matches!(args.as_slice(), [a] if matches!(ast.get_expr(*a), Expr::Bool(false)))
}

fn plan_ctor(ast: &mut Ast, cname: &str, c: &mut ClassCtor) -> Option<FlaggedCtor> {
    let mut site_list: Vec<(ExprId, Vec<ExprId>)> = Vec::new();
    for s in &c.body {
        collect_super_in_stmt(ast, s, &mut site_list);
    }
    let first = c.body.iter().position(|s| binds(ast, s, &parser_site));
    // without a top-level `super(…)` statement the parser put the field
    // initializers at the head; they are the desugar's, not reads the
    // program wrote ahead of the binding
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
    let touches = early_exprs
        .iter()
        .chain(&unbound_roots(ast, &c.body[early.clone()], &parser_site))
        .any(|e| expr_touches_this(ast, *e));
    let early_return = has_own_return(ast, &c.body[early.clone()]);
    let one_leading_super = site_list.len() == 1 && first.is_some();
    if !touches && !early_return && (one_leading_super || site_list.is_empty()) {
        return None;
    }
    // a body with no `super` at all already ends in the raiser
    // (`append_no_super_throw`); one whose `super` is not a top-level
    // statement may reach its end without having run it
    let falls_off = first.is_none() && !site_list.is_empty();
    let early = install_flag(ast, cname, c, early, falls_off);
    let mut early_roots = early_exprs;
    early_roots.extend(unbound_roots(ast, &c.body[early], &parser_site));
    // a super property's own rewrite may read the base alone, with no
    // `this` for the read check to stand on; §13.3.7.1 still asks
    // GetThisBinding first, so the whole site takes the check
    let mut props = SuperPropSites::default();
    for e in &early_roots {
        collect_superprop_in_expr(ast, *e, &mut props);
    }
    Some(FlaggedCtor {
        early_roots,
        early_props: props.sites.iter().map(prop_site_root).collect(),
        sites: site_list,
    })
}

/// Declare the flag at the head of the body, route every early
/// `return` through the exit check (and check at the end when
/// `falls_off`), and answer where the early statements sit now.
fn install_flag(
    ast: &mut Ast,
    cname: &str,
    c: &mut ClassCtor,
    early: Range<usize>,
    falls_off: bool,
) -> Range<usize> {
    route_returns(ast, &mut c.body[early.clone()]);
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
    if falls_off {
        let check = read_check(ast);
        c.body.push(Stmt::Expr(check));
    }
    early.start + 1..early.end + 1
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

/// Route every early `this` of each flagged constructor through the
/// read check and make every `super(…)` site bind the flag.
pub(super) fn apply(ast: &mut Ast, flagged: Vec<FlaggedCtor>) {
    for fc in flagged {
        let mut reads: Vec<ExprId> = Vec::new();
        for e in &fc.early_roots {
            collect_this(ast, *e, &fc.sites, &mut reads);
        }
        // a `delete` operand keeps its reference shape for the
        // lowering's raise; only a computed key runs ahead of it
        let props = fc.early_props.into_iter().filter_map(|id| {
            if !ast.super_delete_operands.contains(&id) {
                return Some(id);
            }
            match ast.get_expr(id) {
                Expr::Index { index, .. } => Some(*index),
                _ => None,
            }
        });
        let props: Vec<ExprId> = props.collect();
        let mut seen: HashSet<ExprId> = HashSet::new();
        for id in reads.into_iter().chain(props) {
            if seen.insert(id) {
                check_before(ast, id);
            }
        }
        for (site, args) in &fc.sites {
            bind_at(ast, *site, args);
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

/// `super(a, …)` →
/// `(((!__this_bound || (a', …, raise(true))), super(a, …)), __this_bound = true), this)`.
///
/// §13.3.7.1 evaluates the arguments (step 4) and constructs the parent
/// (step 6) before BindThisValue refuses a second binding (step 8). An
/// unbound call takes the right arm of the `||` as before; a bound one
/// evaluates `a'`, a copy of the arguments (a spread one into an array,
/// which iterates it as the argument list would), and raises without
/// calling the parent. tr's parent constructor runs on the one instance
/// the factory minted, so letting the call through would re-initialize
/// the bound `this`; the fresh instance step 6 constructs is not built.
fn bind_at(ast: &mut Ast, site: ExprId, args: &[ExprId]) {
    let call = ast.add_expr(ast.exprs[site.0 as usize].clone());
    let flag = ast.add_expr(Expr::Ident(FLAG.into()));
    let unbound = ast.add_expr(Expr::Unary {
        op: UnaryOp::Not,
        expr: flag,
    });
    let mut cloner = BodyCloner::new(ast);
    let copies: Vec<ExprId> = args.iter().map(|a| cloner.clone_expr(*a)).collect();
    super::clone_body_tables::migrate(&mut cloner);
    let mut raise = raise_call(ast, true);
    for copy in copies.into_iter().rev() {
        let left = if matches!(ast.get_expr(copy), Expr::Spread { .. }) {
            ast.add_expr(Expr::Array(vec![copy]))
        } else {
            copy
        };
        raise = ast.add_expr(Expr::Sequence { left, right: raise });
    }
    let refuse = ast.add_expr(Expr::BinOp {
        op: BinOp::LOr,
        left: unbound,
        right: raise,
    });
    let run = ast.add_expr(Expr::Sequence {
        left: refuse,
        right: call,
    });
    let target = ast.add_expr(Expr::Ident(FLAG.into()));
    let value = ast.add_expr(Expr::Bool(true));
    let bind = ast.add_expr(Expr::Assign { target, value });
    let left = ast.add_expr(Expr::Sequence {
        left: run,
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
            !binds_own_this(ast, eid)
                && unbound_roots(ast, body, &parser_site)
                    .into_iter()
                    .any(|e| expr_touches_this(ast, e))
        }
        _ => walk::expr_children(ast, eid)
            .into_iter()
            .any(|c| expr_touches_this(ast, c)),
    }
}

fn binds_own_this(ast: &Ast, eid: ExprId) -> bool {
    owns_this(ast, eid) || ast.gen_fn_exprs.contains_key(&eid)
}

/// The `this` nodes under `eid` that read the constructor's binding. A
/// `super(…)` site is entered only through the arguments it was written
/// with: the receiver its rewrite passes the parent constructor is the
/// binding being initialized, not a read of it. An arrow's body is read
/// in order, like the constructor's own.
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
                let is_site = |_: &Ast, e: ExprId| sites.iter().any(|(id, _)| *id == e);
                for r in unbound_roots(ast, body, &is_site) {
                    collect_this(ast, r, sites, out);
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
