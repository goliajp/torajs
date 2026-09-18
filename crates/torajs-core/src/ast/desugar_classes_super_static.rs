//! `super.<m>(args)` in a STATIC member body — a static method, a
//! static field initializer, a static block. The home object there is
//! the class, so the base is the parent CLASS and the target a static
//! method, run with the body's own `this` (§13.3.7.3).

use super::desugar_classes_super::{ClassIndexEntry, nearest_declaring};
use super::super_collect::collect_supercall_in_stmt;
use super::{Ast, ClassMethod, Expr, ExprId, StaticInit, Stmt};

/// Rewrite every static-body `super.<m>(args)` site of the class
/// `names.0` (whose parent is `names.1`): `__sm_<owner>__<m>(args)`
/// for a receiver-free target, the marked receiver-carrying form
/// otherwise, or the receiver-aware kernel when the program can
/// re-link a prototype (`dynamic`, see `super_home_dynamic`).
pub(super) fn rewrite_static_super_calls(
    ast: &mut Ast,
    class_index: &[ClassIndexEntry],
    names: (&String, &String),
    bodies: (&[ClassMethod], &[StaticInit]),
    dynamic: bool,
) {
    let (cname, parent_name) = names;
    let (static_methods, static_init) = bodies;
    let mut static_sites: Vec<(ExprId, String, Vec<ExprId>)> = Vec::new();
    for m in static_methods {
        for s in &m.body {
            collect_supercall_in_stmt(ast, s, &mut static_sites);
        }
    }
    // 420-04 — a static field initializer and a static block are
    // static member bodies too: §15.7.14 runs both with the class
    // as receiver, so their home object is the class and
    // `super.m()` names the parent CLASS. Only the method list was
    // walked, so those two positions kept the parser's raw
    // `__supercall__<m>` marker and died on it at typecheck. (A
    // bare `super(...)` CALL is a different question — the grammar
    // confines SuperCall to derived constructors, and the class
    // lane refuses it in a static body on its own.)
    for si in static_init {
        match si {
            StaticInit::Field(f) => {
                collect_supercall_in_stmt(ast, &Stmt::Expr(f.init), &mut static_sites);
            }
            StaticInit::Block(v) => {
                for s in v {
                    collect_supercall_in_stmt(ast, s, &mut static_sites);
                }
            }
        }
    }
    for (eid, m_name, args) in static_sites {
        // Same degrade as the instance sites, with the static home
        // object: `super.m()` in a static body names the parent
        // CLASS, whose link is `Object.getPrototypeOf(C)`.
        if dynamic {
            let site = super::desugar_classes_super_prop_mint::SiteCtx::runtime(cname, true);
            let base = site.base(ast);
            let key = ast.add_expr(Expr::String(m_name.into()));
            let recv = site.receiver(ast);
            let pack = ast.add_expr(Expr::Array(args));
            let kernel = ast.add_expr(Expr::Ident("__torajs_super_prop_call".to_string()));
            ast.exprs[eid.0 as usize] = Expr::Call {
                callee: kernel,
                args: vec![base, key, recv, pack],
            };
            continue;
        }
        let owner = nearest_declaring(class_index, parent_name, &m_name, true)
            .unwrap_or(parent_name.clone());
        let mono = format!("__sm_{owner}__{m_name}");
        // §13.3.7.3 — `super.m()` runs `m` with the CURRENT `this`, not
        // with the class that declares `m`. The mono `__sm_` body has no
        // receiver channel (its `this` is the declaring class, minted at
        // compile time), so `static t() { return super.s() }` answered A
        // from `B.t()`, and a `super.create()` factory built the parent.
        // A target that reads its receiver is called through its
        // receiver-polymorphic twin instead, with this body's own static
        // `this` — recorded like a parser-seen one, so pass 2 spells it
        // the class in the mono and the twin mint spells it `__this`
        // (which is also what gives THIS body a twin, so `C.t()` on a
        // subclass carries C all the way up). The twin exists only after
        // pass 3: the site keeps a marker until
        // `desugar_classes_static_route::resolve_static_super_calls`. A
        // receiver-free target keeps the direct mono call.
        // K5-1 — a unit class's static body takes the receiver as its
        // leading `__unit` param whatever it does with it, and mints no
        // twin: the mono IS receiver-polymorphic.
        if ast.eval_unit_classes.contains(&owner) {
            let this = ast.add_expr(Expr::This);
            ast.static_this_sites.insert(this, cname.clone());
            let callee = ast.add_expr(Expr::Ident(mono));
            let mut with_recv = Vec::with_capacity(args.len() + 1);
            with_recv.push(this);
            with_recv.extend(args);
            ast.exprs[eid.0 as usize] = Expr::Call {
                callee,
                args: with_recv,
            };
            continue;
        }
        if reads_receiver(ast, class_index, &owner, &m_name, 0) {
            let this = ast.add_expr(Expr::This);
            ast.static_this_sites.insert(this, cname.clone());
            let callee = ast.add_expr(Expr::Ident(format!("{STATIC_SUPER_MARK}{mono}")));
            let mut with_recv = Vec::with_capacity(args.len() + 1);
            with_recv.push(this);
            with_recv.extend(args);
            ast.exprs[eid.0 as usize] = Expr::Call {
                callee,
                args: with_recv,
            };
            continue;
        }
        let callee = ast.add_expr(Expr::Ident(mono));
        ast.exprs[eid.0 as usize] = Expr::Call { callee, args };
    }
}

/// Callee prefix of a static super call still waiting for its target's
/// twin: `<mark>__sm_<owner>__<m>(this, args…)`.
pub(super) const STATIC_SUPER_MARK: &str = "__ssuper";

/// Whether the static method `owner.m` observes its receiver: its body
/// says a static `this` (a marked super site of an earlier class counts,
/// it carries one), or still holds a raw `super.x()` whose own target
/// does. The hop bound guards a malformed `extends` cycle.
fn reads_receiver(
    ast: &Ast,
    class_index: &[ClassIndexEntry],
    owner: &str,
    m: &str,
    depth: u32,
) -> bool {
    let Some(entry) = class_index.iter().find(|e| e.1 == owner) else {
        return false;
    };
    let Some(method) = entry.8.iter().find(|sm| sm.name == m) else {
        return false;
    };
    if super::capturing_classes::this_sites(ast, &method.body)
        .iter()
        .any(|e| ast.static_this_sites.contains_key(e))
    {
        return true;
    }
    let Some(parent) = entry.3.as_ref().filter(|_| depth < 64) else {
        return false;
    };
    let mut raw: Vec<(ExprId, String, Vec<ExprId>)> = Vec::new();
    for s in &method.body {
        collect_supercall_in_stmt(ast, s, &mut raw);
    }
    raw.iter().any(|(_, target, _)| {
        nearest_declaring(class_index, parent, target, true)
            .is_some_and(|o| reads_receiver(ast, class_index, &o, target, depth + 1))
    })
}
