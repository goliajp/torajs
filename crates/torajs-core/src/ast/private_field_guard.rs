//! RFC 20260916-private-element-brand slice 2b — where a private
//! FIELD access has to check that the field is already added
//! (§7.3.31 / §7.3.32 step 1, PrivateElementFind answering empty).
//!
//! An object reaches code outside its class's construction only once
//! every constructor of its chain returned, and by then each private
//! field of the chain is installed. Inside the class's own constructor
//! the fields are all installed once the last add marker ran. So an
//! access needs the check only where that can still be false:
//!
//! - inside the class's initializer prefix, up to its last add marker
//!   (the initializers run there, before later fields are added), and
//! - anywhere else, when `this` escapes from that prefix or from any
//!   ancestor's constructor — a method called on it, anything read
//!   that is not a declared field, `this` passed along or captured, a
//!   `super` property, a direct `eval`. Every other access stays the
//!   unchecked layout read it was.
//!
//! The initializer write that precedes a field's own install is not an
//! access and stays unchecked.
//!
//! A unit class (RFC 20260917-class-eval-unit K3) adds one more site:
//! an access whose receiver is not `this`. Two evaluations of the
//! class share a layout, so `o.#x` on the other evaluation's instance
//! would read the right slot — and §7.3.31 says it must throw. The
//! check compares the receiver's evaluation with the one `this` runs
//! in; a `this.#x` needs none, the running instance is its own
//! evaluation.

use super::desugar_classes_super::ClassIndexEntry;
use super::desugar_with::walk::{expr_children, stmt_children_ref, stmt_exprs};
use super::{Ast, ClassCtor, Expr, ExprId, Stmt};
use std::collections::{HashMap, HashSet};

/// One constructor's initializer prefix, whether `this` escapes from
/// the prefix / from the whole constructor, and the initializer writes
/// that add a field.
struct CtorScan {
    prefix_ids: HashSet<ExprId>,
    prefix_escapes: bool,
    escapes: bool,
    init_writes: HashSet<ExprId>,
}

pub(super) fn guard_field_sites(
    ast: &mut Ast,
    class_index: &[ClassIndexEntry],
    field_pos: &HashMap<String, (String, u32)>,
) {
    if field_pos.is_empty() {
        return;
    }
    let parent: HashMap<&str, Option<&str>> = class_index
        .iter()
        .map(|e| (e.1.as_str(), e.3.as_deref()))
        .collect();
    let chain = |c: &str| {
        let mut v = vec![c.to_string()];
        while let Some(Some(p)) = parent.get(v.last().unwrap().as_str()) {
            v.push(p.to_string());
        }
        v
    };
    let mut scans: HashMap<&str, CtorScan> = HashMap::new();
    for e in class_index {
        let Some(ctor) = e.6.as_ref() else { continue };
        let names: HashSet<&str> = chain(&e.1)
            .iter()
            .filter_map(|c| class_index.iter().find(|x| &x.1 == c))
            .flat_map(|x| x.4.iter().filter_map(|(k, _)| k.as_str()))
            .collect();
        scans.insert(e.1.as_str(), scan_ctor(ast, ctor, &names));
    }
    let sites: Vec<(usize, String, u32)> = ast
        .exprs
        .iter()
        .enumerate()
        .filter_map(|(i, e)| match e {
            Expr::Member { name, .. } => field_pos.get(name).map(|(c, p)| (i, c.clone(), *p)),
            _ => None,
        })
        .collect();
    for (i, cname, pos) in sites {
        let id = ExprId(i as u32);
        let own = scans.get(cname.as_str());
        if own.is_some_and(|s| s.init_writes.contains(&id)) {
            continue;
        }
        let in_prefix = own.is_some_and(|s| s.prefix_ids.contains(&id) || s.prefix_escapes);
        let ancestor_escapes = chain(&cname)
            .iter()
            .skip(1)
            .any(|c| scans.get(c.as_str()).is_some_and(|s| s.escapes));
        let other_evaluation = ast.eval_unit_classes.contains(&cname) && !receiver_is_this(ast, id);
        if in_prefix || ancestor_escapes || other_evaluation {
            super::private_brand::guard_receiver(ast, i, &cname, pos);
        }
    }
}

/// Whether the member access `id` reads or writes on `this` — the
/// parser's `Expr::This`, or the `__this` an earlier class rewrite
/// left in a body.
fn receiver_is_this(ast: &Ast, id: ExprId) -> bool {
    let Expr::Member { obj, .. } = ast.get_expr(id) else {
        return false;
    };
    match ast.get_expr(*obj) {
        Expr::This => true,
        Expr::Ident(n) => n == "__this",
        _ => false,
    }
}

/// Markers that install together: within one constructor's top-level
/// statements, a run of add markers separated only by initializer writes
/// of a literal value runs no user code between its installs, so the
/// last marker installs the whole run (`Some(range)`) and the others
/// fold away (`None`). A class declaring thousands of fields
/// (`test262 identifiers/start-unicode-*-class`) otherwise grows a
/// constructor with one install per field.
pub(super) fn coalesced_installs(
    ast: &Ast,
    class_index: &[ClassIndexEntry],
    field_pos: &HashMap<String, (String, u32)>,
) -> HashMap<ExprId, Option<(u32, u32)>> {
    let mut out = HashMap::new();
    for e in class_index {
        let Some(ctor) = e.6.as_ref() else { continue };
        let mut run: Vec<(ExprId, u32)> = Vec::new();
        for s in &ctor.body {
            let marker = match s {
                Stmt::Expr(eid) => added_field(ast, *eid)
                    .filter(|_| super::private_brand::is_field_add(ast, *eid))
                    .and_then(|n| field_pos.get(&n).map(|(_, p)| (*eid, *p))),
                _ => None,
            };
            match marker {
                Some((eid, pos)) if run.last().is_none_or(|(_, p)| *p + 1 == pos) => {
                    run.push((eid, pos));
                }
                Some(m) => {
                    close_run(&mut run, &mut out);
                    run.push(m);
                }
                None if literal_init(ast, s) => {}
                None => close_run(&mut run, &mut out),
            }
        }
        close_run(&mut run, &mut out);
    }
    out
}

fn close_run(run: &mut Vec<(ExprId, u32)>, out: &mut HashMap<ExprId, Option<(u32, u32)>>) {
    if run.len() > 1 {
        let (first, last) = (run[0].1, run[run.len() - 1].1);
        for (eid, _) in &run[..run.len() - 1] {
            out.insert(*eid, None);
        }
        out.insert(run[run.len() - 1].0, Some((first, last + 1)));
    }
    run.clear();
}

/// `this.<f> = <literal>` — an initializer that runs no user code.
fn literal_init(ast: &Ast, s: &Stmt) -> bool {
    let Stmt::Expr(eid) = s else { return false };
    let Expr::Assign { target, value } = ast.get_expr(*eid) else {
        return false;
    };
    let this_member = matches!(
        ast.get_expr(*target),
        Expr::Member { obj, .. } if matches!(ast.get_expr(*obj), Expr::This)
    );
    let literal = match ast.get_expr(*value) {
        Expr::Number(_) | Expr::String(_) | Expr::Bool(_) | Expr::Null => true,
        Expr::Ident(n) => n == "undefined",
        _ => false,
    };
    this_member && literal
}

fn scan_ctor(ast: &Ast, ctor: &ClassCtor, fields: &HashSet<&str>) -> CtorScan {
    let last_marker = ctor
        .body
        .iter()
        .rposition(|s| matches!(s, Stmt::Expr(e) if super::private_brand::is_field_add(ast, *e)));
    let prefix = last_marker.map_or(&[][..], |i| &ctor.body[..=i]);
    let mut prefix_ids = HashSet::new();
    collect_stmts(ast, prefix, &mut prefix_ids);
    let mut ids = HashSet::new();
    for p in &ctor.params {
        if let Some(d) = p.default {
            collect_expr(ast, d, &mut ids);
        }
    }
    collect_stmts(ast, &ctor.body, &mut ids);
    CtorScan {
        prefix_escapes: this_escapes(ast, &prefix_ids, fields),
        escapes: this_escapes(ast, &ids, fields),
        init_writes: init_writes(ast, prefix),
        prefix_ids,
    }
}

/// Whether `this` is used in `ids` other than as the plain receiver of
/// a declared field read or write.
fn this_escapes(ast: &Ast, ids: &HashSet<ExprId>, fields: &HashSet<&str>) -> bool {
    let mut this_uses = 0usize;
    let mut field_uses: HashSet<ExprId> = HashSet::new();
    let mut callees: HashSet<ExprId> = HashSet::new();
    for &id in ids {
        match ast.get_expr(id) {
            Expr::This => this_uses += 1,
            Expr::Member { obj, name }
                if matches!(ast.get_expr(*obj), Expr::This) && fields.contains(name.as_str()) =>
            {
                field_uses.insert(id);
            }
            Expr::Call { callee, .. } | Expr::OptCall { callee, .. } => {
                callees.insert(*callee);
            }
            Expr::Ident(n) if n.starts_with("__super") || n == "eval" => return true,
            Expr::Closure { captures, .. } if !captures.is_empty() => return true,
            _ => {}
        }
    }
    this_uses > field_uses.difference(&callees).count()
}

/// The `this.#x = init` targets directly followed by `#x`'s add
/// marker — the initializer prefix the parser built.
fn init_writes(ast: &Ast, body: &[Stmt]) -> HashSet<ExprId> {
    let mut out = HashSet::new();
    for pair in body.windows(2) {
        let [Stmt::Expr(assign), Stmt::Expr(marker)] = pair else {
            continue;
        };
        let Expr::Assign { target, .. } = ast.get_expr(*assign) else {
            continue;
        };
        if let Expr::Member { name, .. } = ast.get_expr(*target)
            && added_field(ast, *marker).is_some_and(|m| m == *name)
        {
            out.insert(*target);
        }
    }
    out
}

/// The field a marker adds — before the rewrite a
/// `__torajs_priv_field_add("__priv_C__x")`, which is what the scan
/// sees (it runs on the markers' positions, not their rewrites).
fn added_field(ast: &Ast, eid: ExprId) -> Option<String> {
    let Expr::Call { args, .. } = ast.get_expr(eid) else {
        return None;
    };
    let [a] = args.as_slice() else { return None };
    match ast.get_expr(*a) {
        Expr::String(n) => Some(n.to_string_lossy_owned()),
        _ => None,
    }
}

fn collect_stmts(ast: &Ast, stmts: &[Stmt], ids: &mut HashSet<ExprId>) {
    for s in stmts {
        for e in stmt_exprs(s) {
            collect_expr(ast, e, ids);
        }
        collect_stmts_ref(ast, &stmt_children_ref(s), ids);
    }
}

fn collect_stmts_ref(ast: &Ast, stmts: &[&Stmt], ids: &mut HashSet<ExprId>) {
    for s in stmts {
        collect_stmts(ast, std::slice::from_ref(*s), ids);
    }
}

fn collect_expr(ast: &Ast, eid: ExprId, ids: &mut HashSet<ExprId>) {
    if !ids.insert(eid) {
        return;
    }
    if let Expr::ArrowFn { params, body, .. } = ast.get_expr(eid) {
        for p in params {
            if let Some(d) = p.default {
                collect_expr(ast, d, ids);
            }
        }
        collect_stmts(ast, body, ids);
    }
    for c in expr_children(ast, eid) {
        collect_expr(ast, c, ids);
    }
}
