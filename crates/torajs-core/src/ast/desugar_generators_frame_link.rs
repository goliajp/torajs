//! Frame link — a generator expression nested in ANOTHER generator's
//! body and capturing that generator's local by reference (642-05).
//!
//! RFC 20260912's by-reference route binds the captured names from the
//! wrapper closure's env: the wrapper reads the name, so its env holds
//! the name's capture box, and the iterator methods bind from that
//! layout (`GenEnvInfo`). That is right while the enclosing scope is a
//! plain function. When the encloser is itself a generator — an async
//! body blade 2 moved, holding an async value — its locals are about to
//! become FIELDS of its own iterator (`prep_generator_body` rewrites
//! `order` to `this.order`), and the wrapper's read follows: it
//! captures the encloser's `this`, not `order`, and the nested body's
//! `order` is left naming nothing ("unknown identifier `order`", then a
//! ReferenceError).
//!
//! The textbook answer is the static link: the nested generator's frame
//! carries a handle on the enclosing frame, and the enclosing local is
//! read through it. Here the enclosing frame IS the encloser's iterator
//! object, so the nested factory gains a leading parameter (`__genouter_
//! <wrapper eid>`, typed as the encloser's iterator class) that the
//! wrapper fills with `this`, the nested generator's own prep moves it
//! onto a field like any parameter, and every read or write of the name
//! in the nested body becomes `<handle>.<name>` — the same field the
//! encloser's own arrows reach through `this.<name>`, so a write from
//! either side lands on one storage and the object holds the lifetime.
//!
//! The link composes: a wrapper nested TWO generators down reads the
//! handle instead of the name (its by-reference record is re-keyed to
//! the handle), and the middle generator's own prep then links that
//! handle the same way — `outer` is reached as `<h2>.<h1>.<name>`.
//!
//! Ordering: the link edits the nested factory while it is still a
//! `function*` declaration, so the driver desugars a container BEFORE
//! anything it contains ([`container_first_order`]); the factory's
//! parameters are re-read from the tree at that point, not from a
//! snapshot taken before the container ran.

use super::desugar_with::walk;
use super::{Ast, Expr, ExprId, Param, Stmt};
use std::collections::HashMap;

/// Wrapper arena slot → the generator it wraps, for every
/// by-reference record. A generator's any twin shares its original's
/// record and wrapper; the wrapper wraps the original.
pub(super) fn wrappers_by_eid(ast: &Ast) -> HashMap<ExprId, String> {
    ast.gen_env_captures
        .iter()
        .filter(|(g, _)| !super::is_generator_any_twin_decl(g))
        .map(|(g, info)| (info.wrapper, g.clone()))
        .collect()
}

/// Every by-reference wrapper arrow reachable from `body`, arrow bodies
/// included (a blade-2 wrapper sits inside the driver thunk's arrow).
pub(super) fn nested_wrappers(
    ast: &Ast,
    body: &[Stmt],
    by_eid: &HashMap<ExprId, String>,
    out: &mut Vec<(ExprId, String)>,
) {
    let mut stack: Vec<&Stmt> = body.iter().rev().collect();
    while let Some(s) = stack.pop() {
        for a in walk::arrow_nodes(ast, s) {
            if let Some(g) = by_eid.get(&a) {
                out.push((a, g.clone()));
            }
            if let Expr::ArrowFn { body, .. } = ast.get_expr(a) {
                stack.extend(body.iter().rev());
            }
        }
        stack.extend(walk::stmt_children_ref(s).into_iter().rev());
    }
}

/// The order the driver desugars `gens` (declaration-order positions,
/// each `(stmt index, name)`) in: a generator before every generator
/// whose by-reference wrapper its body holds, declaration order among
/// the rest. Kahn's algorithm over the containment edges; the tree
/// shape makes a cycle impossible.
pub(super) fn container_first_order(ast: &Ast, gens: &[(usize, String)]) -> Vec<usize> {
    let by_eid = wrappers_by_eid(ast);
    let pos: HashMap<&str, usize> = gens
        .iter()
        .enumerate()
        .map(|(k, (_, n))| (n.as_str(), k))
        .collect();
    let mut contained: Vec<Vec<usize>> = vec![Vec::new(); gens.len()];
    let mut indegree = vec![0usize; gens.len()];
    if !by_eid.is_empty() {
        for (k, (idx, _)) in gens.iter().enumerate() {
            let Stmt::FnDecl { body, .. } = &ast.stmts[*idx] else {
                continue;
            };
            let mut ws = Vec::new();
            nested_wrappers(ast, body, &by_eid, &mut ws);
            for (_, g) in ws {
                if let Some(&j) = pos.get(g.as_str()) {
                    contained[k].push(j);
                    indegree[j] += 1;
                }
            }
        }
    }
    let mut out = Vec::with_capacity(gens.len());
    let mut done = vec![false; gens.len()];
    while out.len() < gens.len() {
        let Some(k) = (0..gens.len()).find(|&k| !done[k] && indegree[k] == 0) else {
            panic!("generator frame links form a cycle — a generator cannot contain itself");
        };
        done[k] = true;
        out.push(k);
        for &j in &contained[k] {
            indegree[j] -= 1;
        }
    }
    out
}

/// `prep_generator_body` hook — for every by-reference wrapper nested
/// in `gen_body` whose captured names include one of `fields` (what
/// this generator is about to move onto `this`): hand the nested
/// factory this generator's frame and re-spell those names in the
/// nested body as `<handle>.<name>`. Runs BEFORE the container's own
/// `this.<name>` rewrite, which then finds nothing of these left.
pub(super) fn link_nested_gen_envs(
    ast: &mut Ast,
    gen_body: &[Stmt],
    gen_name: &str,
    fields: &[Param],
) {
    let by_eid = wrappers_by_eid(ast);
    if by_eid.is_empty() {
        return;
    }
    let mut ws = Vec::new();
    nested_wrappers(ast, gen_body, &by_eid, &mut ws);
    for (weid, inner) in ws {
        let info = ast.gen_env_captures[&inner].clone();
        let linked: Vec<String> = info
            .names
            .iter()
            .filter(|n| fields.iter().any(|p| &p.name == *n))
            .cloned()
            .collect();
        if linked.is_empty() {
            continue;
        }
        let handle = format!("__genouter_{}", weid.0);
        let frame_ty = format!("__Gen_{gen_name}");
        insert_factory_param(ast, &inner, &info.cell_field, &handle, &frame_ty);
        // The wrapper's `this` is the container's iterator only while
        // every function between them is an arrow. A function
        // EXPRESSION or an object-literal method on the way (the
        // blade-2 wrapper of an async value spelled `function`, or a
        // nested async declaration `desugar_async_nested_fns` turned
        // into one) binds its own `this` — the receiver of whoever
        // calls the value, which is the container's iterator only by
        // the accident of `this.f()`, and an `any` even then (the
        // value is a recv-first closure): the factory's typed frame
        // parameter got an any-widened clone that stored the NaN-box
        // into the iterator-typed field, and the nested machine's
        // `next()` looped in the method dispatch (RFC 20260915 —
        // `async function f() { seen += "t"; await null; }` inside a
        // moved body, `seen` a local of that body). The frame is
        // therefore captured lexically OUTSIDE the outermost such
        // owner: `((__gouterv_N: <frame class>) => <owner>)(this)`,
        // and the wrapper reads the parameter.
        let frame_src = match this_owner_between(ast, gen_body, weid) {
            Some(owner) => {
                let local = format!("__gouterv_{}", weid.0);
                capture_frame_outside_owner(ast, owner, &local, &frame_ty);
                ast.add_expr(Expr::Ident(local))
            }
            None => ast.add_expr(Expr::This),
        };
        pass_frame_from_wrapper(ast, weid, &info.cell_field, &linked, frame_src);
        ast.gen_env_captures
            .get_mut(&inner)
            .expect("the record was cloned from this map")
            .names
            .retain(|n| !linked.contains(n));
        for n in &linked {
            relink(ast, &inner, n, &handle);
        }
    }
}

/// The nested factory's `(cell, <handle>, …)`: the handle rides right
/// behind the cell, the position `pass_frame_from_wrapper` fills.
fn insert_factory_param(ast: &mut Ast, gen_name: &str, cell: &str, handle: &str, frame_ty: &str) {
    let decl = ast
        .stmts
        .iter_mut()
        .find(|s| matches!(s, Stmt::FnDecl { name, is_generator: true, .. } if name == gen_name));
    let Some(Stmt::FnDecl { params, .. }) = decl else {
        panic!(
            "generator `{gen_name}` was desugared before its container — the driver must take \
             containers first (container_first_order)"
        );
    };
    assert_eq!(
        params.first().map(|p| p.name.as_str()),
        Some(cell),
        "a by-reference generator's first parameter is its wrapper cell"
    );
    params.insert(
        1,
        Param {
            name: handle.to_string(),
            type_ann: Some(frame_ty.to_string()),
            default: None,
            is_rest: false,
        },
    );
}

/// The outermost `this`-owning arrow-slot expression (a function
/// expression or an object-literal method — `owns_this`) on the path
/// from `body` down to the wrapper `weid`, if any: the one whose
/// `this` a `This` in the wrapper would bind to instead of the
/// container's.
fn this_owner_between(ast: &Ast, body: &[Stmt], weid: ExprId) -> Option<ExprId> {
    fn in_stmts(ast: &Ast, stmts: &[Stmt], weid: ExprId, chain: &mut Vec<ExprId>) -> bool {
        stmts.iter().any(|s| in_stmt(ast, s, weid, chain))
    }
    fn in_stmt(ast: &Ast, s: &Stmt, weid: ExprId, chain: &mut Vec<ExprId>) -> bool {
        for a in walk::arrow_nodes(ast, s) {
            if a == weid {
                return true;
            }
            let Expr::ArrowFn { body, .. } = ast.get_expr(a) else {
                continue;
            };
            chain.push(a);
            if in_stmts(ast, body, weid, chain) {
                return true;
            }
            chain.pop();
        }
        walk::stmt_children_ref(s)
            .into_iter()
            .any(|c| in_stmt(ast, c, weid, chain))
    }
    let mut chain = Vec::new();
    if !in_stmts(ast, body, weid, &mut chain) {
        return None;
    }
    chain
        .into_iter()
        .find(|&a| super::desugar_async_sm_this::owns_this(ast, a))
}

/// `<owner>` (in its arena slot, so every reference to it holds) becomes
/// `((<local>: <frame_ty>) => <owner'>)(this)`, `<owner'>` the same
/// expression moved to a fresh slot — the memberships that name a
/// function-expression node move with it.
fn capture_frame_outside_owner(ast: &mut Ast, owner: ExprId, local: &str, frame_ty: &str) {
    let taken = std::mem::replace(&mut ast.exprs[owner.0 as usize], Expr::This);
    let moved = ast.add_expr(taken);
    ast.expr_spans[moved.0 as usize] = ast.expr_spans[owner.0 as usize];
    for set in [
        &mut ast.fn_expr_exprs,
        &mut ast.objlit_method_exprs,
        &mut ast.fnexpr_recv_faces,
        &mut ast.async_fn_value_exprs,
        &mut ast.async_sm_value_exprs,
        &mut ast.async_ssa.value_exprs,
    ] {
        if set.remove(&owner) {
            set.insert(moved);
        }
    }
    if let Some(n) = ast.fn_expr_self_names.remove(&owner) {
        ast.fn_expr_self_names.insert(moved, n);
    }
    let arrow = ast.add_expr(Expr::ArrowFn {
        params: vec![Param {
            name: local.to_string(),
            type_ann: Some(frame_ty.to_string()),
            default: None,
            is_rest: false,
        }],
        return_type: None,
        body: vec![Stmt::Return(Some(moved))],
    });
    // The machine's `next` is checked in both its receiver spellings
    // (`__cm_` typed, `__cmany_` any); the cast is a no-op in the
    // typed one that runs and keeps the any one well-typed.
    let this = ast.add_expr(Expr::This);
    let typed = ast.add_expr(Expr::As {
        expr: this,
        ty_ann: frame_ty.to_string(),
    });
    ast.exprs[owner.0 as usize] = Expr::Call {
        callee: arrow,
        args: vec![typed],
    };
}

/// The wrapper `(p) => { <reads>; return __genexpr_N(cell, p) }` becomes
/// `(p) => { <reads minus linked>; return __genexpr_N(cell, <frame>, p) }`
/// — `frame` is `this` (the container's iterator once `desugar_classes`
/// has given the method its receiver; the arrow's `this` is lexical)
/// or the local a `this`-owner on the way was wrapped to bind.
fn pass_frame_from_wrapper(
    ast: &mut Ast,
    weid: ExprId,
    cell: &str,
    linked: &[String],
    frame: ExprId,
) {
    let this = frame;
    // Read everything first; the arena is borrowed mutably below.
    let Expr::ArrowFn { body, .. } = ast.get_expr(weid) else {
        panic!("gen-env wrapper {weid:?} is not an arrow (lift_arrow_fns must not have run)");
    };
    let keep: Vec<bool> = body
        .iter()
        .map(|s| {
            !matches!(s, Stmt::Expr(e)
                if matches!(ast.get_expr(*e), Expr::Ident(n) if linked.contains(n)))
        })
        .collect();
    let Some(Stmt::Return(Some(call))) = body.last() else {
        panic!("gen-env wrapper {weid:?} does not end in its factory call");
    };
    let call = *call;
    let Expr::Call { args, .. } = ast.get_expr(call) else {
        panic!("gen-env wrapper {weid:?} returns something other than its factory call");
    };
    assert!(
        matches!(args.first().map(|a| ast.get_expr(*a)), Some(Expr::Ident(n)) if n == cell),
        "a by-reference wrapper's first argument is its own cell"
    );
    if let Expr::ArrowFn { body, .. } = &mut ast.exprs[weid.0 as usize] {
        let mut i = 0;
        body.retain(|_| {
            let k = keep[i];
            i += 1;
            k
        });
    }
    if let Expr::Call { args, .. } = &mut ast.exprs[call.0 as usize] {
        args.insert(1, this);
    }
}

/// In generator `gen`'s body every `Ident(from)` becomes
/// `<handle>.<from>` — and a by-reference wrapper nested in THAT body
/// that captured `from` now captures the handle instead: its record is
/// re-keyed and its own generator re-spelled the same way, so the
/// middle generator's prep links the handle when its turn comes.
fn relink(ast: &mut Ast, gen_name: &str, from: &str, handle: &str) {
    let body = ast
        .stmts
        .iter()
        .find_map(|s| match s {
            Stmt::FnDecl {
                name,
                body,
                is_generator: true,
                ..
            } if name == gen_name => Some(body.clone()),
            _ => None,
        })
        .unwrap_or_else(|| {
            panic!("generator `{gen_name}` was desugared before its container (frame link)")
        });
    super::desugar_generators_rewrite::rewrite_ident_to_frame_field(ast, &body, from, handle);
    let by_eid = wrappers_by_eid(ast);
    let mut ws = Vec::new();
    nested_wrappers(ast, &body, &by_eid, &mut ws);
    for (_, g2) in ws {
        let names = &mut ast
            .gen_env_captures
            .get_mut(&g2)
            .expect("wrappers_by_eid was built from this map")
            .names;
        let Some(p) = names.iter().position(|n| n == from) else {
            continue;
        };
        if names.iter().any(|n| n == handle) {
            names.remove(p);
        } else {
            names[p] = handle.to_string();
        }
        relink(ast, &g2, from, handle);
    }
}
