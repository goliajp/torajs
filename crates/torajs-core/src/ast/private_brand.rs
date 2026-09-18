//! RFC 20260916-private-element-brand slice 2a — the private METHOD /
//! ACCESSOR brand (§7.3.33 PrivateMethodOrAccessorAdd, §7.3.34
//! InitializeInstanceElements).
//!
//! A class's private methods and accessors reify on its prototype, so
//! "the prototype chain has `#m`" used to be the brand — true for an
//! instance whose `super(…)` has not returned yet, and for
//! `Object.create(C.prototype)`. The spec installs the brand at a
//! point in time: a base class right after allocation, a derived
//! class right after `super(…)` returns. So each `Tag::Obj` instance
//! carries an install cursor in the high half of its +8 slot, counting
//! the brands installed so far along its class chain in root-to-leaf
//! order; a class owning private methods holds one position in that
//! order, and its brand is present exactly when the object is an
//! instance of the class and the cursor is past that position.
//!
//! Slice 2b adds the private FIELDS (§7.3.34 step 3): each one holds
//! its own position after its class's brand, in declaration order, and
//! is installed right after its own initializer ran — a
//! `__torajs_priv_field_add` marker the parser leaves in the
//! initializer prefix (`parser/class_field_inits.rs`), rewritten here.
//! A field no initializer precedes installs with its class's elements
//! start, where no user code of the class can run in between. Access
//! checks for fields are placed only where construction can still be
//! in progress (`private_field_guard.rs`).
//!
//! Chains that touch a value-returning constructor
//! (`Ast::ctor_return_override`) or an exotic builtin parent keep the
//! previous behaviour for now: their `this` may be an adopted object
//! with no cursor of this class, which needs the foreign-object store
//! of a later slice.

use super::desugar_classes_super::ClassIndexEntry;
use super::{Ast, Expr, ExprId, Stmt};
use std::collections::{HashMap, HashSet};

/// Runtime kernel names the class desugar mints.
pub(crate) const INSTALL: &str = "__torajs_priv_install";
pub(crate) const CHECK: &str = "__torajs_priv_brand";
/// The parser's marker for "this private field is added here".
pub(crate) const FIELD_ADD: &str = "__torajs_priv_field_add";
/// The parser's synthetic for `#x in o` (`parser/private_in.rs`).
pub(crate) const PRIV_IN_OP: &str = "__torajs_priv_in_op";

/// The receiver a `__torajs_priv_brand(obj, "C", pos[, __this])` check
/// answers, when `eid` is one.
pub(crate) fn checked_receiver(exprs: &[Expr], eid: ExprId) -> Option<ExprId> {
    let Some(Expr::Call { callee, args }) = exprs.get(eid.0 as usize) else {
        return None;
    };
    match (exprs.get(callee.0 as usize), args.as_slice()) {
        (Some(Expr::Ident(n)), [recv, _, _] | [recv, _, _, _]) if n == CHECK => Some(*recv),
        _ => None,
    }
}

/// Whether `eid` is a `__torajs_priv_field_add("__priv_C__x")` marker.
pub(crate) fn is_field_add(ast: &Ast, eid: ExprId) -> bool {
    field_add_name(ast, eid).is_some()
}

fn field_add_name(ast: &Ast, eid: ExprId) -> Option<String> {
    let Expr::Call { callee, args } = ast.get_expr(eid) else {
        return None;
    };
    if !matches!(ast.get_expr(*callee), Expr::Ident(n) if n == FIELD_ADD) {
        return None;
    }
    match args.as_slice() {
        [a] => match ast.get_expr(*a) {
            Expr::String(n) => Some(n.to_string_lossy_owned()),
            _ => None,
        },
        _ => None,
    }
}

/// Plan the element positions, guard the field access sites, rewrite
/// the field markers into installs, then guard the method access
/// sites. Runs once the class
/// parents are known and before any class rewrite: the `super(…)`
/// install consults the plan, and the access guards only touch member
/// receivers no class rewrite has renamed yet.
pub(super) fn run(ast: &mut Ast, class_index: &[ClassIndexEntry]) {
    let markers: Vec<(usize, String)> = (0..ast.exprs.len())
        .filter_map(|i| field_add_name(ast, ExprId(i as u32)).map(|n| (i, n)))
        .collect();
    let marked: HashSet<&str> = markers.iter().map(|(_, n)| n.as_str()).collect();
    let field_pos = plan(ast, class_index, &marked);
    let in_static = super::private_brand_static_in::static_body_exprs(ast);
    // The field guard reads the constructors as the parser left them:
    // markers not yet rewritten, no receiver wrapped yet.
    super::private_field_guard::guard_field_sites(ast, class_index, &field_pos, &in_static);
    // A run of markers no user code separates installs as one range.
    let runs = super::private_field_guard::coalesced_installs(ast, class_index, &field_pos);
    for (i, name) in &markers {
        let id = ExprId(*i as u32);
        ast.exprs[*i] = match (runs.get(&id), field_pos.get(name)) {
            (Some(Some((from, to))), _) => install_expr(ast, Expr::This, *from, *to),
            (None, Some((_, pos))) => install_expr(ast, Expr::This, *pos, *pos + 1),
            // Folded into its run's install, or an uncovered chain that
            // keeps the preinstalled field.
            (Some(None), _) | (None, None) => Expr::Ident("undefined".into()),
        };
    }
    let owner = method_owner(ast, class_index);
    guard_method_sites(ast, &owner, &in_static);
    guard_in_op_sites(ast, &owner, &field_pos, &in_static);
    super::private_brand_static_in::guard_sites(ast, &in_static);
}

/// The private methods / accessors (the brand, 0 or 1 position) and
/// the private instance fields a class declares. A unit class's hidden
/// evaluation slot is spelled like one and is none: nothing installs
/// it, so counting it left the cursor one short of every subclass's
/// base (`cannot add a private element twice`).
fn own_elements(entry: &ClassIndexEntry) -> (u32, Vec<&str>) {
    let (_, _, _, _, fields, _, _, methods, _) = entry;
    let brand = u32::from(methods.iter().any(|m| m.name.starts_with("__priv_")));
    let fields = fields
        .iter()
        .filter_map(|(k, _)| k.as_str())
        .filter(|n| n.starts_with("__priv_") && *n != super::class_unit::UNIT_SLOT)
        .collect();
    (brand, fields)
}

/// Record, for every class owning private elements on a chain this
/// slice covers, where its elements sit in the chain's install order
/// (`Ast::private_elements`), and answer each private field's
/// (owner class, position).
fn plan(
    ast: &mut Ast,
    class_index: &[ClassIndexEntry],
    marked: &HashSet<&str>,
) -> HashMap<String, (String, u32)> {
    let by_name: HashMap<&str, &ClassIndexEntry> =
        class_index.iter().map(|e| (e.1.as_str(), e)).collect();
    let count = |c: &str| {
        by_name.get(c).map_or(0, |e| {
            let (brand, fields) = own_elements(e);
            brand + fields.len() as u32
        })
    };
    let mut out = HashMap::new();
    let mut field_pos = HashMap::new();
    for entry in class_index {
        let cname = entry.1.as_str();
        let (brand, fields) = own_elements(entry);
        if brand == 0 && fields.is_empty() {
            continue;
        }
        let mut base = 0u32;
        let mut covered = true;
        let mut cur = Some(cname);
        while let Some(c) = cur {
            if ast.ctor_return_override.contains(c)
                || super::desugar_classes_builtin_heritage::exotic_root_parent(ast, c).is_some()
            {
                covered = false;
                break;
            }
            cur = by_name.get(c).and_then(|e| e.3.as_deref());
            base += cur.map_or(0, count);
        }
        if !covered {
            continue;
        }
        let head = fields.iter().take_while(|f| !marked.contains(**f)).count() as u32;
        out.insert(cname.to_string(), (base, brand + head));
        for (j, f) in fields.iter().enumerate() {
            field_pos.insert(f.to_string(), (cname.to_string(), base + brand + j as u32));
        }
    }
    ast.private_elements = out;
    field_pos
}

fn install_expr(ast: &mut Ast, recv: Expr, from: u32, to: u32) -> Expr {
    let callee = ast.add_expr(Expr::Ident(INSTALL.into()));
    let recv = ast.add_expr(recv);
    let from = ast.add_expr(Expr::Number(f64::from(from)));
    let to = ast.add_expr(Expr::Number(f64::from(to)));
    Expr::Call {
        callee,
        args: vec![recv, from, to],
    }
}

/// The install of `cname`'s elements start — its brand and the private
/// fields no initializer precedes — or `None` when there is nothing to
/// install there.
pub(crate) fn install_call(ast: &mut Ast, cname: &str, recv: Expr) -> Option<ExprId> {
    let (base, start) = *ast.private_elements.get(cname)?;
    if start == 0 {
        return None;
    }
    let call = install_expr(ast, recv, base, base + start);
    Some(ast.add_expr(call))
}

/// `__torajs_priv_brand(obj, "C", pos)` in place of the receiver of
/// member `i`. The receiver is still evaluated once and the node keeps
/// its shape, so every downstream pass (the class Pass 2 call rewrite,
/// `.call` devirtualisation, promise chains, the static-`this` table
/// keyed on the receiver's ExprId) sees what it saw before; the checker
/// types the call as its first operand.
pub(super) fn guard_receiver(
    ast: &mut Ast,
    i: usize,
    cname: &str,
    pos: u32,
    in_static: &HashMap<ExprId, String>,
) {
    let Expr::Member { obj, .. } = &ast.exprs[i] else {
        return;
    };
    let obj = *obj;
    let callee = ast.add_expr(Expr::Ident(CHECK.into()));
    let class = ast.add_expr(Expr::String(cname.into()));
    let pos = ast.add_expr(Expr::Number(f64::from(pos)));
    let mut args = vec![obj, class, pos];
    // A unit class's element belongs to one evaluation: the check also
    // compares the receiver's with the one the body runs for — the
    // instance's, or a static body's `this` (`ast::class_unit`).
    if ast.eval_unit_classes.contains(cname) {
        let site = ExprId(i as u32);
        args.push(super::private_brand_static_in::running_this(
            ast, in_static, site,
        ));
    }
    let checked = ast.add_expr(Expr::Call { callee, args });
    if let Expr::Member { obj, .. } = &mut ast.exprs[i] {
        *obj = checked;
    }
}

/// Each private method / accessor of a covered class, with the class
/// and the class's brand position (its first).
fn method_owner(ast: &Ast, class_index: &[ClassIndexEntry]) -> HashMap<String, (String, u32)> {
    let mut owner: HashMap<String, (String, u32)> = HashMap::new();
    for entry in class_index {
        let Some((base, _)) = ast.private_elements.get(&entry.1) else {
            continue;
        };
        for m in &entry.7 {
            if let Some(n) = m.name.as_str()
                && n.starts_with("__priv_")
            {
                owner.insert(n.to_string(), (entry.1.clone(), *base));
            }
        }
    }
    owner
}

/// Route the receiver of every access to a private method or accessor
/// of a covered class — the read, the write, the call — through the
/// brand check (§7.3.31 / §7.3.32 step 1).
fn guard_method_sites(
    ast: &mut Ast,
    owner: &HashMap<String, (String, u32)>,
    in_static: &HashMap<ExprId, String>,
) {
    if owner.is_empty() {
        return;
    }
    let sites: Vec<(usize, String, u32)> = ast
        .exprs
        .iter()
        .enumerate()
        .filter_map(|(i, e)| match e {
            Expr::Member { name, .. } => owner.get(name).map(|(c, p)| (i, c.clone(), *p)),
            _ => None,
        })
        .collect();
    for (i, cname, pos) in sites {
        guard_receiver(ast, i, &cname, pos, in_static);
    }
}

/// `#x in o` on a unit class's element (RFC 20260917-class-eval-unit
/// K3): the parser's `__torajs_priv_in_op("__priv_C__x", o)` grows the
/// class name and `__this`, so the lowering can also ask whether `o`
/// belongs to the evaluation the running instance does — another
/// evaluation's instance carries the same row and must answer false.
fn guard_in_op_sites(
    ast: &mut Ast,
    owner: &HashMap<String, (String, u32)>,
    field_pos: &HashMap<String, (String, u32)>,
    in_static: &HashMap<ExprId, String>,
) {
    let class_of = |name: &str| {
        owner
            .get(name)
            .or_else(|| field_pos.get(name))
            .map(|(c, _)| c.clone())
            .filter(|c| ast.eval_unit_classes.contains(c))
    };
    let sites: Vec<(usize, String)> = (0..ast.exprs.len())
        .filter_map(|i| {
            let Expr::Call { callee, args } = &ast.exprs[i] else {
                return None;
            };
            let [key, _] = args.as_slice() else {
                return None;
            };
            if !matches!(ast.get_expr(*callee), Expr::Ident(n) if n == PRIV_IN_OP) {
                return None;
            }
            let Expr::String(key) = ast.get_expr(*key) else {
                return None;
            };
            class_of(&key.to_string_lossy()).map(|c| (i, c))
        })
        .collect();
    for (i, cname) in sites {
        let class = ast.add_expr(Expr::String(cname.as_str().into()));
        let this = super::private_brand_static_in::running_this(ast, in_static, ExprId(i as u32));
        if let Expr::Call { args, .. } = &mut ast.exprs[i] {
            args.push(class);
            args.push(this);
        }
    }
}

/// The install statements for a factory body, placed right after the
/// mint.
///
/// §10.2.2 step 6.b installs a base class's brand as its [[Construct]]
/// begins, and for an instance of a derived class that is the moment
/// the innermost `super(…)` reaches the root; no code between the mint
/// and that moment can observe `this` (a derived constructor cannot
/// touch it before `super`), so the root's install goes at the mint for
/// every class of the chain — including the root's own factory, which
/// no `super` reaches. Every other class installs at its `super(…)`.
///
/// A factory that calls no constructor at all belongs to a chain with
/// none, so no `super` site exists and no user code runs between the
/// installs: the whole chain installs here, root first.
pub(crate) fn factory_install_stmts(ast: &mut Ast, cname: &str, calls_ctor: bool) -> Vec<Stmt> {
    let mut chain = vec![cname.to_string()];
    while let Some(Some(p)) = ast.class_parents.get(chain.last().unwrap()) {
        chain.push(p.clone());
    }
    chain.reverse();
    if calls_ctor {
        chain.truncate(1);
    }
    chain
        .iter()
        .filter_map(|c| install_call(ast, c, Expr::Ident("__this".into())))
        .map(Stmt::Expr)
        .collect()
}
