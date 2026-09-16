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
        if in_prefix || ancestor_escapes {
            super::private_brand::guard_receiver(ast, i, &cname, pos);
        }
    }
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
